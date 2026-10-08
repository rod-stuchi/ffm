//! The generated message catalog.
//!
//! `build.rs` turns `i18n/*.ftl` into typed accessors, so a renamed id breaks
//! `cargo build` in every template that used it. See build.rs for what is
//! checked and why.

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    /// A plural in only one locale still goes through Fluent's plural rules.
    /// Portuguese needs `agendado` / `agendados` where English has one word;
    /// the generator used to look only at English to decide, and rendered
    /// the Portuguese variants as nothing — the statusline read "3 de 5 ".
    #[test]
    fn a_plural_only_portuguese_needs_is_still_rendered() {
        let pt = Catalog::new(Lang::PtBr);
        assert_eq!(pt.selection_count(3, "5"), "3 de 5 selecionadas");
        assert_eq!(pt.selection_count(1, "5"), "1 de 5 selecionada");
        assert_eq!(pt.fold_late(1), "1 atrasado");
        assert_eq!(pt.fold_scheduled(2), "2 agendados");

        let en = Catalog::new(Lang::En);
        assert_eq!(en.selection_count(3, "5"), "3 of 5 selected");
        assert_eq!(en.fold_late(2), "2 late");
    }
}
