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
