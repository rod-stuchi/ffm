//! What every handler is handed.

use crate::config::Config;
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    /// Read once at startup. Shown read-only in `:household` and on `/setup`,
    /// which is the only reason a handler ever needs it.
    pub cfg: Arc<Config>,
    /// Rows to re-fetch on every open page — the vault watcher's channel to
    /// the browser (`GET /live`).
    pub live: crate::services::live::Live,
}
