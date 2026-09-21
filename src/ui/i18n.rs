//! The generated message catalog.
//!
//! `build.rs` turns `i18n/*.ftl` into typed accessors, so a renamed id breaks
//! `cargo build` in every template that used it. See build.rs for what is
//! checked and why.

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));
