//! A PDF waiting in the trim-and-shrink view, before it is filed.
//!
//! Every PDF on its way to a row or a rule stops here first, and so does a
//! stored one opened from `P` to be reworked: the view shows its pages, the
//! person picks which to keep and whether to shrink, and the size line says
//! what that comes to. Only Save sends anything on — and it sends the
//! finished bytes through the **same upload** the file would have taken
//! anyway, so every check a file gets (the bill's binding, the duplicate
//! check, `superseded`, the log) still runs, on what is actually filed.
//!
//! It is scratch space, like `services::pending`: a token directory in the
//! temp dir holding the original and the results computed so far, swept after
//! a day. Nothing in the ledger ever points at it, and a restart that loses it
//! costs somebody one re-drop.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rand::RngCore;

use crate::services::pdftrim;

const TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// What a staged PDF is, besides its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    pub member_id: i64,
    pub filename: String,
    pub pages: u32,
    /// A bill that covers several months, one page each (`spans_period`):
    /// its pages cannot be dropped, or every month after a dropped one would
    /// be read off the wrong page.
    pub spans: bool,
}

/// What shrinking did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shrink {
    /// Not asked for.
    Off,
    /// Made it smaller, and is in the result.
    Applied,
    /// Asked for, but the shrunk copy was not smaller — thrown away.
    NoGain,
    /// Asked for, and there is no Ghostscript on this server.
    Unavailable,
}

/// The file a selection comes to.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub path: PathBuf,
    pub original: u64,
    pub size: u64,
    pub pages: u32,
    pub shrink: Shrink,
}

fn root() -> PathBuf {
    std::env::temp_dir().join("ffm-stage")
}

/// Only 32 lowercase hex characters ever reach the filesystem: a token
/// arrives in a URL, so it is checked before it is joined onto a path.
fn dir_of(token: &str) -> Option<PathBuf> {
    let ok = token.len() == 32
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    ok.then(|| root().join(token))
}

/// Stages a PDF and returns its token. Sweeps anything older than a day on
/// the way, so there is no separate job to forget to run.
pub fn park(meta: &Meta, bytes: &[u8]) -> std::io::Result<String> {
    sweep();
    let mut raw = [0u8; 16];
    rand::rng().fill_bytes(&mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let dir = root().join(&token);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("original.pdf"), bytes)?;
    std::fs::write(
        dir.join("meta"),
        format!(
            "{}\n{}\n{}\n{}",
            meta.member_id,
            meta.pages,
            u8::from(meta.spans),
            meta.filename
        ),
    )?;
    Ok(token)
}

/// Records the page count, which is only known once the file is on disk for
/// `pdfinfo` to read.
pub fn set_pages(token: &str, member_id: i64, pages: u32) -> std::io::Result<()> {
    let Some((dir, meta)) = open(token, member_id) else {
        return Err(std::io::Error::other("no such staged file"));
    };
    std::fs::write(
        dir.join("meta"),
        format!(
            "{}\n{}\n{}\n{}",
            meta.member_id,
            pages,
            u8::from(meta.spans),
            meta.filename
        ),
    )
}

/// The staged PDF's directory and meta — only for the member who staged it.
fn open(token: &str, member_id: i64) -> Option<(PathBuf, Meta)> {
    let dir = dir_of(token)?;
    let raw = std::fs::read_to_string(dir.join("meta")).ok()?;
    let mut lines = raw.splitn(4, '\n');
    let meta = Meta {
        member_id: lines.next()?.parse().ok()?,
        pages: lines.next()?.parse().ok()?,
        spans: lines.next()? == "1",
        filename: lines.next()?.to_string(),
    };
    (meta.member_id == member_id).then_some((dir, meta))
}

pub fn meta(token: &str, member_id: i64) -> Option<Meta> {
    open(token, member_id).map(|(_, m)| m)
}

/// The original, for drawing its pages.
pub fn original(token: &str, member_id: i64) -> Option<PathBuf> {
    open(token, member_id).map(|(dir, _)| dir.join("original.pdf"))
}

/// The file `keep` and `shrink` come to — computed once and kept, so
/// flipping a choice back is instant and Save reuses what the size line
/// already produced. **Blocking**: run it off the async runtime.
///
/// `keep` is the pages to keep, `None` for all of them. A bill spanning
/// several months always keeps all of them, whatever is asked.
pub fn result(
    token: &str,
    member_id: i64,
    keep: Option<&[u32]>,
    shrink: bool,
) -> Result<Outcome, String> {
    result_with(token, member_id, keep, shrink, &|_| {})
}

/// A step [`result_with`] is about to do for real — not one it finds already
/// done from an earlier size check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Trim,
    Shrink,
}

/// [`result`], saying each step as it starts — for the progress box above
/// Save (`web::fidget`).
pub fn result_with(
    token: &str,
    member_id: i64,
    keep: Option<&[u32]>,
    shrink: bool,
    on_step: &dyn Fn(Step),
) -> Result<Outcome, String> {
    let (dir, meta) = open(token, member_id).ok_or("that file is no longer here")?;
    let original = dir.join("original.pdf");
    let original_size = size(&original)?;

    let keep: Vec<u32> = match keep {
        Some(k) if !meta.spans => k
            .iter()
            .copied()
            .filter(|p| (1..=meta.pages).contains(p))
            .collect(),
        _ => (1..=meta.pages).collect(),
    };
    if keep.is_empty() {
        return Err("keep at least one page".into());
    }
    let spec = pdftrim::page_spec(&keep);
    let pages = keep.len() as u32;

    // The pages first. All of them kept is the original itself, untouched.
    let trimmed = if pages == meta.pages {
        original.clone()
    } else {
        let out = dir.join(format!("k{}.pdf", spec.replace(',', "-")));
        if !out.exists() {
            on_step(Step::Trim);
            pdftrim::select(&original, &keep, &out)?;
        }
        out
    };
    let trimmed_size = size(&trimmed)?;

    if !shrink {
        return Ok(Outcome {
            path: trimmed,
            original: original_size,
            size: trimmed_size,
            pages,
            shrink: Shrink::Off,
        });
    }
    if !pdftrim::gs_available() {
        return Ok(Outcome {
            path: trimmed,
            original: original_size,
            size: trimmed_size,
            pages,
            shrink: Shrink::Unavailable,
        });
    }

    let shrunk = dir.join(format!("k{}-s.pdf", spec.replace(',', "-")));
    if !shrunk.exists() {
        on_step(Step::Shrink);
        pdftrim::shrink(&trimmed, &shrunk)?;
    }
    let shrunk_size = size(&shrunk)?;
    // A shrink that does not help is thrown away: an already-lean PDF can
    // grow under /ebook, and the point was to file less, not more.
    Ok(if shrunk_size < trimmed_size {
        Outcome {
            path: shrunk,
            original: original_size,
            size: shrunk_size,
            pages,
            shrink: Shrink::Applied,
        }
    } else {
        Outcome {
            path: trimmed,
            original: original_size,
            size: trimmed_size,
            pages,
            shrink: Shrink::NoGain,
        }
    })
}

/// Drops a staged PDF and everything computed from it.
pub fn discard(token: &str, member_id: i64) {
    if let Some((dir, _)) = open(token, member_id) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn size(path: &Path) -> Result<u64, String> {
    std::fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| e.to_string())
}

fn sweep() {
    let Ok(entries) = std::fs::read_dir(root()) else {
        return;
    };
    let now = SystemTime::now();
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > TTL);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(member_id: i64) -> Meta {
        Meta {
            member_id,
            filename: "conta.pdf".into(),
            pages: 1,
            spans: false,
        }
    }

    #[test]
    fn a_token_is_only_ever_hex() {
        assert!(dir_of("../../etc/passwd").is_none());
        assert!(dir_of("ABCDEF0123456789abcdef0123456789").is_none());
        assert!(dir_of("abcdef0123456789abcdef0123456789").is_some());
    }

    #[test]
    fn a_staged_file_is_only_its_own_members() {
        let token = park(&meta(7), b"%PDF-1.4").unwrap();
        assert_eq!(super::meta(&token, 7).unwrap().filename, "conta.pdf");
        assert!(super::meta(&token, 8).is_none(), "nobody else's to read");
        discard(&token, 7);
        assert!(super::meta(&token, 7).is_none());
    }

    /// A hand-written page is a few hundred bytes; Ghostscript's rewrite of
    /// it carries its own fonts and metadata and comes out bigger. That copy
    /// must be thrown away, never filed.
    #[test]
    fn a_shrink_that_grows_is_thrown_away() {
        if !pdftrim::gs_available() {
            eprintln!("gs not on PATH — skipping");
            return;
        }
        let pdf = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n\
trailer<</Root 1 0 R>>\n%%EOF\n";
        let token = park(&meta(7), pdf).unwrap();
        let out = result(&token, 7, None, true).unwrap();
        assert_eq!(out.shrink, Shrink::NoGain);
        assert_eq!(out.size, pdf.len() as u64, "the original, untouched");
        assert!(out.path.ends_with("original.pdf"));
        discard(&token, 7);
    }

    #[test]
    fn keeping_no_page_is_refused() {
        let token = park(&meta(7), b"%PDF-1.4").unwrap();
        assert!(result(&token, 7, Some(&[]), false).is_err());
        discard(&token, 7);
    }
}
