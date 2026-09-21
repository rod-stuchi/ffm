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

pub mod config;
pub mod db;
pub mod domain;
pub mod services;
pub mod state;
pub mod ui;
pub mod web;
