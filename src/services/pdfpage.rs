//! A PDF's pages as images — the `P` carousel's and the trim view's.
//!
//! **JPEG, not PNG.** Measured on the household's own bills, drawing a page
//! is cheap and PNG's encoder is not:
//!
//! | bill | PNG 300 dpi | JPEG 300 dpi |
//! |---|---|---|
//! | Nubank, 8 pages of text, all at once | 907 ms | 130 ms |
//! | DAAE, 2 pages with embedded images | 4 909 ms | 187 ms |
//! | Damha III, 1 page | 675 ms | 147 ms |
//! | the trim view's large page (150 dpi), DAAE | 283 ms | 75 ms |
//!
//! A JPEG of a text page is about twice the bytes of the PNG — milliseconds
//! on the household's LAN, against seconds of encoding. Quality 90 keeps a
//! boleto's digits and its QR code crisp at 300 dpi.

use std::path::Path;
use std::process::Command;

/// One page of `path`, `page` counted from 1, drawn at `dpi` as a JPEG.
/// `None` when poppler cannot draw it. Blocking: a subprocess, so callers
/// run it on `spawn_blocking`.
pub fn render(path: &Path, page: u32, dpi: &str) -> Option<Vec<u8>> {
    let n = page.to_string();
    let out = Command::new("pdftoppm")
        .args(["-f", &n, "-l", &n, "-r", dpi])
        .args(["-jpeg", "-jpegopt", "quality=90", "-singlefile"])
        .arg(path)
        .output()
        .ok()?;
    (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
}

/// Every page's box, as it is shown: width and height in points, swapped for
/// a page turned 90° or 270° (`pdftoppm` draws it turned). `pdfinfo -f 1 -l
/// N` prints them; blocking, like [`render`].
///
/// The carousel puts them on each page's `<img>` as `width`/`height`, which a
/// browser turns into the image's aspect ratio before it has loaded — so a
/// lazy page not yet drawn already takes its real place, and scrolling to it
/// lands where it will be.
pub fn boxes(path: &Path) -> Vec<(u32, u32)> {
    let Ok(out) = Command::new("pdfinfo")
        .args(["-f", "1", "-l", "99999"])
        .arg(path)
        .output()
    else {
        return Vec::new();
    };
    parse_boxes(&String::from_utf8_lossy(&out.stdout))
}

/// The pure half of [`boxes`]: `Page    1 size: 595 x 842 pts (A4)` and
/// `Page    1 rot:   90` lines, in page order.
pub fn parse_boxes(info: &str) -> Vec<(u32, u32)> {
    let mut pages: Vec<(u32, u32, u32)> = Vec::new();
    for line in info.lines() {
        let Some(rest) = line.strip_prefix("Page") else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let (Some(n), Some(what)) = (words.next(), words.next()) else {
            continue;
        };
        let Ok(n) = n.parse::<usize>() else { continue };
        if n == 0 {
            continue;
        }
        if pages.len() < n {
            pages.resize(n, (0, 0, 0));
        }
        match what {
            "size:" => {
                let w = words.next().and_then(|w| w.parse::<f64>().ok());
                let _x = words.next();
                let h = words.next().and_then(|h| h.parse::<f64>().ok());
                if let (Some(w), Some(h)) = (w, h) {
                    pages[n - 1].0 = w.round() as u32;
                    pages[n - 1].1 = h.round() as u32;
                }
            }
            "rot:" => {
                if let Some(r) = words.next().and_then(|r| r.parse::<u32>().ok()) {
                    pages[n - 1].2 = r % 360;
                }
            }
            _ => {}
        }
    }
    pages
        .into_iter()
        .map(|(w, h, rot)| {
            if rot == 90 || rot == 270 {
                (h, w)
            } else {
                (w, h)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_has_its_box_and_a_turned_page_is_swapped() {
        let info = "Pages:           3\n\
                    Page    1 size:  595 x 842 pts (A4)\n\
                    Page    1 rot:   0\n\
                    Page    2 size:  595.276 x 841.89 pts (A4)\n\
                    Page    2 rot:   90\n\
                    Page    3 size:  446 x 1200 pts\n\
                    Page    3 rot:   0\n";
        assert_eq!(parse_boxes(info), vec![(595, 842), (842, 595), (446, 1200)]);
    }

    #[test]
    fn nothing_to_read_is_no_pages() {
        assert!(parse_boxes("").is_empty());
        assert!(parse_boxes("Pages: 2\nTitle: x\n").is_empty());
    }
}
