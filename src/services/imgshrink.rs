//! Making a screenshot or a photo smaller before it is filed — the image's
//! half of what `services::pdftrim` does for a PDF.
//!
//! A phone screenshot arrives as 16-bit RGBA with a few hundred colors in it:
//! a bank's receipt is flat fills and antialiased text. Almost all of its
//! size is the alpha channel, the second byte per channel and the colors
//! nobody can tell apart. So, in pure Rust — no program on the server, unlike
//! Ghostscript:
//!
//! - **every image** is laid over white, taken to 8 bits a channel and, past
//!   [`MAX_SIDE`], scaled down;
//! - **a PNG** becomes a palette PNG — exact when it has 256 colors or fewer,
//!   otherwise [`PNG_COLORS`] picked by NeuQuant, **never dithered**: dither
//!   noise is what a deflate stream cannot compress, and it took a 69 KB
//!   screenshot to 205 KB;
//! - **a JPEG** is turned upright by its EXIF orientation — re-encoding drops
//!   the tag, and a phone photo would be filed sideways — and re-encoded at
//!   [`JPEG_QUALITY`].
//!
//! The extension never changes: the format of the file is what its name says,
//! because `receipts::store` names the vault's copy from the upload's name.
//! A file whose bytes and name disagree is left as it came. And a shrink that
//! does not come out smaller is thrown away, the rule `pdftrim` follows.

use std::collections::HashMap;
use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader, RgbImage};

/// The longest side a filed image keeps — a phone photo is 4032 px, and at
/// 2400 a boleto's digits are still sharp at the carousel's full width.
const MAX_SIDE: u32 = 2400;

/// A screenshot past 256 colors is cut to this many. Its text is a dark ink
/// antialiased onto one or two grounds, and 32 shades of that read the same
/// as the original: a 297 KB Nubank receipt came to 58 KB, against 64 KB at
/// 64 colors and 47 KB at 16. Fewer is a smaller file and less room for the
/// one colored link or a logo, which NeuQuant gets the fewest samples of.
const PNG_COLORS: usize = 32;

/// q80: a photographed bill's digits stay crisp, the paper's texture goes.
const JPEG_QUALITY: u8 = 80;

/// The two kinds of image a slot takes (`domain::receipt_path::EXTENSIONS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Png,
    Jpeg,
}

impl Kind {
    /// What the bytes are, by their magic number — never by the name alone.
    fn of_bytes(bytes: &[u8]) -> Option<Kind> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Kind::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Kind::Jpeg)
        } else {
            None
        }
    }

    fn of_name(name: &str) -> Option<Kind> {
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        match ext.as_str() {
            "png" => Some(Kind::Png),
            "jpg" | "jpeg" => Some(Kind::Jpeg),
            _ => None,
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Kind::Png => "image/png",
            Kind::Jpeg => "image/jpeg",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Kind::Png => "png",
            Kind::Jpeg => "jpg",
        }
    }
}

/// The image `name` and `bytes` agree it is — `None` for a PDF, a text file,
/// or a PNG named `.jpg`, which is filed exactly as it came.
pub fn kind(name: &str, bytes: &[u8]) -> Option<Kind> {
    let by_bytes = Kind::of_bytes(bytes)?;
    (Kind::of_name(name) == Some(by_bytes)).then_some(by_bytes)
}

/// The shrunk image, or `None` when it would not be smaller (or `name` and
/// `bytes` are not one of the images [`kind`] knows). **Blocking**: decoding
/// a phone photo takes a moment.
pub fn shrink_bytes(name: &str, bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let Some(kind) = kind(name, bytes) else {
        return Ok(None);
    };
    let shrunk = match kind {
        Kind::Png => png_palette(&flat(decode(bytes)?))?,
        Kind::Jpeg => jpeg(&flat(decode(bytes)?))?,
    };
    Ok((shrunk.len() < bytes.len()).then_some(shrunk))
}

/// Decoded and turned upright. Only a JPEG carries an orientation in
/// practice, but asking a PNG costs nothing.
fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// Over white, 8 bits a channel, no side past [`MAX_SIDE`].
fn flat(img: DynamicImage) -> RgbImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut rgb = RgbImage::new(w, h);
    for (out, px) in rgb.pixels_mut().zip(rgba.pixels()) {
        let [r, g, b, a] = px.0;
        let a = u32::from(a);
        let over = |c: u8| ((u32::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
        out.0 = [over(r), over(g), over(b)];
    }
    let long = w.max(h);
    if long <= MAX_SIDE {
        return rgb;
    }
    let scale =
        |side: u32| ((u64::from(side) * u64::from(MAX_SIDE) / u64::from(long)) as u32).max(1);
    image::imageops::resize(&rgb, scale(w), scale(h), FilterType::Lanczos3)
}

/// A palette PNG: the image's own colors when there are 256 or fewer, else
/// NeuQuant's [`PNG_COLORS`], each pixel mapped to its nearest — no dither.
fn png_palette(rgb: &RgbImage) -> Result<Vec<u8>, String> {
    let mut exact: HashMap<[u8; 3], u8> = HashMap::new();
    for px in rgb.pixels() {
        if exact.len() > 256 {
            break;
        }
        let n = exact.len();
        exact.entry(px.0).or_insert(n as u8);
    }
    let (palette, indices): (Vec<u8>, Vec<u8>) = if exact.len() <= 256 {
        let mut palette = vec![0u8; exact.len() * 3];
        for (color, &i) in &exact {
            palette[usize::from(i) * 3..][..3].copy_from_slice(color);
        }
        let indices = rgb.pixels().map(|px| exact[&px.0]).collect();
        (palette, indices)
    } else {
        let rgba: Vec<u8> = rgb
            .pixels()
            .flat_map(|px| [px.0[0], px.0[1], px.0[2], 255])
            .collect();
        let quant = color_quant::NeuQuant::new(10, PNG_COLORS, &rgba);
        // A screenshot repeats a handful of colors across millions of pixels:
        // look each one up once.
        let mut seen: HashMap<[u8; 3], u8> = HashMap::new();
        let indices = rgb
            .pixels()
            .map(|px| {
                *seen
                    .entry(px.0)
                    .or_insert_with(|| quant.index_of(&[px.0[0], px.0[1], px.0[2], 255]) as u8)
            })
            .collect();
        (quant.color_map_rgb(), indices)
    };
    write_indexed(rgb.width(), rgb.height(), &palette, &indices)
}

/// Packs the indices at the smallest depth the palette fits — a 16-color
/// screenshot is two pixels a byte before deflate ever sees it.
fn write_indexed(w: u32, h: u32, palette: &[u8], indices: &[u8]) -> Result<Vec<u8>, String> {
    let colors = palette.len() / 3;
    let (depth, bits) = match colors {
        0..=2 => (png::BitDepth::One, 1),
        3..=4 => (png::BitDepth::Two, 2),
        5..=16 => (png::BitDepth::Four, 4),
        _ => (png::BitDepth::Eight, 8),
    };
    let per_byte = 8 / bits;
    let row_bytes = (w as usize).div_ceil(per_byte);
    let mut packed = Vec::with_capacity(row_bytes * h as usize);
    for row in indices.chunks(w as usize) {
        for chunk in row.chunks(per_byte) {
            let mut byte = 0u8;
            for (k, &i) in chunk.iter().enumerate() {
                byte |= i << (8 - bits * (k + 1));
            }
            packed.push(byte);
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(depth);
    enc.set_palette(palette.to_vec());
    enc.set_compression(png::Compression::High);
    // A palette index is a label, not a quantity: predicting it from its
    // neighbors (Sub, Paeth, the adaptive mix) only scrambles the runs deflate
    // feeds on. Unfiltered was 13% smaller on that receipt.
    enc.set_filter(png::Filter::NoFilter);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer
        .write_image_data(&packed)
        .map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

fn jpeg(rgb: &RgbImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode_image(rgb)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    /// A screenshot's shape: a 16-bit RGBA ground with text-like strokes in
    /// a few antialiased shades — few colors, a lot of bytes per pixel.
    fn screenshot_png() -> Vec<u8> {
        let img: ImageBuffer<Rgba<u16>, Vec<u16>> = ImageBuffer::from_fn(400, 300, |x, y| {
            let ink = if (x / 3 + y / 7) % 5 == 0 {
                (x % 4) as u16 * 16000
            } else {
                65535
            };
            let ground = if y > 150 { 61000 } else { ink };
            Rgba([ground, ground, ground, 65535])
        });
        let mut out = Vec::new();
        DynamicImage::ImageRgba16(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn a_16_bit_screenshot_becomes_a_smaller_palette_png() {
        let original = screenshot_png();
        let shrunk = shrink_bytes("10_scheduled.png", &original)
            .unwrap()
            .unwrap();
        assert!(shrunk.len() < original.len());
        let decoder = png::Decoder::new(Cursor::new(&shrunk));
        let reader = decoder.read_info().unwrap();
        assert_eq!(reader.info().color_type, png::ColorType::Indexed);
        assert_eq!(reader.info().bit_depth, png::BitDepth::Four);
        let back = image::load_from_memory(&shrunk).unwrap().to_rgb8();
        assert_eq!(back.dimensions(), (400, 300));
    }

    #[test]
    fn a_lean_png_is_left_alone() {
        let shrunk = shrink_bytes("a.png", &screenshot_png()).unwrap().unwrap();
        assert_eq!(shrink_bytes("a.png", &shrunk).unwrap(), None);
    }

    #[test]
    fn many_colors_are_cut_to_the_palette() {
        let img = RgbImage::from_fn(300, 200, |x, y| {
            image::Rgb([x as u8, y as u8, (x ^ y) as u8])
        });
        let png = png_palette(&img).unwrap();
        let reader = png::Decoder::new(Cursor::new(&png)).read_info().unwrap();
        assert_eq!(
            reader.info().palette.as_ref().unwrap().len(),
            PNG_COLORS * 3
        );
    }

    #[test]
    fn transparency_is_laid_over_white() {
        let img = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2, 2, Rgba([0, 0, 0, 0])));
        assert_eq!(flat(img).get_pixel(0, 0).0, [255, 255, 255]);
    }

    #[test]
    fn a_big_photo_is_scaled_down_keeping_its_shape() {
        let img = DynamicImage::ImageRgb8(RgbImage::new(4032, 3024));
        assert_eq!(flat(img).dimensions(), (2400, 1800));
    }

    /// EXIF orientation 6 ("rotate 90° clockwise"), the one an iPhone held
    /// upright writes: a JPEG stored 40×20 must come out 20×40.
    #[test]
    fn a_jpeg_is_turned_upright_before_it_loses_its_exif() {
        let mut plain = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 95)
            .encode_image(&RgbImage::from_pixel(40, 20, image::Rgb([200, 30, 30])))
            .unwrap();
        // APP1 right after SOI: "Exif\0\0", a big-endian TIFF header, one IFD
        // entry — Orientation (0x0112), SHORT, 1, value 6.
        let tiff: &[u8] = &[
            b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0,
            0,
        ];
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(tiff);
        let len = (app1.len() + 2) as u16;
        let mut tagged = plain[..2].to_vec();
        tagged.extend_from_slice(&[0xFF, 0xE1]);
        tagged.extend_from_slice(&len.to_be_bytes());
        tagged.extend_from_slice(&app1);
        tagged.extend_from_slice(&plain[2..]);

        let upright = decode(&tagged).unwrap();
        assert_eq!((upright.width(), upright.height()), (20, 40));
    }

    #[test]
    fn names_and_bytes_must_agree() {
        let png = screenshot_png();
        assert_eq!(kind("a.PNG", &png), Some(Kind::Png));
        assert_eq!(kind("a.jpg", &png), None);
        assert_eq!(kind("a.pdf", b"%PDF-1.4"), None);
        assert_eq!(shrink_bytes("a.jpg", &png).unwrap(), None);
    }

    #[test]
    fn a_broken_image_is_an_error_not_a_panic() {
        let mut png = screenshot_png();
        png.truncate(60);
        assert!(shrink_bytes("a.png", &png).is_err());
    }
}
