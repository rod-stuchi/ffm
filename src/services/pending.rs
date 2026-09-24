//! A bill dropped into `A` before its rule exists.
//!
//! `E`'s dropzone attaches straight to a rule, because the rule is already
//! there. `A`'s cannot: the rule is created by the same Create that should
//! attach the bill, and the form is an ordinary urlencoded POST — which is
//! what keeps a rejected save able to come back with every field intact.
//!
//! So the drop is two steps. The file is parsed and **parked** here, and the
//! pane gets back what was read plus a token; Create carries the token like
//! any other field and attaches the parked file to the rule it just made.
//! Nothing touches the vault or the database until then — a pane closed
//! without creating leaves only a scratch file, swept after a day.
//!
//! It is scratch space, not state: a restart that loses it costs somebody one
//! re-drop, and nothing in the ledger ever points at it.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use rand::RngCore;

/// How long a parked bill waits for its Create.
const TTL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct Parked {
    pub member_id: i64,
    pub filename: String,
    pub bytes: Vec<u8>,
}

fn root() -> PathBuf {
    std::env::temp_dir().join("ffm-pending")
}

/// A token is 32 lowercase hex characters, and nothing else ever reaches the
/// filesystem: it arrives in a form field, so it is checked before it is
/// joined onto a path.
fn dir_of(token: &str) -> Option<PathBuf> {
    let ok = token.len() == 32
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    ok.then(|| root().join(token))
}

/// Parks a bill and returns its token. Sweeps anything older than a day on
/// the way, so there is no separate job to forget to run.
pub fn park(member_id: i64, filename: &str, bytes: &[u8]) -> std::io::Result<String> {
    sweep();
    let mut raw = [0u8; 16];
    rand::rng().fill_bytes(&mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let dir = root().join(&token);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("file"), bytes)?;
    std::fs::write(dir.join("meta"), format!("{member_id}\n{filename}"))?;
    Ok(token)
}

/// Takes a parked bill back out — once. Only the member who parked it can
/// claim it; anybody else gets `None`, the same answer as an unknown token.
pub fn claim(token: &str, member_id: i64) -> Option<Parked> {
    let dir = dir_of(token)?;
    let meta = std::fs::read_to_string(dir.join("meta")).ok()?;
    let (owner, filename) = meta.split_once('\n')?;
    if owner.parse::<i64>().ok()? != member_id {
        return None;
    }
    let bytes = std::fs::read(dir.join("file")).ok()?;
    let _ = std::fs::remove_dir_all(&dir);
    Some(Parked {
        member_id,
        filename: filename.to_string(),
        bytes,
    })
}

/// Whether a token still names a parked bill of this member's — for a
/// rejected save, which renders the preview again without consuming it.
pub fn filename_of(token: &str, member_id: i64) -> Option<String> {
    let dir = dir_of(token)?;
    let meta = std::fs::read_to_string(dir.join("meta")).ok()?;
    let (owner, filename) = meta.split_once('\n')?;
    (owner.parse::<i64>().ok()? == member_id).then(|| filename.to_string())
}

/// The parked bytes, without claiming them.
pub fn peek(token: &str, member_id: i64) -> Option<Vec<u8>> {
    filename_of(token, member_id)?;
    std::fs::read(dir_of(token)?.join("file")).ok()
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

    #[test]
    fn a_parked_bill_is_claimed_once_and_only_by_its_owner() {
        let token = park(7, "conta.pdf", b"%PDF").unwrap();
        assert!(claim(&token, 8).is_none());
        assert_eq!(filename_of(&token, 7).as_deref(), Some("conta.pdf"));
        let p = claim(&token, 7).unwrap();
        assert_eq!(p.bytes, b"%PDF");
        assert!(claim(&token, 7).is_none());
    }

    #[test]
    fn a_token_never_reaches_the_filesystem_unless_it_is_hex() {
        assert!(dir_of("../../etc/passwd").is_none());
        assert!(dir_of(&"g".repeat(32)).is_none());
        assert!(dir_of(&"a".repeat(32)).is_some());
    }
}
