//! FFM — a shared household ledger.
//!
//! A library plus a thin binary, rather than one binary crate. The reason is
//! not ceremony: these modules are built out phase by phase, so at any moment a
//! good deal of the API exists and is tested but has no caller yet. In a binary
//! crate that is twenty dead-code warnings, and twenty warnings is how a
//! codebase teaches people to stop reading warnings. In a library they are
//! simply public API.
//!
//! It also means the test suite exercises the same surface a caller would.

pub mod clock;
pub mod config;
pub mod db;
pub mod domain;
pub mod run;
pub mod services;
pub mod state;
pub mod ui;
pub mod web;

/// The version, as `:profile` shows it and the boot log prints it:
/// `v0.0.3` for a release build, `v0.0.3-dev` for anything `cargo run`
/// built — which is the last release plus whatever has changed since, and
/// must not be mistaken for it. The number itself is Cargo.toml's, which
/// `just release` bumps and `just release-approve` tags.
pub fn version() -> &'static str {
    if cfg!(debug_assertions) {
        concat!("v", env!("CARGO_PKG_VERSION"), "-dev")
    } else {
        concat!("v", env!("CARGO_PKG_VERSION"))
    }
}
