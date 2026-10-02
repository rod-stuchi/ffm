//! `:prune` — byte-identical files in the vault, and removing the copies the
//! trash holds of them.
//!
//! Every delete, replace and supersede moves a file into `{slug}/.delete/`
//! and nothing ever leaves, so the trash fills with exact copies of files
//! that are still live, or of each other. This finds them and, at an admin's
//! word, removes the ones in the trash.
//!
//! **It is the one place this app removes a file from disk**, and it is
//! defensible only because what goes is an exact duplicate of a file that
//! STAYS: no bytes are lost. That is proven again on the server at the moment
//! of removal — a fresh scan, never the page's word for it — and a group must
//! keep at least one copy whatever was checked.
//!
//! - **Size first, then blake3.** Only sizes two files share are hashed, so a
//!   vault of unique PDFs costs one `stat` each.
//! - **Only the trash is prunable.** A live file is listed — the same PDF
//!   filed on two rows is worth seeing — but never offered.
//! - **A pruned copy is retired**: a `file_unlink` noted [`PRUNED`], which the
//!   trash, `u` and every restore leave alone exactly as they leave a
//!   [`receipts::MOVED`] copy alone. A copy the app has no record of is logged
//!   too, with `entity_id = 0` (no `receipts` row has id 0) and its path.

use crate::db::changes::{Actor, Change, FileAction, Value};
use crate::domain::period::Period;
use crate::services::receipts;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::io;
use std::path::{Component, Path};

/// The note on the `file_unlink` that records a pruned copy.
pub const PRUNED: &str = "removed for good — a duplicate kept elsewhere";

/// One file in a group.
#[derive(Clone, Debug)]
pub struct Copy {
    /// Relative to the vault, `/`-separated.
    pub rel: String,
    pub place: Place,
}

impl Copy {
    pub fn in_trash(&self) -> bool {
        matches!(self.place, Place::Trash { .. })
    }
}

/// What the app knows about a file — read off `receipts`, never guessed from
/// the path.
#[derive(Clone, Debug)]
pub struct Record {
    pub receipt_id: i64,
    pub title: String,
    pub period: Option<Period>,
    /// `barcode` | `scheduled` | `receipt`.
    pub kind: String,
    pub income: bool,
    pub rule_id: Option<i64>,
    /// When it went to the trash; `None` for a live file.
    pub deleted_at: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Place {
    Live(Option<Record>),
    Trash(Option<Record>),
}

/// Files with the same bytes.
#[derive(Clone, Debug)]
pub struct Group {
    pub hash: String,
    pub size: u64,
    pub copies: Vec<Copy>,
}

impl Group {
    pub fn prunable(&self) -> bool {
        self.copies.iter().any(Copy::in_trash)
    }
}

/// Every group of two or more identical files under `root`: groups with a
/// copy in the trash first, then by the space their extra copies take.
pub async fn scan(pool: &SqlitePool, root: &Path) -> io::Result<Vec<Group>> {
    let (live, trashed) = records(pool, root).await.map_err(io::Error::other)?;
    let dir = root.to_path_buf();
    let found = tokio::task::spawn_blocking(move || identical(&dir))
        .await
        .map_err(io::Error::other)??;
    let mut groups: Vec<Group> = found
        .into_iter()
        .map(|(hash, size, rels)| {
            let mut copies: Vec<Copy> = rels
                .into_iter()
                .map(|rel| {
                    let place = if in_trash(&rel) {
                        Place::Trash(trashed.get(&rel).cloned())
                    } else {
                        Place::Live(live.get(&rel).cloned())
                    };
                    Copy { rel, place }
                })
                .collect();
            // What stays reads first; then the copies that could go.
            copies.sort_by(|a, b| a.in_trash().cmp(&b.in_trash()).then(a.rel.cmp(&b.rel)));
            Group { hash, size, copies }
        })
        .collect();
    groups.sort_by(|a, b| {
        let waste = |g: &Group| g.size * (g.copies.len() as u64 - 1);
        b.prunable()
            .cmp(&a.prunable())
            .then(waste(b).cmp(&waste(a)))
            .then(a.hash.cmp(&b.hash))
    });
    Ok(groups)
}

/// The disk half: walk, group by size, hash what shares a size.
fn identical(root: &Path) -> io::Result<Vec<(String, u64, Vec<String>)>> {
    let mut by_size: HashMap<u64, Vec<String>> = HashMap::new();
    let walk = walkdir::WalkDir::new(root).into_iter().filter_entry(|e| {
        // `.delete` is the one hidden folder that holds files worth looking
        // at; `.DS_Store`, the write probe and the like are the system's.
        let name = e.file_name().to_string_lossy();
        e.depth() == 0 || !name.starts_with('.') || name == ".delete"
    });
    for entry in walk {
        let entry = entry.map_err(io::Error::other)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(rel) = relative(root, entry.path()) else {
            continue;
        };
        let size = entry.metadata().map_err(io::Error::other)?.len();
        by_size.entry(size).or_default().push(rel);
    }

    let mut by_hash: HashMap<(String, u64), Vec<String>> = HashMap::new();
    for (size, rels) in by_size {
        if rels.len() < 2 {
            continue;
        }
        for rel in rels {
            let hash = hash_file(&root.join(&rel))?;
            by_hash.entry((hash, size)).or_default().push(rel);
        }
    }
    Ok(by_hash
        .into_iter()
        .filter(|(_, rels)| rels.len() > 1)
        .map(|((hash, size), rels)| (hash, size, rels))
        .collect())
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(std::fs::File::open(path)?)?;
    Ok(hasher.finalize().to_hex().to_string())
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<&str> = rel
        .components()
        .map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Option<_>>()?;
    Some(parts.join("/"))
}

/// `{slug}/.delete/…` — the member's trash, and nowhere else.
pub fn in_trash(rel: &str) -> bool {
    let mut parts = rel.split('/');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(slug), Some(".delete"), Some(_)) if !slug.is_empty() && !slug.starts_with('.')
    )
}

/// What the app knows about each file, by its path relative to the vault:
/// the live ones by `receipts.path`, the trashed ones by where
/// [`receipts::deleted_copy`] finds them — every naming a delete has ever
/// used, `09_barcode.r42.pdf` included.
async fn records(
    pool: &SqlitePool,
    root: &Path,
) -> sqlx::Result<(HashMap<String, Record>, HashMap<String, Record>)> {
    let rows = sqlx::query!(
        r#"SELECT r.id AS "id!: i64", r.kind AS "kind!: String",
                  COALESCE(r.deleted_from_path, r.path) AS "path!: String",
                  r.deleted_at AS "deleted_at: String",
                  t.title AS "title!: String", t.period AS "period!: String",
                  t.kind AS "tx_kind!: String", t.rule_id AS "rule_id: i64"
           FROM receipts r JOIN transactions t ON t.id = r.transaction_id"#
    )
    .fetch_all(pool)
    .await?;
    let mut live = HashMap::new();
    let mut trashed = HashMap::new();
    for r in rows {
        let record = Record {
            receipt_id: r.id,
            title: r.title,
            period: Period::parse(&r.period),
            kind: r.kind,
            income: r.tx_kind == "in",
            rule_id: r.rule_id,
            deleted_at: r.deleted_at.clone(),
        };
        if r.deleted_at.is_none() {
            live.insert(r.path, record);
        } else if let Some(rel) =
            receipts::deleted_copy(root, &r.path, r.id).and_then(|full| relative(root, &full))
        {
            trashed.insert(rel, record);
        }
    }
    Ok((live, trashed))
}

/// Why a prune did not happen. Nothing is removed when any pick is refused:
/// a batch that half-ran would leave the admin guessing which half.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not a path in a member's trash — a live file, or one outside the vault.
    NotInTrash(String),
    /// Gone already, or no longer a duplicate of anything.
    NotADuplicate(String),
    /// Every copy of a group was checked; one has to stay.
    LastCopy(String),
}

#[derive(Debug)]
pub enum PruneError {
    Refused(Refusal),
    Io(String),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    pub files: usize,
    pub bytes: u64,
}

/// Removes `picks` — paths relative to the vault — and logs each removal,
/// as `admin`. Checked against a fresh scan, all before anything is removed.
pub async fn prune(
    pool: &SqlitePool,
    root: &Path,
    admin: i64,
    picks: &[String],
) -> Result<Pruned, PruneError> {
    for p in picks {
        if !in_trash(p) || p.split('/').any(|s| s == ".." || s == "." || s.is_empty()) {
            return Err(PruneError::Refused(Refusal::NotInTrash(p.clone())));
        }
    }
    let groups = scan(pool, root)
        .await
        .map_err(|e| PruneError::Io(e.to_string()))?;

    // Each pick, with the group it is in and a copy of it that stays.
    let mut plan: Vec<(&Copy, u64, String)> = Vec::new();
    for p in picks {
        let Some(group) = groups.iter().find(|g| g.copies.iter().any(|c| &c.rel == p)) else {
            return Err(PruneError::Refused(Refusal::NotADuplicate(p.clone())));
        };
        let Some(kept) = group.copies.iter().find(|c| !picks.contains(&c.rel)) else {
            return Err(PruneError::Refused(Refusal::LastCopy(p.clone())));
        };
        let copy = group
            .copies
            .iter()
            .find(|c| &c.rel == p)
            .expect("found above");
        if !plan.iter().any(|(c, _, _)| c.rel == copy.rel) {
            plan.push((copy, group.size, kept.rel.clone()));
        }
    }

    // The disk first, the log for what actually went: a removal that failed
    // halfway is still a record of the files that are gone.
    let mut done: Vec<(&Copy, u64, String)> = Vec::new();
    let mut failure = None;
    for (copy, size, kept) in plan {
        match std::fs::remove_file(root.join(&copy.rel)) {
            Ok(()) => {
                receipts::prune_empty(root, &copy.rel);
                done.push((copy, size, kept));
            }
            Err(e) => {
                failure = Some(e.to_string());
                break;
            }
        }
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| PruneError::Io(e.to_string()))?;
    for (copy, _, kept) in &done {
        let record = match &copy.place {
            Place::Trash(r) | Place::Live(r) => r.as_ref(),
        };
        let mut entry = Change::file(
            Actor::User(admin),
            FileAction::Unlink,
            record.map(|r| r.receipt_id).unwrap_or(0),
        )
        .note(PRUNED)
        .diff("path", Value::path(&copy.rel), Value::Path(None))
        .diff("kept", Value::Path(None), Value::path(kept));
        if let Some(r) = record {
            entry = entry.from_rule(r.rule_id);
            if let Some(p) = r.period {
                entry = entry.in_period(p);
            }
        }
        entry
            .write(&mut tx)
            .await
            .map_err(|e| PruneError::Io(e.to_string()))?;
    }
    tx.commit()
        .await
        .map_err(|e| PruneError::Io(e.to_string()))?;

    if let Some(e) = failure {
        return Err(PruneError::Io(e));
    }
    Ok(Pruned {
        files: done.len(),
        bytes: done.iter().map(|(_, size, _)| size).sum(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_members_trash_is_the_trash() {
        assert!(in_trash("udo/.delete/consumo/cpfl/2026/09_barcode.r10.pdf"));
        assert!(!in_trash("udo/consumo/cpfl/2026/09_barcode.pdf"));
        assert!(!in_trash(".delete/x.pdf"));
        assert!(!in_trash("udo/.delete"));
    }
}
