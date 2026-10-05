//! Making a PDF smaller before it is filed: dropping the pages nobody needs,
//! and shrinking what is left.
//!
//! Two programs, both subprocesses, like `pdftotext` and `qpdf` elsewhere:
//!
//! - **`qpdf`** keeps the chosen pages — `qpdf in --pages in 1,3 -- out`. It is
//!   already required (`services::pdflock`).
//! - **Ghostscript** shrinks — `gs -sDEVICE=pdfwrite -dPDFSETTINGS=/ebook …`,
//!   the command the household was running by hand. It is **optional**: with no
//!   `gs` on the server the shrink is simply unavailable, and trimming works.
//!
//! A shrink that does not come out smaller is thrown away by the caller
//! (`services::stage`): a PDF that is already lean can grow under `/ebook`,
//! and filing the bigger copy would be the opposite of the point.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Long enough for a scanned 20-page statement; short enough that a `gs`
/// stuck on a malformed file does not hold a request forever.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Whether Ghostscript is installed — asked once per process.
pub fn gs_available() -> bool {
    static GS: OnceLock<bool> = OnceLock::new();
    *GS.get_or_init(|| {
        Command::new("gs")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// `1,3,4` — the pages to keep, as qpdf reads them. Sorted and unique, so one
/// selection always has one spelling (and one cached file, see `stage`).
pub fn page_spec(keep: &[u32]) -> String {
    let mut pages: Vec<u32> = keep.to_vec();
    pages.sort_unstable();
    pages.dedup();
    pages
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Keeps `keep` of `input`, in order, into `out`.
pub fn select(input: &Path, keep: &[u32], out: &Path) -> Result<(), String> {
    let mut cmd = Command::new("qpdf");
    cmd.arg(input)
        .arg("--pages")
        .arg(input)
        .arg(page_spec(keep))
        .arg("--")
        .arg(out);
    // 3 is "succeeded, with warnings" — most PDFs a bank ever produced.
    run(cmd, "qpdf", &[0, 3])
}

/// Shrinks `input` into `out` with Ghostscript's `/ebook` preset — images
/// resampled to 150 dpi, which keeps a boleto's digits and QR code readable.
pub fn shrink(input: &Path, out: &Path) -> Result<(), String> {
    let mut cmd = Command::new("gs");
    cmd.args([
        "-sDEVICE=pdfwrite",
        "-dCompatibilityLevel=1.4",
        "-dPDFSETTINGS=/ebook",
        "-dNOPAUSE",
        "-dQUIET",
        "-dBATCH",
        "-dSAFER",
    ])
    .arg(format!("-sOutputFile={}", out.display()))
    .arg(input);
    run(cmd, "gs", &[0])
}

/// Runs a command to completion or `TIMEOUT`, whichever comes first — killed
/// on the timeout, never left running behind a request that gave up on it.
fn run(mut cmd: Command, name: &str, ok: &[i32]) -> Result<(), String> {
    let mut child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{name} could not run: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.code().is_some_and(|c| ok.contains(&c)) {
                    Ok(())
                } else {
                    let mut err = String::new();
                    if let Some(mut e) = child.stderr.take() {
                        let _ = std::io::Read::read_to_string(&mut e, &mut err);
                    }
                    Err(format!("{name}: {}", err.trim()))
                };
            }
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{name} took longer than {}s", TIMEOUT.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(format!("{name}: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have(tool: &str) -> bool {
        let ok = Command::new(tool).arg("--version").output().is_ok();
        if !ok {
            eprintln!("{tool} not on PATH — skipping");
        }
        ok
    }

    /// A PDF of `n` empty pages, made by qpdf itself.
    fn pages(n: u32, dir: &Path) -> std::path::PathBuf {
        let one = dir.join("one.pdf");
        let empty = qpdf_empty(&one);
        assert!(empty, "qpdf --empty");
        let out = dir.join(format!("p{n}.pdf"));
        let mut cmd = Command::new("qpdf");
        cmd.arg("--empty").arg("--pages");
        for _ in 0..n {
            cmd.arg(&one);
        }
        cmd.arg("--").arg(&out);
        // 3: repaired with warnings — the hand-written page has no xref.
        let code = cmd.status().unwrap().code();
        assert!(matches!(code, Some(0) | Some(3)), "qpdf exited {code:?}");
        out
    }

    fn qpdf_empty(out: &Path) -> bool {
        // A one-page PDF written by hand: qpdf --empty has no pages at all.
        let pdf = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n\
trailer<</Root 1 0 R>>\n%%EOF\n";
        std::fs::write(out, pdf).is_ok()
    }

    fn count(path: &Path) -> u32 {
        let out = Command::new("qpdf")
            .arg("--show-npages")
            .arg(path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ffm-trim-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_page_spec_has_one_spelling() {
        assert_eq!(page_spec(&[3, 1, 3, 2]), "1,2,3");
    }

    #[test]
    fn keeping_pages_one_and_three_of_three_leaves_two() {
        if !have("qpdf") {
            return;
        }
        let dir = scratch();
        let three = pages(3, &dir);
        let out = dir.join("kept.pdf");
        select(&three, &[1, 3], &out).unwrap();
        assert_eq!(count(&out), 2);
    }

    #[test]
    fn a_shrink_writes_a_pdf() {
        if !have("qpdf") || !gs_available() {
            return;
        }
        let dir = scratch();
        let two = pages(2, &dir);
        let out = dir.join("small.pdf");
        shrink(&two, &out).unwrap();
        assert_eq!(count(&out), 2, "every page survives the shrink");
    }
}
