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
//! inherit color from whatever control holds them — which is what lets the
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
        // The ledger bar's own three. Light/dark is a property of the device,
        // so the icons name a device state rather than a mood: a screen for
        // "whatever the system says", then sun and moon for the two the member
        // has pinned. The mockups used `○ ◐ ●`, which is exactly the emoji-ish
        // glyph the row states forbid — different weight and shape on every
        // platform, and unable to take a theme token.
        "monitor" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg>"#
        }
        "sun" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4.2"/><path d="M12 2v2.5M12 19.5V22M2 12h2.5M19.5 12H22M4.9 4.9l1.8 1.8M17.3 17.3l1.8 1.8M19.1 4.9l-1.8 1.8M6.7 17.3l-1.8 1.8"/></svg>"#
        }
        "moon" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z"/></svg>"#
        }
        // The pre-auth chrome's language switch, so it sits beside the
        // light/dark control as the same kind of button.
        "globe" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18z"/></svg>"#
        }
        "person" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="8" r="3.6"/><path d="M5 20a7 7 0 0 1 14 0"/></svg>"#
        }
        // The statusline's mode badge on a touch screen: tap it for the
        // keyboard, which a phone shows only for a focused field.
        "keyboard" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/></svg>"#
        }
        // The rule pane's PDF password: show it, and hide it again — and the
        // statusline's eye, which does the same for every amount (`gh`).
        "eye" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg>"#
        }
        "eye_off" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10.6 5.1A10 10 0 0 1 12 5c6.5 0 10 7 10 7a17 17 0 0 1-2.6 3.6M6.6 6.6A17 17 0 0 0 2 12s3.5 7 10 7a9.6 9.6 0 0 0 5.4-1.6"/><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2"/><path d="M3 3l18 18"/></svg>"#
        }
        // A rule pane section's fold: pointing right while closed, turned
        // down by CSS while open — the one icon, so the turn is animated.
        "chevron" => {
            r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6"/></svg>"#
        }
        _ => r#"<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/></svg>"#,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name the UI asks for by hand — the row's five, the ledger
    /// bar's four, the pre-auth chrome's globe and the rule pane's two eyes.
    /// A name added to `svg()` and not to here is untested; a name used in a
    /// template and not here is worse, because the fallback circle renders
    /// and nothing complains.
    const NAMES: [&str; 13] = [
        "bill",
        "scheduled",
        "receipt",
        "recurring",
        "private",
        "monitor",
        "sun",
        "moon",
        "person",
        "globe",
        "eye",
        "eye_off",
        "chevron",
    ];

    #[test]
    fn every_icon_the_row_asks_for_exists() {
        // An unknown name falls back to a circle rather than rendering nothing,
        // but the names the row actually uses must all be real.
        for name in NAMES {
            assert!(svg(name).contains("<svg"), "{name}");
            assert_ne!(svg(name), svg("nonexistent"), "{name} fell back");
        }
    }

    #[test]
    fn icons_inherit_color_rather_than_hardcoding_it() {
        // stroke/fill come from CSS (currentColor via the .ficon rule), so a
        // palette swap reaches them. A hex here would be invisible to the theme.
        for name in NAMES {
            assert!(!svg(name).contains('#'), "{name} hardcodes a color");
        }
    }
}
