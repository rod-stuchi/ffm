//! Presentation. Everything a template is allowed to call lives here.
//!
//! The boundary with `domain` is the point: `domain` is where money *is*,
//! `ui` is where it is rendered. A template can reach `ui::fmt`; it cannot
//! reach a database or a filesystem.

pub mod fmt;
pub mod i18n;
pub mod icons;
pub mod log;
pub mod row;
pub mod theme;
