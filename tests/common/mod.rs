//! Shared test setup: a real SQLite file, migrated, optionally seeded.
//!
//! A real file rather than `:memory:` on purpose — WAL, the busy timeout and
//! the append-only triggers are all properties of how the app actually opens
//! the database, and an in-memory pool would quietly test something else.

use ffm::config::Config;
use ffm::db;
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

#[allow(dead_code)] // each test binary uses one or the other
pub async fn pool() -> SqlitePool {
    let dir = std::env::temp_dir().join(format!("ffm-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("t{}.db", N.fetch_add(1, Ordering::Relaxed)));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", file.display()));
    }

    let cfg = Config {
        db_path: file,
        receipts_root: dir.clone(),
        backup_dir: dir,
        file: None,
    };
    db::connect(&cfg).await.expect("migrate")
}

#[allow(dead_code)]
pub async fn seeded() -> SqlitePool {
    let pool = pool().await;
    // The same file `just seed` applies, so a fixture cannot drift from what a
    // developer is actually looking at in the browser.
    let sql = include_str!("../../seeds/household.sql");
    for stmt in sql.split(";\n") {
        if stmt.trim().is_empty() {
            continue;
        }
        sqlx::query(stmt).execute(&pool).await.expect(stmt);
    }
    pool
}
