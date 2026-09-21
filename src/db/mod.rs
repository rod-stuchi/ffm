//! The store. One SQLite file, one process, one household.

pub mod auth;
pub mod changes;
pub mod ledger;
pub mod settings;
pub mod tx;

use crate::config::Config;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::time::Duration;

/// Opens the pool and brings the schema up to date.
///
/// Two PRAGMAs live here rather than in the migration, and neither is a style
/// choice:
///
/// * `foreign_keys` is **per connection and off by default**, so every
///   `REFERENCES` in the schema is decorative until it is set. Setting it in
///   the connect options is the only place that covers every connection the
///   pool will ever open.
/// * `journal_mode = WAL` **cannot run inside a transaction**, and SQLx runs
///   each migration in one — it fails with "cannot change into wal mode from
///   within a transaction".
pub async fn connect(cfg: &Config) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(&cfg.db_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        // NORMAL is the right pairing with WAL: a crash can lose the last
        // committed transaction only if the OS itself goes down, and this is a
        // household ledger on one machine, not a payment processor.
        .synchronous(SqliteSynchronous::Normal)
        // Two people saving in the same second is the whole concurrency story
        // here. A writer that waits five seconds is invisible; one that returns
        // SQLITE_BUSY is a lost edit.
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
