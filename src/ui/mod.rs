//! Presentation. Everything a template is allowed to call lives here.
//!
//! The boundary with `domain` is the point: `domain` is where money *is*,
//! `ui` is where it is rendered. A template can reach `ui::fmt`; it cannot
//! reach a database or a filesystem.

// Scaffolding, phase 1 — see the note in domain/mod.rs. Removed in phase 2.
#![allow(dead_code)]

pub mod fmt;
pub mod i18n;
pub mod row;
pub mod theme;
