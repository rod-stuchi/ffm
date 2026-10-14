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
//!
//! **And a file kept longer than the household keeps them** —
//! `household_settings.trash_keep_days`, 90 by default — is offered too,
//! duplicate or not. Here bytes ARE lost, deliberately: the trash used to only
//! grow. So it is counted from when the file was removed
//! (`receipts.deleted_at`, never the bill's month or the file's mtime), a file
//! the app has no record of is never offered by age, nothing goes but at an
//! admin's `y`, and 0 keeps everything forever. It is retired like a
//! duplicate, noted [`EXPIRED`].

use crate::db::changes::{Actor, Change, FileAction, Value};
use crate::domain::period::Period;
use crate::services::receipts;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::io;
use std::path::{Component, Path};

/// The note on the `file_unlink` that records a pruned copy.
pub const PRUNED: &str = "removed for good — a duplicate kept elsewhere";

/// The note on the `file_unlink` that records a file removed because it was
/// in the trash longer than the household keeps them.
pub const EXPIRED: &str = "removed for good — kept in the trash past its time";

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

/// A file in the trash longer than the household keeps them, with no
/// duplicate — one with a duplicate is in its [`Group`], where it goes with
/// no bytes lost.
#[derive(Clone, Debug)]
pub struct Expired {
    /// Relative to the vault, `/`-separated.
    pub rel: String,
    pub size: u64,
    pub record: Record,
}

/// What `:prune` can offer: duplicate groups, and files past the household's
/// time in the trash.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub groups: Vec<Group>,
    /// Largest first.
    pub expired: Vec<Expired>,
    /// `household_settings.trash_keep_days` as it was read; 0 offers nothing
    /// by age.
    pub keep_days: i64,
}

impl Scan {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.expired.is_empty()
    }
}

/// Every group of two or more identical files under `root` — groups with a
/// copy in the trash first, then by the space their extra copies take — and
/// every trashed file kept longer than the household keeps them.
pub async fn scan(pool: &SqlitePool, root: &Path) -> io::Result<Scan> {
    let keep_days = crate::db::settings::trash_keep_days(pool).await;
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

    let expired = past_their_time(root, &trashed, &groups, keep_days);
    Ok(Scan {
        groups,
        expired,
        keep_days,
    })
}

/// The trashed files removed more than `keep_days` ago, by their record's
/// `deleted_at` — stored as `%Y-%m-%dT%H:%M:%SZ`, so a string compare is a
/// time compare. Measured with `clock::now`, so a dev server run as of
/// another date agrees. One already in a duplicate group is left there.
fn past_their_time(
    root: &Path,
    trashed: &HashMap<String, Record>,
    groups: &[Group],
    keep_days: i64,
) -> Vec<Expired> {
    if keep_days <= 0 {
        return Vec::new();
    }
    let cutoff = (crate::clock::now() - chrono::Duration::days(keep_days))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let grouped: std::collections::HashSet<&str> = groups
        .iter()
        .flat_map(|g| g.copies.iter().map(|c| c.rel.as_str()))
        .collect();
    let mut out: Vec<Expired> = trashed
        .iter()
        .filter(|(rel, r)| {
            !grouped.contains(rel.as_str())
                && r.deleted_at.as_deref().is_some_and(|d| d < cutoff.as_str())
        })
        .filter_map(|(rel, r)| {
            let size = std::fs::metadata(root.join(rel)).ok()?.len();
            Some(Expired {
                rel: rel.clone(),
                size,
                record: r.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.size.cmp(&a.size).then(a.rel.cmp(&b.rel)));
    out
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
    /// Gone already, no longer a duplicate of anything, and not in the trash
    /// longer than the household keeps files.
    NotPrunable(String),
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
    let found = scan(pool, root)
        .await
        .map_err(|e| PruneError::Io(e.to_string()))?;

    // Each pick, and why it may go: a duplicate, with a copy of it that
    // stays; or a file past the household's time in the trash, with none.
    let mut plan: Vec<Pick> = Vec::new();
    for p in picks {
        if plan.iter().any(|x| x.rel() == p) {
            continue;
        }
        if let Some(group) = found
            .groups
            .iter()
            .find(|g| g.copies.iter().any(|c| &c.rel == p))
        {
            let Some(kept) = group.copies.iter().find(|c| !picks.contains(&c.rel)) else {
                return Err(PruneError::Refused(Refusal::LastCopy(p.clone())));
            };
            let copy = group
                .copies
                .iter()
                .find(|c| &c.rel == p)
                .expect("found above");
            plan.push(Pick::Duplicate {
                copy,
                size: group.size,
                kept: kept.rel.clone(),
            });
        } else if let Some(file) = found.expired.iter().find(|e| &e.rel == p) {
            plan.push(Pick::Expired(file));
        } else {
            return Err(PruneError::Refused(Refusal::NotPrunable(p.clone())));
        }
    }

    // The disk first, the log for what actually went: a removal that failed
    // halfway is still a record of the files that are gone.
    let mut done: Vec<Pick> = Vec::new();
    let mut failure = None;
    for pick in plan {
        match std::fs::remove_file(root.join(pick.rel())) {
            Ok(()) => {
                receipts::prune_empty(root, pick.rel());
                done.push(pick);
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
    for pick in &done {
        let record = pick.record();
        let entry = Change::file(
            Actor::User(admin),
            FileAction::Unlink,
            record.map(|r| r.receipt_id).unwrap_or(0),
        );
        let mut entry = match pick {
            Pick::Duplicate { copy, kept, .. } => entry
                .note(PRUNED)
                .diff("path", Value::path(&copy.rel), Value::Path(None))
                .diff("kept", Value::Path(None), Value::path(kept)),
            Pick::Expired(file) => {
                entry
                    .note(EXPIRED)
                    .diff("path", Value::path(&file.rel), Value::Path(None))
            }
        };
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
        bytes: done.iter().map(Pick::size).sum(),
    })
}

/// One file `prune` will remove, and why it may.
enum Pick<'a> {
    /// The same bytes as `kept`, which stays.
    Duplicate {
        copy: &'a Copy,
        size: u64,
        kept: String,
    },
    /// In the trash longer than the household keeps files.
    Expired(&'a Expired),
}

impl Pick<'_> {
    fn rel(&self) -> &str {
        match self {
            Pick::Duplicate { copy, .. } => &copy.rel,
            Pick::Expired(file) => &file.rel,
        }
    }

    fn size(&self) -> u64 {
        match self {
            Pick::Duplicate { size, .. } => *size,
            Pick::Expired(file) => file.size,
        }
    }

    fn record(&self) -> Option<&Record> {
        match self {
            Pick::Duplicate { copy, .. } => match &copy.place {
                Place::Trash(r) | Place::Live(r) => r.as_ref(),
            },
            Pick::Expired(file) => Some(&file.record),
        }
    }
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
