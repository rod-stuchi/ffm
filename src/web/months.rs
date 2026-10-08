//! What a months expression (`1-10`, `^11,^12`) reads as, in words — shared
//! by the two fields that take one: a rule's *Only some months* and
//! `:order`'s months to copy to. One grammar (`domain::months`), one set of
//! messages for what it means and what is wrong with it.

use crate::domain::months::MonthsError;
use crate::ui::i18n::Catalog;

/// A month's mark in a read-back: its initial. The expression itself is
/// digits in every locale; the marks are the same twelve letters the rule
/// pane has always drawn.
pub(crate) const INITIALS: [&str; 12] =
    ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"];

/// The month names, from the catalog — the one place in this feature where
/// language reaches the output at all.
pub(crate) fn month_names(t: &Catalog, months: &[u32]) -> Vec<String> {
    months
        .iter()
        .map(|m| t.month_short(*m as i64).to_string())
        .collect()
}

/// What is wrong with an expression, in the member's language.
pub(crate) fn months_error(t: &Catalog, e: &MonthsError) -> String {
    match e {
        MonthsError::Empty => t.months_err_empty().to_string(),
        MonthsError::NotANumber(tok) => t.months_err_not_a_number(tok),
        MonthsError::OutOfRange(n) => t.months_err_out_of_range(*n as i64),
        MonthsError::BackwardsRange(a, b) => t.months_err_backwards(&format!("{a}-{b}")),
        MonthsError::NothingLeft => t.months_err_nothing_left().to_string(),
    }
}
