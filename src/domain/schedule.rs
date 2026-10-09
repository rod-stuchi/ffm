//! Which months a recurring rule runs in — decided here, and only here.
//!
//! Two readers ask: the materializer, which turns each of these months into a
//! row, and the row's `3/10` badge, which counts them. They used to be one
//! reader (the materializer's loop was the only place `months` was read), and
//! a second copy of that loop in the badge would be two answers to "does this
//! rule run in March" — the drift this codebase keeps paying for.
//!
//! A rule steps from its start by its cadence, skips a month its `months` list
//! leaves out, stops at its end date, and stops after `total` installments. A
//! skipped month is not an installment — `total` counts months that produce a
//! row — and neither is a month the caller says to skip (the materializer's
//! inactive-member stretches), which is why the caller passes that in.

use chrono::NaiveDate;

use super::months::Months;
use super::period::Period;

/// A rule's shape, as far as its months are concerned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub start: Period,
    pub end: Option<Period>,
    /// Months between occurrences, at least 1.
    pub cadence: u32,
    /// `None` runs every month. An "every month" list is normalized to `None`.
    pub months: Option<Months>,
    /// A finite run's installment count.
    pub total: Option<u32>,
}

impl Plan {
    /// From the rule's stored columns. `None` when the start date does not
    /// parse — the materializer skips such a rule rather than guessing.
    ///
    /// A months value that no longer parses runs every month rather than
    /// none: a rule that quietly stops producing rows is the worst failure the
    /// materializer has.
    pub fn from_columns(
        start_date: &str,
        end_date: Option<&str>,
        cadence_months: i64,
        months: Option<&str>,
        total_installments: Option<i64>,
    ) -> Option<Plan> {
        let date = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
        Some(Plan {
            start: Period::of(date(start_date)?),
            end: end_date.and_then(date).map(Period::of),
            cadence: cadence_months.max(1) as u32,
            months: months
                .and_then(Months::from_storage)
                .filter(|m| !m.is_all()),
            total: total_installments.map(|n| n.max(0) as u32),
        })
    }

    /// Every month the rule runs in, from its start up to and including `to`
    /// (and never past its end), oldest first. `skip` leaves a month out
    /// without counting it toward `total`.
    pub fn periods(&self, to: Period, mut skip: impl FnMut(Period) -> bool) -> Vec<Period> {
        let last = match self.end {
            Some(e) if e < to => e,
            _ => to,
        };
        let mut out = Vec::new();
        let mut period = self.start;
        while period <= last {
            // A finite run stops at its installment count, whatever the dates
            // say — the form keeps the two in sync, and this is the safety net
            // if they ever disagree.
            if self.total.is_some_and(|t| out.len() as u32 >= t) {
                break;
            }
            let runs = self
                .months
                .as_ref()
                .is_none_or(|m| m.contains(period.month()));
            if runs && !skip(period) {
                out.push(period);
            }
            period = period.succ_n(self.cadence);
        }
        out
    }

    /// The run a row's `x/N` badge counts within, if the rule has one:
    ///
    /// - **finite** (an installment count or an end date): the whole run;
    /// - **seasonal** (a months list, no end): the months of `at`'s calendar
    ///   year — IPTU's `1-10` is ten a year, and starts again in January;
    /// - **open-ended and every month**: none, there is nothing to count to.
    ///
    /// `None` too when `at` is not one of the run's months — a row left
    /// behind after the end or the months list was narrowed is not
    /// "installment 8 of 7".
    pub fn run_for(&self, at: Period) -> Option<Vec<Period>> {
        let run = if self.total.is_some() || self.end.is_some() {
            // An installment count with no end date stops itself; the bound
            // only has to be far enough for every skipped month on the way.
            let to = self.end.unwrap_or_else(|| {
                let n = self.total.unwrap_or(0).max(1);
                self.start.succ_n(self.cadence * n * 12)
            });
            self.periods(to, |_| false)
        } else if self.months.is_some() {
            let dec = Period::new(at.year(), 12)?;
            self.periods(dec, |_| false)
                .into_iter()
                .filter(|p| p.year() == at.year())
                .collect()
        } else {
            return None;
        };
        run.contains(&at).then_some(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(y: i32, m: u32) -> Period {
        Period::new(y, m).unwrap()
    }

    fn plan(
        start: &str,
        end: Option<&str>,
        cadence: i64,
        months: Option<&str>,
        total: Option<i64>,
    ) -> Plan {
        Plan::from_columns(start, end, cadence, months, total).unwrap()
    }

    #[test]
    fn a_count_stops_after_that_many() {
        let pl = plan("2026-03-05", None, 1, None, Some(4));
        let run = pl.run_for(p(2026, 4)).unwrap();
        assert_eq!(run, vec![p(2026, 3), p(2026, 4), p(2026, 5), p(2026, 6)]);
        assert_eq!(pl.periods(p(2026, 12), |_| false).len(), 4);
    }

    #[test]
    fn an_end_date_bounds_the_run() {
        let pl = plan("2026-01-10", Some("2026-07-31"), 1, None, None);
        let run = pl.run_for(p(2026, 5)).unwrap();
        assert_eq!(run.len(), 7);
        assert_eq!(run.last(), Some(&p(2026, 7)));
        assert!(
            pl.run_for(p(2026, 8)).is_none(),
            "past the end is no installment"
        );
    }

    #[test]
    fn a_seasonal_rule_counts_its_months_a_year() {
        let pl = plan("2026-01-01", None, 1, Some("1,2,3,4,5,6,7,8,9,10"), None);
        let run = pl.run_for(p(2026, 3)).unwrap();
        assert_eq!(run.len(), 10);
        assert_eq!(run[0], p(2026, 1));
        // And again next year.
        let next = pl.run_for(p(2027, 1)).unwrap();
        assert_eq!(next.len(), 10);
        assert!(next.iter().all(|x| x.year() == 2027));
        assert!(
            pl.run_for(p(2026, 11)).is_none(),
            "November is not one of its months"
        );
    }

    #[test]
    fn a_seasonal_rule_started_mid_year_counts_what_is_left_of_it() {
        let pl = plan("2026-06-15", None, 1, Some("1,2,3,4,5,6,7,8,9,10"), None);
        assert_eq!(pl.run_for(p(2026, 6)).unwrap().len(), 5, "Jun–Oct");
        assert_eq!(pl.run_for(p(2027, 6)).unwrap().len(), 10);
    }

    #[test]
    fn cadence_steps_from_the_start() {
        let pl = plan("2026-02-01", Some("2026-12-31"), 2, None, None);
        let run = pl.run_for(p(2026, 4)).unwrap();
        assert_eq!(
            run,
            vec![
                p(2026, 2),
                p(2026, 4),
                p(2026, 6),
                p(2026, 8),
                p(2026, 10),
                p(2026, 12)
            ]
        );
        assert!(pl.run_for(p(2026, 3)).is_none());
    }

    #[test]
    fn a_skipped_month_is_not_an_installment() {
        // Twelve installments, Jan–Oct only: the last two land next year.
        let pl = plan(
            "2026-01-01",
            None,
            1,
            Some("1,2,3,4,5,6,7,8,9,10"),
            Some(12),
        );
        let run = pl.run_for(p(2026, 5)).unwrap();
        assert_eq!(run.len(), 12);
        assert_eq!(run[10], p(2027, 1));
        // And a month the caller skips is not one either.
        let some = pl.periods(p(2026, 12), |m| m == p(2026, 2));
        assert_eq!(some.len(), 9);
    }

    #[test]
    fn an_open_ended_monthly_rule_has_nothing_to_count_to() {
        let pl = plan("2026-01-05", None, 1, None, None);
        assert!(pl.run_for(p(2026, 5)).is_none());
        // Every month listed is the same as no list.
        let all = plan(
            "2026-01-05",
            None,
            1,
            Some("1,2,3,4,5,6,7,8,9,10,11,12"),
            None,
        );
        assert!(all.run_for(p(2026, 5)).is_none());
    }
}
