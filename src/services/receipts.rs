//! The receipt vault: what is on disk, and how it gets there.
//!
//! **The filesystem is authoritative for `paid`, on purpose.** The vault is
//! meant to be browsable and manipulable by hand — backups, tidying,
//! cross-checking at tax time — and a database flag that could silently
//! disagree with what is actually in the folder is the failure mode this app
//! avoids everywhere else. A `receipts` row still accompanies every upload, but
//! only as audit and as the extension index; it never decides paid/not-paid.
//!
//! Checking is therefore two steps, and **both are required**:
//!
//! 1. Read `receipts.ext`, build the one exact path, `exists()` it — so a file
//!    deleted by hand correctly reads as unpaid.
//! 2. With no `receipts` row, probe the four allowed extensions — so a file
//!    *added* by hand is still found.
//!
//! Four `stat` calls at worst, still not a traversal. Skip step 2 and
//! hand-filing into the vault silently stops working, which removes the entire
//! reason the filesystem is authoritative in the first place.

use crate::db::changes::{Actor, Change, FileAction, Value};
use crate::domain::money::{Cents, Currency};
use crate::domain::period::Period;
use crate::domain::receipt_path::{self, Kind, Taxonomy};
use crate::ui::row::FilePresence;
use chrono::Utc;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

fn now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Everything about one occurrence that a path depends on.
#[derive(Clone, Debug)]
pub struct Slot {
    pub transaction_id: i64,
    pub vault: String,
    pub bucket: String,
    pub bill_name: String,
    pub instance: Option<String>,
    pub period: Period,
    /// Only consulted for the receipt, and only when the rule opted in.
    pub amount: Option<(Cents, Currency)>,
    pub amount_in_filename: bool,
}

impl Slot {
    fn taxonomy(&self) -> Taxonomy<'_> {
        Taxonomy {
            vault: &self.vault,
            bucket: &self.bucket,
            bill_name: &self.bill_name,
            instance: self.instance.as_deref(),
        }
    }

    fn path(&self, kind: Kind, ext: &str) -> String {
        receipt_path::file(
            &self.taxonomy(),
            self.period,
            kind,
            ext,
            self.amount_in_filename.then_some(self.amount).flatten(),
        )
    }
}

/// What is on disk for one occurrence, as extensions — which is what the row
/// needs, since the path ends in one.
pub async fn presence(pool: &SqlitePool, root: &Path, slot: &Slot) -> sqlx::Result<FilePresence> {
    let rows = sqlx::query!(
        r#"SELECT kind AS "kind!: String", ext AS "ext!: String"
           FROM receipts
           WHERE transaction_id = ?1 AND deleted_at IS NULL"#,
        slot.transaction_id
    )
    .fetch_all(pool)
    .await?;

    let mut out = FilePresence::default();
    for kind in Kind::ALL {
        // Step 1: the extension we recorded, if there is one.
        let known = rows
            .iter()
            .find(|r| r.kind == kind.code())
            .map(|r| r.ext.clone());

        let found = match known {
            Some(ext) if root.join(slot.path(kind, &ext)).exists() => Some(ext),
            // A recorded file that is no longer there reads as absent. The
            // disk decides, not the row.
            Some(_) => None,
            // Step 2: no row at all — somebody may have filed it by hand.
            None => receipt_path::EXTENSIONS
                .iter()
                .find(|ext| root.join(slot.path(kind, ext)).exists())
                .map(|ext| (*ext).to_string()),
        };

        match kind {
            Kind::Barcode => out.bill = found,
            Kind::Scheduled => out.scheduled = found,
            Kind::Receipt => out.receipt = found,
        }
    }
    Ok(out)
}

#[derive(Debug)]
pub enum StoreError {
    /// Not one of `pdf` / `jpg` / `png` / `txt`.
    Extension,
    /// The occurrence has no taxonomy to file under — an untracked rule or a
    /// one-off, neither of which gets attachments.
    NotTracked,
    Io(String),
    Db(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Extension => write!(f, "that file type is not one we keep"),
            StoreError::NotTracked => write!(f, "this row has nothing to file under"),
            StoreError::Io(e) | StoreError::Db(e) => write!(f, "{e}"),
        }
    }
}

/// Writes one file into the vault and records it.
///
/// The file is renamed to the deterministic convention on disk; the user's own
/// filename is kept as metadata only, for their reference.
///
/// Uploading the **receipt supersedes the scheduled file automatically** — the
/// promise is resolved, so only the definitive proof is left standing. That is
/// a system-triggered soft delete, logged like a manual one but with
/// `reason = superseded`, so `u` on the upload restores the scheduled file too
/// and the audit trail is honest about why it moved.
pub async fn store(
    pool: &SqlitePool,
    root: &Path,
    actor: Actor,
    slot: &Slot,
    kind: Kind,
    original_filename: &str,
    bytes: &[u8],
) -> Result<String, StoreError> {
    let ext = Path::new(original_filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| receipt_path::is_allowed_extension(e))
        .ok_or(StoreError::Extension)?;

    if slot.bucket.trim().is_empty() || slot.bill_name.trim().is_empty() {
        return Err(StoreError::NotTracked);
    }

    // **Replacing a file moves the old one aside first.** This used to write
    // straight over the same path and only mark the old row deleted — so the
    // old bytes were gone, and the "deleted" row's path pointed at the new
    // file. A bill replaced by the wrong upload could not be got back. Now it
    // is the same soft delete every other removal is, tagged `superseded`,
    // and `u` walks back through it: first the new upload, then the old file.
    soft_delete(pool, root, actor, slot, kind, "superseded", None).await?;

    let rel = slot.path(kind, &ext);
    let full = root.join(&rel);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| StoreError::Io(e.to_string()))?;
    }
    std::fs::write(&full, bytes).map_err(|e| StoreError::Io(e.to_string()))?;

    let mut tx = pool.begin().await.map_err(db)?;
    let at = now();
    let kind_s = kind.code();
    let by = match actor {
        Actor::User(id) | Actor::Import(Some(id)) => id,
        // Only a person uploads a file today; the column is NOT NULL and a
        // system upload has no meaning yet.
        _ => return Err(StoreError::Db("no uploader".into())),
    };

    // One live file of each kind per occurrence. The soft delete above has
    // already retired the row of a file that was on disk; this catches a row
    // whose file had vanished by hand, so there is still only one live row.
    sqlx::query!(
        r#"UPDATE receipts SET deleted_at = ?3, deleted_by = ?4,
                               delete_reason = 'manual', deleted_from_path = path
           WHERE transaction_id = ?1 AND kind = ?2 AND deleted_at IS NULL"#,
        slot.transaction_id,
        kind_s,
        at,
        by,
    )
    .execute(&mut *tx)
    .await
    .map_err(db)?;

    let receipt_id = sqlx::query_scalar!(
        r#"INSERT INTO receipts
               (transaction_id, kind, path, ext, original_filename, uploaded_by, uploaded_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
           RETURNING id AS "id!: i64""#,
        slot.transaction_id,
        kind_s,
        rel,
        ext,
        original_filename,
        by,
        at,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;

    Change::file(actor, FileAction::Upload, receipt_id)
        .in_period(slot.period)
        .diff("path", Value::Path(None), Value::path(&rel))
        .diff("kind", Value::Text(None), Value::text(kind.code()))
        .diff(
            "original_filename",
            Value::Text(None),
            Value::text(original_filename),
        )
        .write(&mut tx)
        .await
        .map_err(db)?;

    tx.commit().await.map_err(db)?;

    if kind == Kind::Receipt {
        supersede_scheduled(pool, root, actor, slot).await?;
    }

    Ok(rel)
}

/// The receipt landed, so the scheduled file has done its job.
async fn supersede_scheduled(
    pool: &SqlitePool,
    root: &Path,
    actor: Actor,
    slot: &Slot,
) -> Result<(), StoreError> {
    let present = presence(pool, root, slot).await.map_err(db)?;
    if present.scheduled.is_none() {
        return Ok(());
    }
    soft_delete(pool, root, actor, slot, Kind::Scheduled, "superseded", None)
        .await
        .map(|_| ())
}

/// Moves a file into the member's trash, `{slug}/.delete/`, under the path it
/// had, and records the move.
/// Never removed from disk — the same rule the ledger follows.
pub async fn soft_delete(
    pool: &SqlitePool,
    root: &Path,
    actor: Actor,
    slot: &Slot,
    kind: Kind,
    reason: &str,
    // The change this reverses, when it is an undo. Without it `u` would
    // oscillate between the same two entries forever instead of walking back
    // through history: `last_undoable` skips a change something points at, and
    // that pointer is this.
    undo_of: Option<i64>,
) -> Result<bool, StoreError> {
    let present = presence(pool, root, slot).await.map_err(db)?;
    let ext = match kind {
        Kind::Barcode => present.bill,
        Kind::Scheduled => present.scheduled,
        Kind::Receipt => present.receipt,
    };
    let Some(ext) = ext else { return Ok(false) };

    let rel = slot.path(kind, &ext);
    let from_full = root.join(&rel);

    // The row first, so the file's new name can carry its id — see
    // `receipt_path::deleted_path_for`. The rename happens while the
    // transaction is open and the commit comes last, so the disk and the
    // database never disagree about where the file is.
    let mut tx = pool.begin().await.map_err(db)?;
    let at = now();
    let kind_s = kind.code();
    let by = match actor {
        Actor::User(id) | Actor::Import(Some(id)) => Some(id),
        // A supersede is the app acting on its own, which the column allows.
        _ => None,
    };

    let receipt_id: Option<i64> = sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM receipts
           WHERE transaction_id = ?1 AND kind = ?2 AND deleted_at IS NULL"#,
        slot.transaction_id,
        kind_s,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;

    // A hand-filed file has no receipts row, so the delete records one for it:
    // the move still has to be undoable, and the row is where the old path
    // lives.
    let id = match receipt_id {
        Some(id) => {
            sqlx::query!(
                r#"UPDATE receipts SET deleted_at = ?2, deleted_by = ?3,
                                       delete_reason = ?4, deleted_from_path = path
                   WHERE id = ?1"#,
                id,
                at,
                by,
                reason,
            )
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            id
        }
        None => sqlx::query_scalar!(
            r#"INSERT INTO receipts
                   (transaction_id, kind, path, ext, original_filename,
                    uploaded_by, uploaded_at, deleted_at, deleted_by,
                    delete_reason, deleted_from_path)
               VALUES (?1, ?2, ?3, ?4, '(filed by hand)', ?5, ?6, ?6, ?5, ?7, ?3)
               RETURNING id AS "id!: i64""#,
            slot.transaction_id,
            kind_s,
            rel,
            ext,
            by,
            at,
            reason,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?,
    };

    let to = receipt_path::deleted_path_for(&rel, id);
    let to_full = root.join(&to);
    if let Some(parent) = to_full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| StoreError::Io(e.to_string()))?;
    }
    if to_full.exists() {
        return Err(StoreError::Io(format!(
            "{} already exists — refusing to overwrite a deleted file",
            to_full.display()
        )));
    }
    std::fs::rename(&from_full, &to_full).map_err(|e| StoreError::Io(e.to_string()))?;
    prune_empty(root, &rel);

    let mut entry = Change::file(actor, FileAction::Delete, id)
        .in_period(slot.period)
        .note(reason)
        .diff("path", Value::path(&rel), Value::Path(None));
    if let Some(of) = undo_of {
        entry = entry.undoing(of);
    }
    if let Err(e) = entry.write(&mut tx).await {
        let _ = std::fs::rename(&to_full, &from_full);
        return Err(db(e));
    }
    if let Err(e) = tx.commit().await {
        let _ = std::fs::rename(&to_full, &from_full);
        return Err(db(e));
    }
    Ok(true)
}

/// Puts a soft-deleted file back where it was. What `u` does to an upload's
/// supersede, and to a manual delete.
pub async fn restore(
    pool: &SqlitePool,
    root: &Path,
    actor: Actor,
    receipt_id: i64,
    undo_of: Option<i64>,
) -> Result<bool, StoreError> {
    let rel: Option<String> = sqlx::query_scalar!(
        r#"SELECT deleted_from_path AS "p: String" FROM receipts
           WHERE id = ?1 AND deleted_at IS NOT NULL"#,
        receipt_id
    )
    .fetch_optional(pool)
    .await
    .map_err(db)?
    .flatten();

    let Some(rel) = rel else { return Ok(false) };

    let Some(from) = deleted_copy(root, &rel, receipt_id) else {
        // Nothing on disk to put back — the row is still restored, and the
        // disk goes on deciding what it shows, as it does everywhere.
        return restore_row_only(pool, actor, receipt_id, &rel, undo_of).await;
    };
    let to = root.join(&rel);
    // Restoring over a live file would be the hard delete this module exists
    // to prevent, pointed the other way.
    if from.exists() && to.exists() {
        return Err(StoreError::Io(format!(
            "{} is occupied — move the current file aside before restoring",
            to.display()
        )));
    }
    if from.exists() {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::Io(e.to_string()))?;
        }
        std::fs::rename(&from, &to).map_err(|e| StoreError::Io(e.to_string()))?;
    }

    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query!(
        r#"UPDATE receipts SET deleted_at = NULL, deleted_by = NULL,
                               delete_reason = NULL, deleted_from_path = NULL
           WHERE id = ?1"#,
        receipt_id
    )
    .execute(&mut *tx)
    .await
    .map_err(db)?;

    let mut entry = Change::file(actor, FileAction::Upload, receipt_id).diff(
        "path",
        Value::Path(None),
        Value::path(&rel),
    );
    if let Some(of) = undo_of {
        entry = entry.undoing(of);
    }
    entry.write(&mut tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(true)
}

/// Where the vault lives, as an absolute path.
pub fn root(cfg: &crate::config::Config) -> PathBuf {
    cfg.receipts_root.clone()
}

fn db(e: sqlx::Error) -> StoreError {
    StoreError::Db(e.to_string())
}

/// Where a deleted file is on disk now, looked for in every place a delete
/// has ever put one: the member's trash (today), a row-named `.delete/`
/// beside it, and the legacy fixed name beside it.
pub fn deleted_copy(root: &Path, rel: &str, receipt_id: i64) -> Option<PathBuf> {
    [
        receipt_path::deleted_path_for(rel, receipt_id),
        receipt_path::deleted_path_beside(rel, receipt_id),
        receipt_path::deleted_path(rel),
    ]
    .into_iter()
    .map(|p| root.join(p))
    .find(|p| p.exists())
}

/// Removes the folders a move left empty, from the file's own folder up to —
/// never including — the member's folder. The live tree then holds only what
/// is live: a bill whose every file was deleted stops showing as a folder.
/// `remove_dir` refuses a folder with anything in it, which is the whole
/// safety of this: nothing here can remove a file.
pub fn prune_empty(root: &Path, rel: &str) {
    let Some((vault, _)) = rel.split_once('/') else {
        return;
    };
    let stop = root.join(vault);
    let mut dir = root.join(rel).parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        if d == stop || !d.starts_with(&stop) || std::fs::remove_dir(&d).is_err() {
            break;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
}

/// A restore with no file left to move: the row is un-deleted and the log
/// says so, and the disk still decides whether anything shows.
async fn restore_row_only(
    pool: &SqlitePool,
    actor: Actor,
    receipt_id: i64,
    rel: &str,
    undo_of: Option<i64>,
) -> Result<bool, StoreError> {
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query!(
        r#"UPDATE receipts SET deleted_at = NULL, deleted_by = NULL,
                               delete_reason = NULL, deleted_from_path = NULL
           WHERE id = ?1"#,
        receipt_id
    )
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    let mut entry = Change::file(actor, FileAction::Upload, receipt_id).diff(
        "path",
        Value::Path(None),
        Value::path(rel),
    );
    if let Some(of) = undo_of {
        entry = entry.undoing(of);
    }
    entry.write(&mut tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// a row's files follow the row
// ---------------------------------------------------------------------------

/// The note on a file delete that happened because its ROW was deleted.
///
/// It is how `u` knows those moves are part of the row's delete rather than
/// separate steps: `db::tx::last_undoable` skips them, and restoring the row
/// brings them back — one keystroke for the whole thing, not one per file.
pub const WITH_ROW: &str = "moved with its row";

/// Moves every live file of a row into the member's trash, because the row
/// was deleted. A deleted row's bill left standing in the vault reads as
/// live to anybody browsing it — and the vault is meant to be browsed.
///
/// Works from the `receipts` rows rather than a computed slot, because the
/// row is (or is about to be) deleted and the slot lookup refuses one. A file
/// filed by hand, with no `receipts` row, is left where it is.
pub async fn retire_with_row(
    pool: &SqlitePool,
    root: &Path,
    actor: Actor,
    transaction_id: i64,
) -> Result<usize, StoreError> {
    let live = sqlx::query!(
        r#"SELECT r.id AS "id!: i64", r.path AS "path!: String", t.period AS "period!: String"
           FROM receipts r JOIN transactions t ON t.id = r.transaction_id
           WHERE r.transaction_id = ?1 AND r.deleted_at IS NULL"#,
        transaction_id
    )
    .fetch_all(pool)
    .await
    .map_err(db)?;

    let by = match actor {
        Actor::User(id) | Actor::Import(Some(id)) => Some(id),
        _ => None,
    };
    let mut moved = 0;
    for r in live {
        let from = root.join(&r.path);
        let to = root.join(receipt_path::deleted_path_for(&r.path, r.id));
        let on_disk = from.exists();

        let mut tx = pool.begin().await.map_err(db)?;
        let at = now();
        sqlx::query!(
            r#"UPDATE receipts SET deleted_at = ?2, deleted_by = ?3,
                                   delete_reason = 'manual', deleted_from_path = path
               WHERE id = ?1"#,
            r.id,
            at,
            by
        )
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        let mut entry = Change::file(actor, FileAction::Delete, r.id)
            .note(WITH_ROW)
            .diff("path", Value::path(&r.path), Value::Path(None));
        if let Some(p) = Period::parse(&r.period) {
            entry = entry.in_period(p);
        }
        entry.write(&mut tx).await.map_err(db)?;

        if on_disk {
            if to.exists() {
                return Err(StoreError::Io(format!(
                    "{} already exists — refusing to overwrite a deleted file",
                    to.display()
                )));
            }
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| StoreError::Io(e.to_string()))?;
            }
            std::fs::rename(&from, &to).map_err(|e| StoreError::Io(e.to_string()))?;
        }
        if let Err(e) = tx.commit().await {
            if on_disk {
                let _ = std::fs::rename(&to, &from);
            }
            return Err(db(e));
        }
        if on_disk {
            prune_empty(root, &r.path);
        }
        moved += 1;
    }
    Ok(moved)
}

/// Puts back the files that [`retire_with_row`] moved, because the row was
/// restored. Each restore points at the move it reverses, so the log reads
/// "moved with its row", then "restored · undo of …", like any other undo.
pub async fn restore_with_row(
    pool: &SqlitePool,
    root: &Path,
    actor: Actor,
    transaction_id: i64,
) -> Result<usize, StoreError> {
    let moves = sqlx::query!(
        r#"SELECT c.id AS "change_id!: i64", c.entity_id AS "receipt_id!: i64"
           FROM change c JOIN receipts r ON r.id = c.entity_id
           WHERE c.entity = 'file' AND c.action = 'file_delete' AND c.note = ?2
             AND r.transaction_id = ?1 AND r.deleted_at IS NOT NULL
             AND NOT EXISTS (SELECT 1 FROM change u WHERE u.undo_of = c.id)"#,
        transaction_id,
        WITH_ROW
    )
    .fetch_all(pool)
    .await
    .map_err(db)?;
    let mut n = 0;
    for m in moves {
        if restore(pool, root, actor, m.receipt_id, Some(m.change_id)).await? {
            n += 1;
        }
    }
    Ok(n)
}
