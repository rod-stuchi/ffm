//! Rendering money and dates.
//!
//! **Read the signatures.** `money` takes a `Currency` and never a `Lang`;
//! `date` takes neither. That is the three-axis rule from `CLAUDE.md`'s
//! Conventions section, made structural:
//!
//! | axis | source | varies by |
//! |---|---|---|
//! | date format | fixed `dd/mm/yyyy` | nothing |
//! | money format | `household_settings.locale_currency` | the household |
//! | language | `member_settings.language` | the member |
//!
//! Fusing them is the mistake. A member who reads the UI in English still sees
//! `R$ 1.234,56` if the household is Brazilian, and both members see the same
//! amount formatted the same way, because it is one ledger.
//!
//! Everything here formats server-side. None of this logic goes into Alpine.

use crate::domain::money::{Cents, Currency};
use chrono::NaiveDate;

/// `1234567` cents in BRL -> `R$ 12.345,67`; in EUR -> `12.345,67 €`.
///
/// Renders the magnitude only. Direction lives in the row's `kind`, and a
/// genuinely signed figure (a negative balance) goes through [`money_signed`].
pub fn money(amount: Cents, currency: Currency) -> String {
    let f = currency.format();
    format!("{}{}{}", f.prefix, digits(amount.abs(), currency), f.suffix)
}

/// A figure that can legitimately be negative — a pane's Balance, the
/// household's Net. Uses a minus sign, placed before the whole rendering so
/// `-R$ 10,00` reads as one negative amount rather than as `R$ -10,00`.
pub fn money_signed(amount: Cents, currency: Currency) -> String {
    if amount.is_negative() {
        format!("-{}", money(amount, currency))
    } else {
        money(amount, currency)
    }
}

/// An income projection: `~R$ 4.133,33`.
///
/// The tilde is a separate visual channel from the quieter colour the template
/// applies, and both say the same thing — this figure is a guess from history,
/// not a fact. It propagates: a pane's INs and Balance, and the household's IN
/// and NET, all carry the tilde when any component is projected.
pub fn money_estimated(amount: Cents, currency: Currency) -> String {
    format!("~{}", money(amount, currency))
}

/// The "no amount yet" placeholder: `R$ __,__`.
///
/// This is itself a money format, so it follows the household's currency like
/// everything else — a dollar sign next to a comma decimal would be exactly the
/// half-dynamic inconsistency the currency setting exists to rule out. Shown
/// for a tracked row whose bill has not arrived, and for a rule that does not
/// carry its amount forward.
pub fn money_placeholder(currency: Currency) -> String {
    let f = currency.format();
    let blanks = "_".repeat(f.decimals as usize);
    format!("{}__{}{}{}", f.prefix, f.decimal_sep, blanks, f.suffix)
}

/// The amount suffix in a receipt filename: `122.94` in `09_brl122.94.pdf`.
///
/// Deliberately not the display format: dot decimal, no grouping, nothing that
/// would need shell quoting. A household that switches its display currency
/// does not rewrite the names of files already on disk.
pub fn money_filename(amount: Cents, currency: Currency) -> String {
    let f = currency.format();
    let div = 10_i64.pow(f.decimals);
    let units = amount.abs().get();
    format!(
        "{}{}.{:0width$}",
        currency.filename_tag(),
        units / div,
        units % div,
        width = f.decimals as usize
    )
}

/// How this app writes a date, spelled out for copy that needs to *say* it.
///
/// It lives here rather than in the catalog on purpose: a `.ftl` containing a
/// date pattern fails the build, precisely so there is one place that decides
/// this. Copy interpolates it as an argument.
pub const DATE_PATTERN: &str = "dd/mm/yyyy";

/// `11/07/2026`. Fixed, forever, in every language.
///
/// There is no locale parameter and there will not be one: a household reading
/// the UI in English still writes dates the Brazilian way, because that is how
/// they write them on paper.
pub fn date(d: NaiveDate) -> String {
    use chrono::Datelike;
    format!("{:02}/{:02}/{:04}", d.day(), d.month(), d.year())
}

/// `07/2026` — a period in the statusline and the month picker.
pub fn period(p: crate::domain::period::Period) -> String {
    format!("{:02}/{:04}", p.month(), p.year())
}

/// The digits alone, grouped and separated for this currency: `12.345,67`.
fn digits(amount: Cents, currency: Currency) -> String {
    let f = currency.format();
    let div = 10_i64.pow(f.decimals);
    let units = amount.get();
    let int = units / div;
    let frac = units % div;

    let mut out = group(int, f.group_sep);
    if f.decimals > 0 {
        out.push(f.decimal_sep);
        out.push_str(&format!("{:0width$}", frac, width = f.decimals as usize));
    }
    out
}

/// `1234567` -> `1.234.567` with the given separator.
fn group(n: i64, sep: char) -> String {
    let s = n.abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::period::Period;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn money_follows_the_household_currency() {
        let n = Cents(1_234_567);
        assert_eq!(money(n, Currency::Brl), "R$ 12.345,67");
        assert_eq!(money(n, Currency::Usd), "$12,345.67");
        assert_eq!(money(n, Currency::Eur), "12.345,67 €");
        assert_eq!(money(n, Currency::Gbp), "£12,345.67");
    }

    #[test]
    fn grouping_kicks_in_at_the_right_place() {
        assert_eq!(money(Cents(0), Currency::Brl), "R$ 0,00");
        assert_eq!(money(Cents(5), Currency::Brl), "R$ 0,05");
        assert_eq!(money(Cents(99), Currency::Brl), "R$ 0,99");
        assert_eq!(money(Cents(100), Currency::Brl), "R$ 1,00");
        assert_eq!(money(Cents(99_999), Currency::Brl), "R$ 999,99");
        assert_eq!(money(Cents(100_000), Currency::Brl), "R$ 1.000,00");
        assert_eq!(money(Cents(100_000_000), Currency::Brl), "R$ 1.000.000,00");
    }

    #[test]
    fn the_amounts_from_the_mockup_render_exactly() {
        // Straight off design/ffm-app-mockup.html's July ledger. If the port
        // drifts, these are what notice.
        assert_eq!(money(Cents(34362), Currency::Brl), "R$ 343,62");
        assert_eq!(money(Cents(10840), Currency::Brl), "R$ 108,40");
        assert_eq!(money(Cents(96500), Currency::Brl), "R$ 965,00");
        assert_eq!(money(Cents(1_180_000), Currency::Brl), "R$ 11.800,00");
        assert_eq!(money(Cents(155_692), Currency::Brl), "R$ 1.556,92");
    }

    #[test]
    fn sign_goes_outside_the_whole_rendering() {
        // Not "R$ -10,00", and not "-12.345,67 €" split from its symbol.
        assert_eq!(money_signed(Cents(-1000), Currency::Brl), "-R$ 10,00");
        assert_eq!(money_signed(Cents(-1000), Currency::Eur), "-10,00 €");
        assert_eq!(money_signed(Cents(1000), Currency::Brl), "R$ 10,00");
    }

    #[test]
    fn placeholder_is_a_money_format_too() {
        // It has to follow the currency swap like any other amount.
        assert_eq!(money_placeholder(Currency::Brl), "R$ __,__");
        assert_eq!(money_placeholder(Currency::Usd), "$__.__");
        assert_eq!(money_placeholder(Currency::Eur), "__,__ €");
    }

    #[test]
    fn estimate_carries_a_tilde() {
        assert_eq!(
            money_estimated(Cents(413_333), Currency::Brl),
            "~R$ 4.133,33"
        );
    }

    #[test]
    fn filename_amount_is_shell_safe_and_currency_tagged() {
        // `09_brl122.94.pdf` — dot decimal, no grouping, and the tag is what
        // keeps the month readable: `09_9_99` would be ambiguous.
        assert_eq!(money_filename(Cents(12294), Currency::Brl), "brl122.94");
        assert_eq!(money_filename(Cents(155_692), Currency::Brl), "brl1556.92");
        assert_eq!(money_filename(Cents(5), Currency::Usd), "usd0.05");

        let name = money_filename(Cents(155_692), Currency::Brl);
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'),
            "nothing in a filename may need shell quoting: {name}"
        );
    }

    #[test]
    fn dates_are_fixed_regardless_of_anything() {
        assert_eq!(date(d(2026, 7, 11)), "11/07/2026");
        assert_eq!(date(d(2026, 12, 1)), "01/12/2026");
        assert_eq!(period(Period::new(2026, 7).unwrap()), "07/2026");
    }
}
