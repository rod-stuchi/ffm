//! Inline stroke SVGs.
//!
//! In Rust rather than an Askama `include`, for two reasons. The mechanical
//! one: an included template shares its parent's scope, and a field read there
//! arrives as `&&str`, which will not match a `&str` pattern — fighting that
//! puts type juggling into markup. The real one: choosing an icon is a decision,
//! and decisions belong beside the other pure functions of the row's fields,
//! not in the template.
//!
//! They are stroke paths in `currentColor`, not an emoji font: emoji render
//! differently on every platform and cannot take a theme token, whereas these
//! inherit colour from whatever control holds them — which is what lets the
//! file rail stay neutral and the meta flags dim when off.

pub fn svg(name: &str) -> &'static str {
    match name {
        "bill" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 2h9l5 5v15H6z"/><path d="M15 2v5h5"/><path d="M9 12h7M9 16h5"/></svg>"#
        }
        "scheduled" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M8 3v4M16 3v4M3 10h18"/><path d="M12 14v3l2 1"/></svg>"#
        }
        "receipt" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 3v18l2.5-1.5L10 21l2-1.5L14 21l2.5-1.5L19 21V3z"/><path d="M9 8h6M9 12h6"/></svg>"#
        }
        "recurring" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 12a9 9 0 0 1 15-6.7L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-15 6.7L3 16"/><path d="M3 21v-5h5"/></svg>"#
        }
        "private" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="10" width="16" height="11" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3"/></svg>"#
        }
        _ => r#"<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/></svg>"#,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_the_row_asks_for_exists() {
        // An unknown name falls back to a circle rather than rendering nothing,
        // but the names the row actually uses must all be real.
        for name in ["bill", "scheduled", "receipt", "recurring", "private"] {
            assert!(svg(name).contains("<svg"), "{name}");
            assert_ne!(svg(name), svg("nonexistent"), "{name} fell back");
        }
    }

    #[test]
    fn icons_inherit_colour_rather_than_hardcoding_it() {
        // stroke/fill come from CSS (currentColor via the .ficon rule), so a
        // palette swap reaches them. A hex here would be invisible to the theme.
        for name in ["bill", "scheduled", "receipt", "recurring", "private"] {
            assert!(!svg(name).contains('#'), "{name} hardcodes a colour");
        }
    }
}
