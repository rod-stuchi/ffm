//! How a name sorts — one answer for every A–Z in the app.
//!
//! Rows by title (`ledger_sort = "name"`) and groups by bucket
//! (`group_order = "name"`) both read it, so the two cannot disagree about
//! where "Água" goes.

/// A–Z the way a Brazilian household reads it: "Água" among the A's rather
/// than after "Z", which is where a byte comparison puts an accented letter.
/// Case is set aside too, so "mercado" and "Mercado" sit together.
///
/// Done in Rust rather than SQL: SQLite's `lower()` folds only ASCII, and a
/// custom collation would be missing from the connection SQLx checks queries
/// against at compile time.
pub fn name_key(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_and_case_are_set_aside() {
        assert_eq!(name_key("Água · Sabesp"), "agua · sabesp");
        assert_eq!(name_key("DÍVIDAS"), "dividas");
        let mut names = vec!["Mercado", "energia", "Água", "Zelador"];
        names.sort_by_cached_key(|n| name_key(n));
        assert_eq!(names, ["Água", "energia", "Mercado", "Zelador"]);
    }
}
