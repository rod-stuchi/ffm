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
//!
//! **Three at a time** ([`DRAWING`]), and **a vault file's pages are kept on
//! disk once drawn** ([`cached`]), beside the database and never in the vault,
//! which is browsed by hand. The browser keeps a page too, but only that
//! browser: a second device, or the installed app beside Safari, drew every
//! page again.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;

/// How many pages are drawn at once. Opening `P` on a row with three
/// statements asked for every page near the view of all three stages
/// together — a couple of dozen `pdftoppm` runs, each a core, on a server
/// that may be a small box. Queued in the order they were asked for, the
/// page on show, asked for first, is drawn first.
static DRAWING: Semaphore = Semaphore::const_new(3);

/// How long a drawn page is kept, counted from when it was drawn. A bill is
/// looked at in its month and seldom after; one opened later is drawn again,
/// once. A replaced or moved file's drawings are never read again and wait
/// here for the sweep.
const KEEP: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// One page of `path`, `page` counted from 1, drawn at `dpi` as a JPEG.
/// `None` when poppler cannot draw it. Blocking: a subprocess — callers go
/// through [`draw`] or [`cached`], which run it off the async runtime.
pub fn render(path: &Path, page: u32, dpi: u32) -> Option<Vec<u8>> {
    let n = page.to_string();
    let out = Command::new("pdftoppm")
        .args(["-f", &n, "-l", &n, "-r", &dpi.to_string()])
        .args(["-jpeg", "-jpegopt", "quality=90", "-singlefile"])
        .arg(path)
        .output()
        .ok()?;
    (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
}

/// [`render`], off the runtime and at most [`DRAWING`] at a time — for a
/// file that is not kept: the trim view's, staged under a single-use token.
pub async fn draw(path: PathBuf, page: u32, dpi: u32) -> Option<Vec<u8>> {
    let permit = DRAWING.acquire().await.ok()?;
    tokio::task::spawn_blocking(move || {
        // Held until the page is drawn, even when the request that asked for
        // it is gone: the process runs to the end either way.
        let _permit = permit;
        render(&path, page, dpi)
    })
    .await
    .ok()?
}

/// One page of a vault file, from `cache` when this version of the file was
/// drawn at this `dpi` before; else drawn, at most [`DRAWING`] at a time, and
/// kept there. A drawing that cannot be kept is still answered.
pub async fn cached(cache: PathBuf, path: PathBuf, page: u32, dpi: u32) -> Option<Vec<u8>> {
    let meta = tokio::fs::metadata(&path).await.ok()?;
    let kept = cache.join(entry_name(
        &path,
        meta.len(),
        nanos(meta.modified().ok()),
        page,
        dpi,
    ));
    if let Ok(jpeg) = tokio::fs::read(&kept).await {
        return Some(jpeg);
    }
    let permit = DRAWING.acquire().await.ok()?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        // The same page asked for twice at once — two devices — waited
        // behind the first ask, which has kept it by now.
        if let Ok(jpeg) = std::fs::read(&kept) {
            return Some(jpeg);
        }
        let jpeg = render(&path, page, dpi)?;
        keep(&kept, &jpeg);
        Some(jpeg)
    })
    .await
    .ok()?
}

/// `{hash}-{page}-{dpi}.jpg`, the hash over the file's path, size and
/// modified time — what the page's `?v=` carries, to the nanosecond. A file
/// replaced in its slot, or moved out of it, is a new name, so an old drawing
/// can never be answered for it.
fn entry_name(path: &Path, len: u64, mtime: u128, page: u32, dpi: u32) -> String {
    let mut h = blake3::Hasher::new();
    h.update(path.as_os_str().as_encoded_bytes());
    h.update(&len.to_le_bytes());
    h.update(&mtime.to_le_bytes());
    format!("{}-{page}-{dpi}.jpg", &h.finalize().to_hex()[..32])
}

fn nanos(t: Option<SystemTime>) -> u128 {
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos())
}

/// Writes a drawing beside its final name and renames it into place, so a
/// page read while another is being kept is whole or absent, never half.
fn keep(kept: &Path, jpeg: &[u8]) {
    static N: AtomicU64 = AtomicU64::new(0);
    let tmp = kept.with_extension(format!(
        "jpg.tmp-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let done = kept
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&tmp, jpeg))
        .and_then(|()| std::fs::rename(&tmp, kept));
    if let Err(e) = done {
        let _ = std::fs::remove_file(&tmp);
        // Once: a folder that cannot be written fails every page alike.
        static SAID: AtomicBool = AtomicBool::new(false);
        if !SAID.swap(true, Ordering::Relaxed) {
            tracing::warn!("pages · cannot keep drawn pages in {}: {e}", kept.display());
        }
    }
}

/// Drops the drawings older than [`KEEP`] — and only files this module
/// named, so anything else in the folder stays. `now` is the real time, not
/// `clock::now()`: it is compared with the files' own modified times, which a
/// dev server's moved clock does not move. Blocking: one pass over one
/// folder.
pub fn sweep(cache: &Path, now: SystemTime) -> usize {
    let Ok(dir) = std::fs::read_dir(cache) else {
        return 0;
    };
    let mut gone = 0;
    for entry in dir.flatten() {
        if !entry.file_name().to_str().is_some_and(ours) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > KEEP);
        if old && std::fs::remove_file(entry.path()).is_ok() {
            gone += 1;
        }
    }
    gone
}

/// A name [`entry_name`] or [`keep`] wrote: 32 hex digits, a dash, and a
/// `.jpg` (or a `.jpg.tmp-…` a crash left behind).
fn ours(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 33
        && b[..32].iter().all(u8::is_ascii_hexdigit)
        && b[32] == b'-'
        && name.contains(".jpg")
}

/// One sweep now, then one a day, as the backup runs.
pub fn spawn_sweep(cache: PathBuf) {
    tokio::spawn(async move {
        loop {
            let dir = cache.clone();
            let gone = tokio::task::spawn_blocking(move || sweep(&dir, SystemTime::now()))
                .await
                .unwrap_or(0);
            if gone > 0 {
                tracing::info!("pages · {gone} drawn page(s) older than 30 days dropped");
            }
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
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
    fn a_drawing_is_named_after_the_file_as_it_is_now() {
        let base = entry_name(Path::new("a/2026/10.pdf"), 1000, 7, 1, 150);
        assert!(ours(&base), "{base}");
        assert!(base.ends_with("-1-150.jpg"));
        // Replaced (size, time) or moved (path): a name never answered before.
        for other in [
            entry_name(Path::new("a/2026/10.pdf"), 1001, 7, 1, 150),
            entry_name(Path::new("a/2026/10.pdf"), 1000, 8, 1, 150),
            entry_name(Path::new("b/2026/10.pdf"), 1000, 7, 1, 150),
            entry_name(Path::new("a/2026/10.pdf"), 1000, 7, 2, 150),
            entry_name(Path::new("a/2026/10.pdf"), 1000, 7, 1, 300),
        ] {
            assert_ne!(other, base);
        }
    }

    #[test]
    fn the_sweep_drops_old_drawings_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("ffm-pages-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = entry_name(Path::new("x.pdf"), 1, 1, 1, 150);
        let fresh = entry_name(Path::new("x.pdf"), 1, 1, 2, 150);
        let crashed = old.replace(".jpg", ".jpg.tmp-1-0");
        for name in [&old, &fresh, &crashed, &"notes.txt".to_string()] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let month_ago = SystemTime::now() - KEEP - Duration::from_secs(60);
        for name in [&old, &crashed, &"notes.txt".to_string()] {
            std::fs::File::options()
                .write(true)
                .open(dir.join(name))
                .unwrap()
                .set_modified(month_ago)
                .unwrap();
        }
        assert_eq!(sweep(&dir, SystemTime::now()), 2);
        assert!(!dir.join(&old).exists() && !dir.join(&crashed).exists());
        assert!(dir.join(&fresh).exists(), "drawn this month");
        assert!(dir.join("notes.txt").exists(), "not a name the cache wrote");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_page_is_drawn_once_and_again_when_its_file_is_replaced() {
        if Command::new("pdftoppm").arg("-v").output().is_err() {
            eprintln!("pdftoppm not on PATH — skipping");
            return;
        }
        let dir = std::env::temp_dir().join(format!("ffm-pages-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (pdf, cache) = (dir.join("bill.pdf"), dir.join("pages"));
        std::fs::write(&pdf, one_page(200)).unwrap();

        let first = cached(cache.clone(), pdf.clone(), 1, 150)
            .await
            .expect("drawn");
        assert_eq!(&first[..2], b"\xFF\xD8", "a JPEG");
        let kept: Vec<_> = std::fs::read_dir(&cache).unwrap().flatten().collect();
        assert_eq!(kept.len(), 1, "kept once, under its final name");
        // Read back, not drawn: the kept bytes are what comes out.
        std::fs::write(kept[0].path(), b"kept").unwrap();
        assert_eq!(
            cached(cache.clone(), pdf.clone(), 1, 150).await.unwrap(),
            b"kept"
        );

        // A different file in the same slot is drawn, never the old drawing.
        // Another size too, so a filesystem keeping whole seconds tells them
        // apart.
        std::fs::write(&pdf, one_page(1000)).unwrap();
        let again = cached(cache.clone(), pdf.clone(), 1, 150).await.unwrap();
        assert_eq!(&again[..2], b"\xFF\xD8");
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 2);
        // A page the file does not have is no page, and nothing is kept.
        assert!(cached(cache.clone(), pdf.clone(), 9, 150).await.is_none());
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A one-page PDF written by hand, `side` points square.
    fn one_page(side: u32) -> Vec<u8> {
        format!(
            "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
             2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
             3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 {side} {side}]>>endobj\n\
             trailer<</Root 1 0 R>>\n%%EOF\n"
        )
        .into_bytes()
    }

    #[test]
    fn nothing_to_read_is_no_pages() {
        assert!(parse_boxes("").is_empty());
        assert!(parse_boxes("Pages: 2\nTitle: x\n").is_empty());
    }
}
