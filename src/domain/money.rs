//! Money: an integer count of minor units, and the household's format for it.
//!
//! Two rules this module exists to enforce:
//!
//! 1. **Never a float.** Binary floating point cannot represent `0.10` exactly,
//!    so a ledger that adds in `f64` drifts. `Cents` is an `i64` and there is no
//!    conversion from a float anywhere in this file.
//! 2. **Arithmetic and rendering are different things.** `Cents` can be added;
//!    it deliberately does not implement `Display`, because you cannot render an
//!    amount without knowing the household's currency. Formatting lives in
//!    `ui::fmt`, whose signature takes a `Currency` and never a `Lang` — money
//!    format and language are two of the three independent axes.

use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// An amount in a currency's smallest unit. Not a float, ever.
///
/// Stored amounts are non-negative — direction lives in the transaction's
/// `kind`, not in the sign — but a computed balance is genuinely signed, so
/// this type allows negatives.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub struct Cents(pub i64);

impl Cents {
    pub const ZERO: Cents = Cents(0);

    pub fn get(self) -> i64 {
        self.0
    }

    pub fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub fn abs(self) -> Cents {
        Cents(self.0.abs())
    }

    /// Percent change from `prev` to `self`, rounded to the nearest whole
    /// percent — the month-over-month delta. `None` when there is nothing to
    /// compare against, which is also what a zero previous amount means: a
    /// change "from nothing" has no meaningful percentage.
    pub fn delta_pct(self, prev: Cents) -> Option<i32> {
        if prev.0 == 0 {
            return None;
        }
        let change = (self.0 - prev.0) as f64 / prev.0.abs() as f64 * 100.0;
        Some(change.round() as i32)
    }
}

impl Add for Cents {
    type Output = Cents;
    fn add(self, rhs: Cents) -> Cents {
        Cents(self.0 + rhs.0)
    }
}
impl Sub for Cents {
    type Output = Cents;
    fn sub(self, rhs: Cents) -> Cents {
        Cents(self.0 - rhs.0)
    }
}
impl Neg for Cents {
    type Output = Cents;
    fn neg(self) -> Cents {
        Cents(-self.0)
    }
}
impl AddAssign for Cents {
    fn add_assign(&mut self, rhs: Cents) {
        self.0 += rhs.0;
    }
}
impl SubAssign for Cents {
    fn sub_assign(&mut self, rhs: Cents) {
        self.0 -= rhs.0;
    }
}
impl Sum for Cents {
    fn sum<I: Iterator<Item = Cents>>(iter: I) -> Cents {
        Cents(iter.map(|c| c.0).sum())
    }
}

/// The household's currency. Closed set, admin-only, never free-typed — and
/// never a per-member preference, because two people looking at one ledger must
/// see one set of numbers.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub enum Currency {
    #[default]
    Brl,
    Usd,
    Eur,
    Gbp,
}

/// Choosing a currency chooses its whole conventional number format as one
/// preset — symbol, placement, decimal separator, grouping separator. They are
/// not four independent settings, which is why `:currency usd` is one command
/// and not a form.
#[derive(Copy, Clone, Debug)]
pub struct Format {
    /// Includes its own trailing space where the convention has one.
    pub prefix: &'static str,
    /// Includes its own leading space. Euro sits after the number.
    pub suffix: &'static str,
    pub decimals: u32,
    pub decimal_sep: char,
    pub group_sep: char,
}

impl Currency {
    pub const ALL: [Currency; 4] = [Currency::Brl, Currency::Usd, Currency::Eur, Currency::Gbp];

    /// The ISO code, which is what the database column stores.
    pub fn code(self) -> &'static str {
        match self {
            Currency::Brl => "BRL",
            Currency::Usd => "USD",
            Currency::Eur => "EUR",
            Currency::Gbp => "GBP",
        }
    }

    pub fn parse(s: &str) -> Option<Currency> {
        match s.to_ascii_uppercase().as_str() {
            "BRL" => Some(Currency::Brl),
            "USD" => Some(Currency::Usd),
            "EUR" => Some(Currency::Eur),
            "GBP" => Some(Currency::Gbp),
            _ => None,
        }
    }

    pub fn format(self) -> Format {
        match self {
            Currency::Brl => Format {
                prefix: "R$ ",
                suffix: "",
                decimals: 2,
                decimal_sep: ',',
                group_sep: '.',
            },
            Currency::Usd => Format {
                prefix: "$",
                suffix: "",
                decimals: 2,
                decimal_sep: '.',
                group_sep: ',',
            },
            // Euro trails the number, unlike the other three. Taken from
            // ffm-settings-mockup.html, which is the later file; the app mockup
            // still has it as a prefix and is stale here.
            Currency::Eur => Format {
                prefix: "",
                suffix: " €",
                decimals: 2,
                decimal_sep: ',',
                group_sep: '.',
            },
            Currency::Gbp => Format {
                prefix: "£",
                suffix: "",
                decimals: 2,
                decimal_sep: '.',
                group_sep: ',',
            },
        }
    }

    /// The amount suffix used in a receipt filename when a rule opts into it:
    /// `09_brl122.94.pdf`. Deliberately NOT the household's display format —
    /// a filename wants a dot decimal, no grouping and nothing needing shell
    /// quoting. The currency code is what keeps the month readable, since
    /// `09_9_99` is ambiguous.
    pub fn filename_tag(self) -> &'static str {
        match self {
            Currency::Brl => "brl",
            Currency::Usd => "usd",
            Currency::Eur => "eur",
            Currency::Gbp => "gbp",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_is_exact() {
        // The canonical float failure: 0.1 + 0.2 != 0.3 in binary floating
        // point. In minor units it is just addition.
        let sum: Cents = [Cents(10), Cents(20)].into_iter().sum();
        assert_eq!(sum, Cents(30));

        let many: Cents = std::iter::repeat_n(Cents(1), 1_000_000).sum();
        assert_eq!(many, Cents(1_000_000));
    }

    #[test]
    fn delta_pct_rounds_and_guards_zero() {
        assert_eq!(Cents(34362).delta_pct(Cents(28140)), Some(22));
        assert_eq!(Cents(28140).delta_pct(Cents(34362)), Some(-18));
        assert_eq!(Cents(100).delta_pct(Cents(100)), Some(0));
        // No previous amount to compare against is not a 0% change.
        assert_eq!(Cents(100).delta_pct(Cents::ZERO), None);
    }

    #[test]
    fn currency_codes_round_trip() {
        for c in Currency::ALL {
            assert_eq!(Currency::parse(c.code()), Some(c));
        }
        assert_eq!(Currency::parse("brl"), Some(Currency::Brl));
        assert_eq!(Currency::parse("nope"), None);
    }

    #[test]
    fn euro_trails_the_number() {
        // The one currency of the four whose symbol is a suffix. Guards against
        // a future "simplification" that assumes every symbol is a prefix.
        let eur = Currency::Eur.format();
        assert_eq!(eur.prefix, "");
        assert_eq!(eur.suffix, " €");
        assert!(
            Currency::ALL
                .iter()
                .filter(|c| !c.format().suffix.is_empty())
                .count()
                == 1
        );
    }
}
