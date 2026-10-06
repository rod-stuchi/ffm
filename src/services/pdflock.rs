//! Opening a password-protected PDF before it reaches the vault.
//!
//! Some banks send statements (and some employers payslips) that open only
//! with a password. A rule may carry that password (migration 0012), and every
//! upload to its rows passes through [`unlock`] first: a locked PDF is opened
//! with it and the **unlocked copy** is what gets stored, so the browser tab,
//! the `P` carousel, `pdftotext` and a person browsing the folder by hand never
//! meet the password again. A locked file the password does not open is
//! refused before anything is written.
//!
//! `qpdf` does the opening — a subprocess, for the reason `pdftotext` is one:
//! it already handles every encryption a bank produces, and a PDF library
//! in-process is a large dependency for a job a small binary does well. It is
//! only ever run on a file that says it is encrypted, so a server without it
//! still takes every ordinary PDF.

use std::borrow::Cow;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Why a file was not opened. Every one of them means nothing is stored.
#[derive(Debug, PartialEq, Eq)]
pub enum LockError {
    /// Locked, and the rule has no password to try.
    NeedsPassword,
    /// Locked, and the rule's password does not open it.
    WrongPassword,
    /// Locked, and there is no `qpdf` on this server to open it with.
    NoTool,
    /// `qpdf` ran and failed for some other reason — a damaged file, usually.
    /// For the log, not the statusline.
    Tool(String),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::NeedsPassword => f.write_str("the PDF is password-protected"),
            LockError::WrongPassword => f.write_str("the password does not open the PDF"),
            LockError::NoTool => f.write_str("qpdf is not installed"),
            LockError::Tool(e) => write!(f, "qpdf: {e}"),
        }
    }
}

/// The bytes to store: the upload untouched when it is not a locked PDF, or an
/// unlocked copy of it.
///
/// A PDF that is encrypted with only an owner password — it opens for anybody
/// and merely forbids printing or copying — is unlocked too, with no password
/// asked for: the vault keeps plain files.
pub fn unlock<'a>(
    filename: &str,
    bytes: &'a [u8],
    password: Option<&str>,
) -> Result<Cow<'a, [u8]>, LockError> {
    if !is_pdf(filename, bytes) || !mentions_encrypt(bytes) {
        return Ok(Cow::Borrowed(bytes));
    }

    let scratch = Scratch::new().map_err(|e| LockError::Tool(e.to_string()))?;
    std::fs::write(&scratch.input, bytes).map_err(|e| LockError::Tool(e.to_string()))?;

    // 0: a password is needed · 2: not encrypted · 3: encrypted, but it opens
    // without one. The token scan above is a heuristic; this is the answer.
    let probe = qpdf()
        .arg("--requires-password")
        .arg(&scratch.input)
        .output()
        .map_err(spawn_error)?;
    match probe.status.code() {
        Some(2) => return Ok(Cow::Borrowed(bytes)),
        Some(3) => decrypt(&scratch, None),
        Some(0) => match password {
            None => Err(LockError::NeedsPassword),
            Some(p) => decrypt(&scratch, Some(p)),
        },
        _ => Err(LockError::Tool(stderr_of(&probe))),
    }
    .map(Cow::Owned)
}

/// `qpdf --decrypt`, with the password on **stdin** — never on the command
/// line, where every user on the machine could read it in `ps`.
fn decrypt(scratch: &Scratch, password: Option<&str>) -> Result<Vec<u8>, LockError> {
    let mut cmd = qpdf();
    if password.is_some() {
        cmd.arg("--password-file=-");
    }
    cmd.arg("--decrypt")
        .arg(&scratch.input)
        .arg(&scratch.output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(spawn_error)?;
    if let Some(mut stdin) = child.stdin.take() {
        // A single line, no newline needed; qpdf reads up to the first one.
        let _ = stdin.write_all(password.unwrap_or_default().as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| LockError::Tool(e.to_string()))?;
    // 3 is "succeeded, with warnings" — a slightly malformed but readable file,
    // which is most PDFs a bank ever produced.
    match out.status.code() {
        Some(0) | Some(3) => {
            std::fs::read(&scratch.output).map_err(|e| LockError::Tool(e.to_string()))
        }
        _ => {
            let err = stderr_of(&out);
            if err.contains("invalid password") {
                Err(LockError::WrongPassword)
            } else {
                Err(LockError::Tool(err))
            }
        }
    }
}

fn qpdf() -> Command {
    Command::new("qpdf")
}

fn spawn_error(e: std::io::Error) -> LockError {
    if e.kind() == std::io::ErrorKind::NotFound {
        LockError::NoTool
    } else {
        LockError::Tool(format!("qpdf could not run: {e}"))
    }
}

fn stderr_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

/// Whether [`unlock`] will have work to do — a PDF that names an encryption
/// dictionary. Cheap: no `qpdf`, so a page can say "Opening the PDF" only
/// when it is true.
pub fn looks_locked(filename: &str, bytes: &[u8]) -> bool {
    is_pdf(filename, bytes) && mentions_encrypt(bytes)
}

/// By its name, or by its header — which the spec allows anywhere in the
/// first kilobyte, and which some scanners put a few bytes in.
fn is_pdf(filename: &str, bytes: &[u8]) -> bool {
    Path::new(filename)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        || bytes[..bytes.len().min(1024)]
            .windows(5)
            .any(|w| w == b"%PDF-")
}

/// An encrypted PDF names its encryption dictionary in the trailer (or the
/// cross-reference stream's dictionary), and neither is ever compressed — so
/// a file without the token is certainly not encrypted, and `qpdf` is not
/// needed to say so.
fn mentions_encrypt(bytes: &[u8]) -> bool {
    bytes.windows(8).any(|w| w == b"/Encrypt")
}

/// Two files in the temp dir, removed when this goes out of scope — on every
/// path out, including the early ones.
struct Scratch {
    input: PathBuf,
    output: PathBuf,
}

impl Scratch {
    fn new() -> std::io::Result<Scratch> {
        let mut raw = [0u8; 8];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut raw);
        let tag: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir();
        Ok(Scratch {
            input: dir.join(format!("ffm-lock-{tag}.pdf")),
            output: dir.join(format!("ffm-open-{tag}.pdf")),
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.input);
        let _ = std::fs::remove_file(&self.output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// qpdf is a system tool, not a crate: where it is missing these say so
    /// and pass, rather than failing a build that has nothing wrong with it.
    fn have_qpdf() -> bool {
        let ok = Command::new("qpdf").arg("--version").output().is_ok();
        if !ok {
            eprintln!("qpdf not on PATH — skipping");
        }
        ok
    }

    /// A one-page PDF from qpdf itself, encrypted as asked.
    fn fixture(encrypt: &[&str]) -> Vec<u8> {
        let dir = std::env::temp_dir();
        let mut raw = [0u8; 8];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut raw);
        let tag: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let plain = dir.join(format!("ffm-fixture-{tag}.pdf"));
        let locked = dir.join(format!("ffm-fixture-{tag}-locked.pdf"));
        assert!(
            Command::new("qpdf")
                .arg("--empty")
                .arg(&plain)
                .status()
                .unwrap()
                .success()
        );
        let out = if encrypt.is_empty() {
            plain.clone()
        } else {
            let mut cmd = Command::new("qpdf");
            cmd.arg("--encrypt")
                .args(encrypt)
                .arg("--")
                .arg(&plain)
                .arg(&locked);
            assert!(cmd.status().unwrap().success());
            locked.clone()
        };
        let bytes = std::fs::read(&out).unwrap();
        let _ = std::fs::remove_file(&plain);
        let _ = std::fs::remove_file(&locked);
        bytes
    }

    fn locked() -> Vec<u8> {
        fixture(&["--user-password=1234", "--owner-password=x", "--bits=256"])
    }

    fn still_locked(bytes: &[u8]) -> bool {
        mentions_encrypt(bytes)
    }

    #[test]
    fn a_file_that_is_not_a_pdf_is_left_alone() {
        let bytes = b"conta de luz /Encrypt".to_vec();
        assert!(matches!(
            unlock("bill.txt", &bytes, None),
            Ok(Cow::Borrowed(_))
        ));
    }

    #[test]
    fn a_plain_pdf_is_stored_as_it_came() {
        if !have_qpdf() {
            return;
        }
        let bytes = fixture(&[]);
        assert!(matches!(
            unlock("bill.pdf", &bytes, None),
            Ok(Cow::Borrowed(_))
        ));
    }

    #[test]
    fn a_locked_pdf_with_no_password_is_refused() {
        if !have_qpdf() {
            return;
        }
        assert_eq!(
            unlock("bill.pdf", &locked(), None),
            Err(LockError::NeedsPassword)
        );
    }

    #[test]
    fn the_wrong_password_is_refused() {
        if !have_qpdf() {
            return;
        }
        assert_eq!(
            unlock("bill.pdf", &locked(), Some("4321")),
            Err(LockError::WrongPassword)
        );
    }

    #[test]
    fn the_right_password_stores_an_unlocked_copy() {
        if !have_qpdf() {
            return;
        }
        let bytes = locked();
        let open = unlock("fatura.pdf", &bytes, Some("1234")).unwrap();
        assert!(matches!(open, Cow::Owned(_)));
        assert!(open.starts_with(b"%PDF-"));
        assert!(!still_locked(&open));
        // And it opens with no password at all now.
        assert!(matches!(
            unlock("fatura.pdf", &open, None),
            Ok(Cow::Borrowed(_))
        ));
    }

    #[test]
    fn an_owner_password_alone_is_lifted_without_asking() {
        if !have_qpdf() {
            return;
        }
        let bytes = fixture(&["--user-password=", "--owner-password=x", "--bits=256"]);
        let open = unlock("holerite.pdf", &bytes, None).unwrap();
        assert!(!still_locked(&open));
    }

    #[test]
    fn the_header_names_a_pdf_whatever_the_file_is_called() {
        assert!(is_pdf("scan", b"%PDF-1.7\n"));
        assert!(is_pdf("x.PDF", b""));
        assert!(!is_pdf("x.png", b"\x89PNG"));
    }
}
