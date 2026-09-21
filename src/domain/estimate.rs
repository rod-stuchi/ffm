//! Projecting an income that varies.
//!
//! A commission or a freelance invoice lands differently every month, so a
//! rule with `varies` cannot seed a period from its own number the way a fixed
//! bill does. Until the period is confirmed the row shows an **estimate**,
//! computed from history.
//!
//! **The estimate is computed, never stored.** What persists is the rule's
//! baseline or the real confirmed figure; the projection is derived on read,
//! so it follows new history instead of freezing an old guess into the data —
//! and it never writes a spurious "amount changed" entry into the change log.

use crate::domain::money::Cents;

/// How to read the history. Per rule, because a steady retainer and a
/// commission that swings want different answers.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Basis {
    /// The default, and deliberately conservative: for income the dangerous
    /// error is over-estimating, which produces a shortfall the household then
    /// has to absorb. Bills are the mirror image, which is why their
    /// carry-forward default points the other way.
    #[default]
    Lowest,
    Average,
    Last,
}

impl Basis {
    pub fn code(self) -> &'static str {
        match self {
            Basis::Lowest => "lowest",
            Basis::Average => "average",
            Basis::Last => "last",
        }
    }

    pub fn parse(s: &str) -> Option<Basis> {
        match s {
            "lowest" => Some(Basis::Lowest),
            "average" => Some(Basis::Average),
            "last" => Some(Basis::Last),
            _ => None,
        }
    }
}

/// One period that actually happened, newest first.
///
/// Only **confirmed** periods reach this: an unconfirmed month is skipped
/// entirely, never folded in. Two unconfirmed periods back to back would
/// otherwise project a guess from another guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirmed {
    /// `YYYY-MM`, for the box that shows which months an estimate came from.
    pub period: String,
    pub amount: Cents,
}

/// What the row will show, and what it was worked out from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projection {
    pub amount: Cents,
    /// The months that fed it, newest first — shown dim under the figure, so
    /// a household can see *why* the number is what it is before trusting it.
    pub from: Vec<String>,
}

/// Projects from history, newest first, over the last `window` confirmed
/// periods.
///
/// `fallback` is the rule's own stated amount: a rule's first occurrence has
/// nothing behind it, which is the identical carve-out a bill's carry-forward
/// already has.
pub fn project(
    history: &[Confirmed],
    basis: Basis,
    window: usize,
    fallback: Option<Cents>,
) -> Option<Projection> {
    let considered: Vec<&Confirmed> = history.iter().take(window.max(1)).collect();

    if considered.is_empty() {
        // Nothing to learn from yet. The rule's own number is a stated
        // expectation rather than a projection, so it is not marked as one —
        // see `is_projection`.
        return fallback.map(|amount| Projection {
            amount,
            from: Vec::new(),
        });
    }

    let amount = match basis {
        Basis::Last => considered[0].amount,
        Basis::Lowest => considered.iter().map(|c| c.amount).min()?,
        Basis::Average => {
            let total: i64 = considered.iter().map(|c| c.amount.get()).sum();
            // Integer division truncates, which rounds an average DOWN — the
            // conservative direction for income, and the same instinct the
            // default basis follows.
            Cents(total / considered.len() as i64)
        }
    };

    Some(Projection {
        amount,
        from: considered.iter().map(|c| c.period.clone()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(months: &[(&str, i64)]) -> Vec<Confirmed> {
        months
            .iter()
            .map(|(p, c)| Confirmed {
                period: (*p).to_string(),
                amount: Cents(*c),
            })
            .collect()
    }

    #[test]
    fn lowest_of_the_last_three_is_the_default() {
        let history = h(&[
            ("2026-08", 520_000),
            ("2026-07", 380_000),
            ("2026-06", 610_000),
            // Older, and outside a window of three — a bumper month two
            // quarters back must not drag a projection up.
            ("2026-05", 900_000),
        ]);
        let p = project(&history, Basis::Lowest, 3, None).unwrap();
        assert_eq!(p.amount, Cents(380_000));
        assert_eq!(p.from, ["2026-08", "2026-07", "2026-06"]);
    }

    #[test]
    fn average_rounds_down_because_over_estimating_is_the_dangerous_error() {
        let p = project(
            &h(&[("2026-08", 100), ("2026-07", 101)]),
            Basis::Average,
            3,
            None,
        );
        assert_eq!(p.unwrap().amount, Cents(100), "not 101");
    }

    #[test]
    fn last_is_the_most_recent_confirmed_period() {
        let history = h(&[("2026-08", 520_000), ("2026-07", 380_000)]);
        assert_eq!(
            project(&history, Basis::Last, 3, None).unwrap().amount,
            Cents(520_000)
        );
    }

    #[test]
    fn a_rules_first_occurrence_falls_back_to_its_own_amount() {
        // Nothing behind it — the identical carve-out a bill's carry-forward
        // already has.
        let p = project(&[], Basis::Lowest, 3, Some(Cents(420_000))).unwrap();
        assert_eq!(p.amount, Cents(420_000));
        assert!(p.from.is_empty(), "there is nothing to have come from");

        // And with no fallback either, there is simply no number to show.
        assert_eq!(project(&[], Basis::Lowest, 3, None), None);
    }

    #[test]
    fn a_window_of_one_is_a_window_not_a_bug() {
        let history = h(&[("2026-08", 520_000), ("2026-07", 380_000)]);
        assert_eq!(
            project(&history, Basis::Lowest, 1, None).unwrap().amount,
            Cents(520_000),
            "one month back, so the lowest of it is it"
        );
        // A zero window is nonsense; treat it as one rather than dividing by it.
        assert!(project(&history, Basis::Average, 0, None).is_some());
    }

    #[test]
    fn the_basis_round_trips_through_its_stored_code() {
        for b in [Basis::Lowest, Basis::Average, Basis::Last] {
            assert_eq!(Basis::parse(b.code()), Some(b));
        }
        assert_eq!(Basis::parse("nonsense"), None);
        assert_eq!(Basis::default(), Basis::Lowest);
    }
}
