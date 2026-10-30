//! The vault's folder names, moved once from `{uuid}-{slug}` to `{slug}`.
//!
//! The uuid prefix was there so a rename would not move a member's folder. But
//! names are `UNIQUE` and cannot be changed, and slugs are `UNIQUE` too, so it
//! bought nothing except a path nobody could read — in a vault that exists to
//! be browsed by hand. See `domain::receipt_path::vault`.
//!
//! **Idempotent, and run at every startup** before anything reads a path: a
//! member whose folder is already `{slug}` is skipped, so the step costs one
//! `stat` per member once it has done its work. It rewrites the stored
//! relative paths in the same transaction as the folder move, so the database
//! and the disk never disagree about where a file is. The change log keeps
//! the old paths as written — it is history, and append-only.

use sqlx::SqlitePool;
use std::path::Path;

#[derive(Debug)]
pub enum VaultError {
    /// Both folders exist. Merging two trees by guesswork is exactly the kind
    /// of silent decision this app refuses; the operator gets told which two.
    BothExist {
        old: String,
        new: String,
    },
    Io(String),
    Db(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultError::BothExist { old, new } => write!(
                f,
                "✗ vault · both {old} and {new} exist — merge them by hand, keeping {new}"
            ),
            VaultError::Io(e) => write!(f, "✗ vault · {e}"),
            VaultError::Db(e) => write!(f, "✗ vault · {e}"),
        }
    }
}

/// Moves every member's `{uuid}-{slug}` folder to `{slug}` and rewrites the
/// paths that point into it. Returns how many members were moved.
pub async fn drop_uuid_prefix(pool: &SqlitePool, root: &Path) -> Result<usize, VaultError> {
    let members =
        sqlx::query!(r#"SELECT uuid AS "uuid!: String", slug AS "slug!: String" FROM members"#)
            .fetch_all(pool)
            .await
            .map_err(|e| VaultError::Db(e.to_string()))?;

    let mut moved = 0;
    for m in members {
        let old_name = format!("{}-{}", m.uuid, m.slug);
        let new_name = crate::domain::receipt_path::vault(&m.slug);
        let old_dir = root.join(&old_name);
        let new_dir = root.join(&new_name);

        let old_exists = old_dir.is_dir();
        let stored = count_stored(pool, &old_name).await?;
        if !old_exists && stored == 0 {
            continue;
        }
        if old_exists && new_dir.exists() {
            return Err(VaultError::BothExist {
                old: old_dir.display().to_string(),
                new: new_dir.display().to_string(),
            });
        }

        // The database first, inside a transaction; the folder while it is
        // still open; commit last. A failed rename rolls the paths back, and a
        // failed commit renames the folder back — either way the two agree.
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| VaultError::Db(e.to_string()))?;
        let prefix = format!("{old_name}/");
        let like = format!("{prefix}%");
        let cut = (prefix.len() + 1) as i64;
        let new_prefix = format!("{new_name}/");
        for sql in [
            "UPDATE receipts SET path = ?1 || substr(path, ?2) WHERE path LIKE ?3",
            "UPDATE receipts SET deleted_from_path = ?1 || substr(deleted_from_path, ?2)
             WHERE deleted_from_path LIKE ?3",
            "UPDATE rule_barcode_window SET file_path = ?1 || substr(file_path, ?2)
             WHERE file_path LIKE ?3",
        ] {
            sqlx::query(sql)
                .bind(&new_prefix)
                .bind(cut)
                .bind(&like)
                .execute(&mut *tx)
                .await
                .map_err(|e| VaultError::Db(e.to_string()))?;
        }

        if old_exists {
            std::fs::rename(&old_dir, &new_dir)
                .map_err(|e| VaultError::Io(format!("cannot move {} — {e}", old_dir.display())))?;
        }
        if let Err(e) = tx.commit().await {
            if old_exists {
                let _ = std::fs::rename(&new_dir, &old_dir);
            }
            return Err(VaultError::Db(e.to_string()));
        }
        tracing::info!("vault · {old_name} → {new_name}");
        moved += 1;
    }
    Ok(moved)
}

async fn count_stored(pool: &SqlitePool, old_name: &str) -> Result<i64, VaultError> {
    let like = format!("{old_name}/%");
    sqlx::query_scalar::<_, i64>(
        "SELECT (SELECT count(*) FROM receipts WHERE path LIKE ?1 OR deleted_from_path LIKE ?1)
              + (SELECT count(*) FROM rule_barcode_window WHERE file_path LIKE ?1)",
    )
    .bind(like)
    .fetch_one(pool)
    .await
    .map_err(|e| VaultError::Db(e.to_string()))
}

/// Brings the vault in line with the one-trash-per-member layout, at startup.
/// Idempotent, like [`drop_uuid_prefix`]: once done, it finds nothing to do.
///
/// - A deleted file still in a `.delete/` beside where it lived is moved into
///   the member's trash (`receipt_path::deleted_path_for`), and the folders
///   that leaves empty are removed.
/// - A **deleted row whose files are still live** — deleted before a row's
///   files followed it — has them moved now, logged as the app's own move and
///   tagged so restoring the row brings them back
///   (`receipts::retire_with_row`).
///
/// Returns how many files were moved.
pub async fn tidy_trash(pool: &SqlitePool, root: &Path) -> Result<usize, VaultError> {
    use crate::domain::receipt_path::{deleted_path, deleted_path_beside, deleted_path_for};
    use crate::services::receipts;

    let mut moved = 0;

    // --- deleted files, into the member's trash -------------------------
    let deleted = sqlx::query!(
        r#"SELECT id AS "id!: i64", deleted_from_path AS "from!: String"
           FROM receipts
           WHERE deleted_at IS NOT NULL AND deleted_from_path IS NOT NULL"#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| VaultError::Db(e.to_string()))?;
    for r in &deleted {
        let target = root.join(deleted_path_for(&r.from, r.id));
        if target.exists() {
            continue;
        }
        // The legacy fixed name is shared by every delete of that slot, so it
        // is only claimed when exactly one deleted row could own it.
        let owners = deleted.iter().filter(|o| o.from == r.from).count();
        let candidates = [
            Some(deleted_path_beside(&r.from, r.id)),
            (owners == 1).then(|| deleted_path(&r.from)),
        ];
        let Some(found) = candidates
            .into_iter()
            .flatten()
            .map(|p| root.join(p))
            .find(|p| p.exists())
        else {
            continue;
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| VaultError::Io(e.to_string()))?;
        }
        std::fs::rename(&found, &target)
            .map_err(|e| VaultError::Io(format!("cannot move {} — {e}", found.display())))?;
        // `found` sat in `{dir}/.delete/`; prune from inside that folder so
        // both it and anything above it left empty go.
        if let Ok(rel) = found.strip_prefix(root) {
            receipts::prune_empty(root, &rel.to_string_lossy());
        }
        tracing::info!("vault · {} → member trash", r.from);
        moved += 1;
    }

    // --- live files of deleted rows -------------------------------------
    let rows = sqlx::query_scalar!(
        r#"SELECT DISTINCT t.id AS "id!: i64"
           FROM receipts r JOIN transactions t ON t.id = r.transaction_id
           WHERE r.deleted_at IS NULL AND t.deleted_at IS NOT NULL"#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| VaultError::Db(e.to_string()))?;
    for id in rows {
        let n = receipts::retire_with_row(pool, root, crate::db::changes::Actor::System, id)
            .await
            .map_err(|e| VaultError::Io(e.to_string()))?;
        if n > 0 {
            tracing::info!("vault · {n} file(s) of deleted row {id} → member trash");
        }
        moved += n;
    }
    Ok(moved)
}

/// The vault's upkeep at startup: [`tidy_trash`], then [`refile_all`]. Run
/// after the listener is bound — both are no-ops once done, and the ledger
/// does not need them to answer — so it is never fatal: a file that cannot
/// move is logged, and the household is not kept out of its ledger for it.
pub async fn upkeep(pool: &SqlitePool, root: &Path) {
    if let Err(e) = tidy_trash(pool, root).await {
        tracing::warn!("{e}");
    }
    match refile_all(pool, root).await {
        Ok((0, _)) => {}
        Ok((n, blocked)) => tracing::info!(
            "vault · {n} file(s) re-filed where their row now looks, {} left in place",
            blocked.len()
        ),
        Err(e) => tracing::warn!("{e}"),
    }
}

/// Re-files every live file the app holds to the path its row computes now
/// (`receipts::refile_row`), at startup. Idempotent: once the vault agrees,
/// it is one path comparison per file.
///
/// It exists for a vault left by v0.0.5 and earlier, where renaming a rule's
/// bucket, bill name or instance — or giving a row to another member —
/// moved the computed path and left the file behind, so the row read as
/// having no file while its record said it had one. It never overwrites: a
/// file whose new path is taken stays where it is, and is named in the log
/// for the operator to sort out by hand.
///
/// Returns how many files moved, and the paths left where they were.
pub async fn refile_all(
    pool: &SqlitePool,
    root: &Path,
) -> Result<(usize, Vec<String>), VaultError> {
    use crate::services::receipts;
    let rows = receipts::all_tracked(pool)
        .await
        .map_err(|e| VaultError::Db(e.to_string()))?;
    let mut all = receipts::Refiled::default();
    for (id, _, _) in rows {
        // A shutdown mid-pass ends it, rather than logging every row left as
        // a failure; the next startup finishes it.
        if pool.is_closed() {
            break;
        }
        // One file that cannot move is that file's problem, logged — not a
        // reason to keep the household out of its ledger.
        match receipts::refile_row(pool, root, id).await {
            Ok(r) => all.absorb(r),
            Err(e) => tracing::warn!("vault · files of row {id} not re-filed: {e}"),
        }
    }
    for b in &all.blocked {
        tracing::warn!("vault · not re-filed, its new path is taken: {b}");
    }
    Ok((all.moved.len(), all.blocked))
}
