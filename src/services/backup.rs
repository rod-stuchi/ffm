//! A rolling two-month backup of the database.
//!
//! **`VACUUM INTO`, not a file copy.** SQLite in WAL mode keeps recent commits
//! in a sidecar file, so copying `ffm.db` on its own captures a database
//! missing whatever has not been checkpointed yet — a backup that looks fine
//! until the day it is needed. `VACUUM INTO` asks SQLite itself for a
//! consistent snapshot, while the app keeps running.
//!
//! Two months, then the oldest goes. The point is recovering from "I deleted
//! the wrong thing last Tuesday", not archival: the ledger already keeps
//! everything, since nothing is ever hard-deleted.

use chrono::NaiveDate;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Once a day is the right cadence for a household: the change log records
/// every edit anyway, so a backup is a floor rather than a journal.
const TICK: Duration = Duration::from_secs(24 * 60 * 60);

/// How long a snapshot is kept. Two months, as decided.
const KEEP_DAYS: i64 = 62;

fn name_for(day: NaiveDate) -> String {
    format!("ffm-{}.db", day.format("%Y-%m-%d"))
}

/// Writes today's snapshot and drops anything older than the window.
///
/// Idempotent by name: running twice in a day overwrites the same file rather
/// than accumulating, so a restart does not fill the disk.
pub async fn run(pool: &SqlitePool, dir: &Path, today: NaiveDate) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let target = dir.join(name_for(today));

    // VACUUM INTO refuses an existing file, and "today's backup, again" is a
    // replacement rather than a second one.
    if target.exists() {
        std::fs::remove_file(&target)?;
    }

    // The path goes into SQL as a literal, so a directory with a quote in it
    // would otherwise end the string early.
    let escaped = target.display().to_string().replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{escaped}'"))
        .execute(pool)
        .await?;

    prune(dir, today)?;
    Ok(target)
}

/// Removes snapshots past the window — and **only** files this module named,
/// so a `backup_dir` that happens to hold something else keeps it.
fn prune(dir: &Path, today: NaiveDate) -> anyhow::Result<()> {
    let cutoff = today - chrono::Duration::days(KEEP_DAYS);
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(date) = name
            .strip_prefix("ffm-")
            .and_then(|rest| rest.strip_suffix(".db"))
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        else {
            continue;
        };
        if date < cutoff {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// One snapshot at startup, then one a day.
///
/// Startup matters for the same reason it does for the materializer: a process
/// that was down for a week should not also be a week without a backup.
pub fn spawn(pool: SqlitePool, dir: PathBuf) {
    tokio::spawn(async move {
        loop {
            match run(&pool, &dir, crate::clock::today()).await {
                Ok(path) => tracing::info!("backup · {}", path.display()),
                // Not fatal: the ledger is still working, and the next tick
                // tries again. A household that cannot back up should know,
                // not stop.
                Err(e) => tracing::error!("backup failed: {e}"),
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Flushes the WAL back into the main database file.
///
/// Called on the way out so the file left behind is complete on its own. WAL
/// mode is why this is needed at all: without it, `ffm.db` is only the whole
/// database when read *with* its sidecar, which is fine for the app and
/// surprising for whoever copies the file.
pub async fn checkpoint(pool: &SqlitePool) {
    match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await
    {
        Ok(_) => tracing::info!("wal checkpointed"),
        Err(e) => tracing::warn!("could not checkpoint the wal: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_named_for_its_day_so_a_restart_replaces_rather_than_piles_up() {
        let day = NaiveDate::from_ymd_opt(2026, 9, 22).unwrap();
        assert_eq!(name_for(day), "ffm-2026-09-22.db");
    }

    #[test]
    fn pruning_removes_only_what_this_module_named() {
        let dir = std::env::temp_dir().join("ffm-backup-prune-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 9, 22).unwrap();
        let old = name_for(today - chrono::Duration::days(90));
        let recent = name_for(today - chrono::Duration::days(30));
        for f in [
            &old,
            &recent,
            &"notes.txt".to_string(),
            &"ffm-backup.db".to_string(),
        ] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }

        prune(&dir, today).unwrap();

        assert!(!dir.join(&old).exists(), "past the window");
        assert!(dir.join(&recent).exists(), "inside it");
        // A backup_dir that happens to hold something else keeps it: this
        // module deletes only files it can prove it wrote.
        assert!(dir.join("notes.txt").exists());
        assert!(dir.join("ffm-backup.db").exists(), "not a date, not ours");
    }
}
