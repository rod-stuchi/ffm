//! One PDF page carrying a bill's barcode, drawn from its code.
//!
//! For a bill that reached the household without a usable barcode — a
//! statement, a scan, a code from an email — `P`'s *Add barcode* appends this
//! page to the PDF on show, through the trim view (`web::prepare`).
//!
//! **Written by hand, not by a library.** One page, a few rectangles and some
//! text: a minimal PDF 1.4 with the standard fonts (Helvetica, Courier), which
//! every reader has, so nothing is embedded and no dependency is added. The
//! bars are filled rectangles at exact widths — a PDF is vector, so the code
//! is as sharp at any zoom as the screen a phone is held up to.
//!
//! **The page is a readable bill too.** The code is printed as text above the
//! bars and the 44 digits under them, so the app's own reader
//! (`import::candidates` + `boleto::parse`) finds it from the page's text,
//! exactly as it finds one on a bill a company issued.

use crate::services::itf;

/// What the page says. Every string arrives already worded and formatted —
/// the catalog and `fmt` are the web layer's — so this module only lays out.
#[derive(Clone, Debug, Default)]
pub struct PageInfo {
    pub heading: String,
    /// `(label, value)`, in the order they are printed.
    pub details: Vec<(String, String)>,
    /// The code as a person types it, in its printed groups.
    pub typed: String,
    /// The 44 digits the bars encode.
    pub barcode: String,
    pub footer: String,
}

/// Points. Half an inch: inside every printer's unprintable edge.
const MARGIN: f32 = 36.0;
/// Tall enough to scan comfortably from a screen, and well over FEBRABAN's
/// 13 mm (≈ 37 pt) minimum.
const BAR_HEIGHT: f32 = 72.0;
/// The quiet zone each side of the bars, in narrow units — ITF needs ten.
const QUIET: f32 = 10.0;

/// A4, when the page it follows has no size we could read.
pub const A4: (f32, f32) = (595.28, 841.89);

/// The page, as the bytes of a one-page PDF. `None` when `info.barcode` is not
/// something ITF can draw (an odd count, a non-digit).
pub fn page(info: &PageInfo, width: f32, height: f32) -> Option<Vec<u8>> {
    let elements = itf::encode(&info.barcode)?;
    let mut c = String::new();

    let mut y = head(&mut c, info, height);
    y -= 8.0;
    text(&mut c, "F2", 12.0, MARGIN, y, &info.typed);
    y -= 28.0;

    // --- the bars: the full width inside the margins -------------------
    let unit = (width - 2.0 * MARGIN) / (itf::units(&elements) as f32 + 2.0 * QUIET);
    let bars_bottom = y - BAR_HEIGHT;
    let mut x = MARGIN + QUIET * unit;
    c.push_str("0 0 0 rg\n");
    for e in &elements {
        let w = e.units() as f32 * unit;
        if e.bar {
            c.push_str(&format!(
                "{} {} {} {} re f\n",
                fmt(x),
                fmt(bars_bottom),
                fmt(w),
                fmt(BAR_HEIGHT)
            ));
        }
        x += w;
    }

    // The 44 digits under the bars, centered: Courier is 0.6 em per glyph.
    let size = 10.0;
    let digits_w = info.barcode.chars().count() as f32 * 0.6 * size;
    text(
        &mut c,
        "F2",
        size,
        (width - digits_w) / 2.0,
        bars_bottom - 16.0,
        &info.barcode,
    );

    text(&mut c, "F1", 8.0, MARGIN, MARGIN, &info.footer);

    Some(write_pdf(width, height, c.as_bytes()))
}

/// The same page with no code on it: the heading, the details and the
/// footer. For a document that has no barcode to draw — a receipt, a
/// scheduled-payment proof — written by the dev-only `ffm-anon` as a stand-in
/// for a real file it must not copy.
pub fn text_page(info: &PageInfo, width: f32, height: f32) -> Vec<u8> {
    let mut c = String::new();
    head(&mut c, info, height);
    text(&mut c, "F1", 8.0, MARGIN, MARGIN, &info.footer);
    write_pdf(width, height, c.as_bytes())
}

/// The heading and the details, top down. Returns where the next line goes.
fn head(c: &mut String, info: &PageInfo, height: f32) -> f32 {
    let mut y = height - MARGIN - 16.0;
    text(c, "F3", 16.0, MARGIN, y, &info.heading);
    y -= 30.0;
    for (label, value) in &info.details {
        text(c, "F1", 11.0, MARGIN, y, label);
        text(c, "F1", 11.0, MARGIN + 130.0, y, value);
        y -= 17.0;
    }
    y
}

/// A line of text at `(x, y)`, its baseline.
fn text(c: &mut String, font: &str, size: f32, x: f32, y: f32, s: &str) {
    c.push_str(&format!(
        "BT /{font} {} Tf {} {} Td ({}) Tj ET\n",
        fmt(size),
        fmt(x),
        fmt(y),
        escape(s)
    ));
}

/// Two decimals, no exponent — what a PDF number looks like.
fn fmt(v: f32) -> String {
    format!("{v:.2}")
}

/// A PDF literal string body in WinAnsi (cp1252) — the encoding the standard
/// fonts are declared with below, which covers Portuguese and the punctuation
/// this app writes. Anything else becomes `?` rather than garbage.
fn escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        let b: u8 = match ch {
            '(' | ')' | '\\' => {
                out.push('\\');
                ch as u8
            }
            c if (c as u32) < 0x80 => c as u8,
            c if (0xA0..=0xFF).contains(&(c as u32)) => c as u32 as u8,
            '€' => 0x80,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            _ => b'?',
        };
        if b < 0x80 {
            out.push(b as char);
        } else {
            // An octal escape keeps the content stream plain ASCII.
            out.push_str(&format!("\\{b:03o}"));
        }
    }
    out
}

/// The file around one content stream: catalog, pages, the page, three
/// standard fonts, the stream, the cross-reference table.
fn write_pdf(width: f32, height: f32, content: &[u8]) -> Vec<u8> {
    let font = |name: &str| {
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /{name} /Encoding /WinAnsiEncoding >>")
            .into_bytes()
    };
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] \
             /Resources << /Font << /F1 4 0 R /F2 5 0 R /F3 6 0 R >> >> /Contents 7 0 R >>",
            fmt(width),
            fmt(height)
        )
        .into_bytes(),
        font("Helvetica"),
        font("Courier"),
        font("Helvetica-Bold"),
        {
            let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            s.extend_from_slice(content);
            s.extend_from_slice(b"\nendstream");
            s
        },
    ];

    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn info() -> PageInfo {
        PageInfo {
            heading: "Código de barras · conta de consumo".into(),
            details: vec![
                ("Valor".into(), "R$ 5,31".into()),
                ("Empresa".into(), "0024".into()),
                ("Segmento".into(), "saneamento".into()),
            ],
            typed: "82660000000-2 05310024001-8 00097682201-5 10202620003-4".into(),
            barcode: "82660000000053100240010009768220110202620003".into(),
            footer: "Água · 10/2026 — feito pelo ffm em 07/10/2026".into(),
        }
    }

    fn tool(name: &str) -> bool {
        Command::new(name).arg("-v").output().is_ok()
    }

    fn written() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ffm-barcode-page-{}-{:?}.pdf",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, page(&info(), A4.0, A4.1).unwrap()).unwrap();
        path
    }

    #[test]
    fn an_odd_barcode_is_not_drawn() {
        let mut i = info();
        i.barcode.pop();
        assert!(page(&i, A4.0, A4.1).is_none());
    }

    #[test]
    fn text_is_winansi_and_parentheses_are_escaped() {
        assert_eq!(escape("(a)"), "\\(a\\)");
        assert_eq!(escape("ç"), "\\347");
        assert_eq!(escape("—"), "\\227");
        assert_eq!(escape("☃"), "?");
    }

    #[test]
    fn the_page_is_a_valid_pdf() {
        if !tool("qpdf") {
            return;
        }
        let out = Command::new("qpdf")
            .arg("--check")
            .arg(written())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    #[test]
    fn the_page_reads_back_as_the_same_bill() {
        // The generated page is a bill the app can read: its own reader finds
        // the code in the page's text and parses it to the same barcode.
        if !tool("pdftotext") {
            return;
        }
        let path = written();
        let text = crate::services::import::text_of(&path).unwrap();
        assert!(text.contains("R$ 5,31"), "{text}");
        assert!(text.contains("Água"), "accents survive: {text}");
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let found = crate::services::import::candidates(&text)
            .into_iter()
            .find_map(|c| crate::services::boleto::parse(&c, today).ok())
            .expect("a barcode in the page's text");
        assert_eq!(found.raw, info().barcode);
    }
}
