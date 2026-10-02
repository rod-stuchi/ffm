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
/// The tilde is a separate visual channel from the quieter color the template
/// applies, and both say the same thing — this figure is a guess from history,
/// not a fact. It propagates: a pane's INs and Balance, and the household's IN
/// and NET, all carry the tilde when any component is projected.
pub fn money_estimated(amount: Cents, currency: Currency) -> String {
    format!("~{}", money(amount, currency))
}

/// A month's NET in the picker: `+20k`, `−3,4k`, `+850`.
///
/// Signed, because a picker cell has no `kind` to carry direction the way a
/// row does. No symbol: twelve `R$` in a 320px popup is noise, and the ledger
/// bar beside it already says which currency this is. The decimal separator is
/// still the currency's own, so a BRL household reads `3,4k` and a USD one
/// `3.4k` — the one part of the format a compact figure keeps.
///
/// Whole units under a thousand, one decimal under ten thousand, whole
/// thousands above. Zero is `0`, with no sign to argue about.
pub fn money_compact(amount: Cents, currency: Currency) -> String {
    let f = currency.format();
    let div = 10_i64.pow(f.decimals);
    let abs = amount.abs().get();
    let sign = if amount.is_negative() { "−" } else { "+" };

    let units = (abs + div / 2) / div;
    if units == 0 {
        return "0".to_string();
    }
    if units < 1000 {
        return format!("{sign}{units}");
    }
    let tenths = (abs * 10 + 500 * div) / (1000 * div);
    if tenths < 100 {
        return if tenths % 10 == 0 {
            format!("{sign}{}k", tenths / 10)
        } else {
            format!("{sign}{}{}{}k", tenths / 10, f.decimal_sep, tenths % 10)
        };
    }
    let thousands = (abs + 500 * div) / (1000 * div);
    format!("{sign}{}k", group(thousands, f.group_sep))
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

/// A file size: `412 KB`, `1,2 MB` — the decimal separator is the
/// household's, since a number reads one way in a household whatever the
/// words around it. Whole bytes and kilobytes, one decimal from megabytes up.
pub fn bytes(n: u64, currency: Currency) -> String {
    const UNITS: [&str; 3] = ["MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let kb = n as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{} KB", kb.round() as u64);
    }
    let mut v = kb / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    let tenths = (v * 10.0).round() as u64;
    format!(
        "{}{}{} {}",
        tenths / 10,
        currency.format().decimal_sep,
        tenths % 10,
        UNITS[unit]
    )
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
    fn a_file_size_reads_in_the_households_numbers() {
        assert_eq!(bytes(512, Currency::Brl), "512 B");
        assert_eq!(bytes(421_888, Currency::Brl), "412 KB");
        assert_eq!(bytes(1_258_291, Currency::Brl), "1,2 MB");
        assert_eq!(bytes(1_258_291, Currency::Usd), "1.2 MB");
    }

    #[test]
    fn compact_money_keeps_the_currency_separator_and_nothing_else() {
        assert_eq!(money_compact(Cents(123_400), Currency::Brl), "+1,2k");
        assert_eq!(money_compact(Cents(123_400), Currency::Usd), "+1.2k");
        assert_eq!(money_compact(Cents(-340_000), Currency::Brl), "−3,4k");
        assert_eq!(money_compact(Cents(85_000), Currency::Brl), "+850");
        assert_eq!(money_compact(Cents(2_000_000), Currency::Brl), "+20k");
        assert_eq!(money_compact(Cents(300_000), Currency::Brl), "+3k");
        // Rounds up across the decimal boundary rather than printing `10,0k`.
        assert_eq!(money_compact(Cents(996_000), Currency::Brl), "+10k");
        assert_eq!(money_compact(Cents(123_456_700), Currency::Usd), "+1,235k");
        assert_eq!(money_compact(Cents(40), Currency::Brl), "0");
        assert_eq!(money_compact(Cents::ZERO, Currency::Brl), "0");
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
