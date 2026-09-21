//! `Period` — which month's *view* a row belongs to.
//!
//! Explicit rather than derived from a date, because July's bill paid on 03/08
//! still belongs to July. "Months are views, never copies": the ledger slices
//! one continuous table by this value, so it is an equality match on an indexed
//! column rather than a date range.

use chrono::{Datelike, Months, NaiveDate};
use std::fmt;

/// A calendar month. Stored as `'YYYY-MM'` TEXT, which sorts correctly as a
/// string because the format is fixed-width and zero-padded.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Period {
    year: i32,
    month: u32, // 1..=12, enforced by the constructors
}

impl Period {
    pub fn new(year: i32, month: u32) -> Option<Period> {
        (1..=12).contains(&month).then_some(Period { year, month })
    }

    pub fn of(date: NaiveDate) -> Period {
        Period {
            year: date.year(),
            month: date.month(),
        }
    }

    /// Parses `'YYYY-MM'`. Strict: the schema's CHECK accepts exactly this
    /// shape, so anything else is a bug rather than input to be salvaged.
    pub fn parse(s: &str) -> Option<Period> {
        let (y, m) = s.split_once('-')?;
        if y.len() != 4 || m.len() != 2 || !y.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit()) {
            return None;
        }
        Period::new(y.parse().ok()?, m.parse().ok()?)
    }

    pub fn year(self) -> i32 {
        self.year
    }

    pub fn month(self) -> u32 {
        self.month
    }

    /// The `{year}` and `{month}` segments of a receipt path. `{month}` is
    /// zero-padded, and the year folder exists precisely so the same month
    /// cannot collide across years.
    pub fn path_year(self) -> String {
        self.year.to_string()
    }

    pub fn path_month(self) -> String {
        format!("{:02}", self.month)
    }

    pub fn first_day(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.year, self.month, 1).expect("month is 1..=12")
    }

    pub fn last_day(self) -> NaiveDate {
        self.succ_n(1)
            .first_day()
            .pred_opt()
            .expect("not the epoch")
    }

    pub fn succ(self) -> Period {
        self.succ_n(1)
    }

    pub fn pred(self) -> Period {
        self.pred_n(1)
    }

    pub fn succ_n(self, n: u32) -> Period {
        Period::of(self.first_day() + Months::new(n))
    }

    pub fn pred_n(self, n: u32) -> Period {
        Period::of(self.first_day() - Months::new(n))
    }

    /// Whole months from `self` to `other`; negative when `other` is earlier.
    /// This is what turns a barcode window's start into a page number.
    pub fn months_until(self, other: Period) -> i32 {
        (other.year - self.year) * 12 + other.month as i32 - self.month as i32
    }

    /// The occurrence's due date, given the rule's day-of-month.
    ///
    /// Clamped to the length of *this* month, which is the whole reason this
    /// is a method and not `from_ymd_opt`: a rule due on the 31st has to mean
    /// the 28th in February (or the 29th in a leap year), not `None`.
    pub fn due_date(self, day: u32) -> Option<NaiveDate> {
        if !(1..=31).contains(&day) {
            return None;
        }
        let last = self.last_day().day();
        NaiveDate::from_ymd_opt(self.year, self.month, day.min(last))
    }

    /// How far through the window from this period's first day to `target` we
    /// are on `today`, as `0.0..=1.0`. The due ring is elapsed time, not a
    /// countdown — three-quarters amber reads as "still room, but not much"
    /// with no legend to explain it.
    ///
    /// Returns `1.0` once the target has passed, which is what makes the ring
    /// go fully red for an overdue row.
    pub fn elapsed_toward(self, target: NaiveDate, today: NaiveDate) -> f32 {
        let start = self.first_day();
        if today >= target {
            return 1.0;
        }
        if today <= start {
            return 0.0;
        }
        let span = (target - start).num_days();
        if span <= 0 {
            return 1.0;
        }
        ((today - start).num_days() as f32 / span as f32).clamp(0.0, 1.0)
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn parses_and_renders_the_stored_shape() {
        let p = Period::parse("2026-09").unwrap();
        assert_eq!((p.year(), p.month()), (2026, 9));
        assert_eq!(p.to_string(), "2026-09");

        // The schema's CHECK accepts exactly this shape; so does this.
        assert!(Period::parse("2026-9").is_none());
        assert!(Period::parse("2026-13").is_none());
        assert!(Period::parse("2026-00").is_none());
        assert!(Period::parse("abcd-ef").is_none());
        assert!(Period::parse("2026-09-01").is_none());
    }

    #[test]
    fn string_order_matches_chronological_order() {
        // The reason the column is TEXT rather than an integer: ISO-8601 sorts
        // lexicographically, so ORDER BY period is already chronological.
        let mut v = ["2026-10", "2025-12", "2026-02"];
        v.sort();
        assert_eq!(v, ["2025-12", "2026-02", "2026-10"]);
    }

    #[test]
    fn crosses_year_boundaries() {
        let dec = Period::new(2026, 12).unwrap();
        assert_eq!(dec.succ().to_string(), "2027-01");
        assert_eq!(Period::new(2027, 1).unwrap().pred().to_string(), "2026-12");
        assert_eq!(dec.succ_n(13).to_string(), "2028-01");
    }

    #[test]
    fn months_until_is_signed() {
        let jan = Period::new(2026, 1).unwrap();
        let dec = Period::new(2026, 12).unwrap();
        assert_eq!(jan.months_until(dec), 11);
        assert_eq!(dec.months_until(jan), -11);
        assert_eq!(jan.months_until(jan), 0);
        // A barcode window renewed for the next year: last January is page -11,
        // which is how an out-of-window page gets flagged rather than shown.
        assert_eq!(Period::new(2027, 1).unwrap().months_until(jan), -12);
    }

    #[test]
    fn due_day_clamps_to_the_month() {
        // The case that makes this a method: a rule due on the 31st.
        assert_eq!(
            Period::new(2026, 2).unwrap().due_date(31),
            Some(d(2026, 2, 28))
        );
        assert_eq!(
            Period::new(2028, 2).unwrap().due_date(31),
            Some(d(2028, 2, 29)) // leap year
        );
        assert_eq!(
            Period::new(2026, 4).unwrap().due_date(31),
            Some(d(2026, 4, 30))
        );
        assert_eq!(
            Period::new(2026, 1).unwrap().due_date(15),
            Some(d(2026, 1, 15))
        );
        assert_eq!(Period::new(2026, 1).unwrap().due_date(0), None);
        assert_eq!(Period::new(2026, 1).unwrap().due_date(32), None);
    }

    #[test]
    fn last_day_is_right_for_every_month_shape() {
        assert_eq!(Period::new(2026, 1).unwrap().last_day(), d(2026, 1, 31));
        assert_eq!(Period::new(2026, 2).unwrap().last_day(), d(2026, 2, 28));
        assert_eq!(Period::new(2028, 2).unwrap().last_day(), d(2028, 2, 29));
        assert_eq!(Period::new(2026, 4).unwrap().last_day(), d(2026, 4, 30));
        assert_eq!(Period::new(2026, 12).unwrap().last_day(), d(2026, 12, 31));
    }

    #[test]
    fn elapsed_is_time_gone_not_time_left() {
        let sep = Period::new(2026, 9).unwrap();
        let due = d(2026, 9, 15); // 14 days from the 1st

        assert_eq!(sep.elapsed_toward(due, d(2026, 9, 1)), 0.0);
        assert!((sep.elapsed_toward(due, d(2026, 9, 8)) - 0.5).abs() < 0.01);
        // Past due is fully elapsed — this is what turns the ring red.
        assert_eq!(sep.elapsed_toward(due, d(2026, 9, 15)), 1.0);
        assert_eq!(sep.elapsed_toward(due, d(2026, 10, 2)), 1.0);
        // Viewing a future month from today: nothing has elapsed yet.
        assert_eq!(
            Period::new(2026, 11)
                .unwrap()
                .elapsed_toward(d(2026, 11, 10), d(2026, 9, 20)),
            0.0
        );
    }

    #[test]
    fn path_segments_are_zero_padded() {
        let p = Period::new(2026, 7).unwrap();
        assert_eq!(p.path_year(), "2026");
        assert_eq!(p.path_month(), "07");
    }
}
