//! Which months of the year a recurring rule actually runs.
//!
//! Some bills only apply part of the year, every year: São Paulo's IPTU bills
//! in ten installments (Jan–Oct) and then starts again the next January; a
//! school fee runs Feb–Dec and skips the holidays. The rule still recurs
//! forever — it just skips certain months.
//!
//! This is not `total_installments` (a finite run that ends permanently) and
//! not the multi-period barcode window (which asks *which page of the shared
//! file is this month's*, not *does this month exist at all*).
//!
//! # The expression
//!
//! The form takes an expression rather than twelve toggles: toggles are a
//! mouse gesture in a keyboard-first app, and `1-10` is one thing to type.
//!
//! ```text
//! 1-10          Jan–Oct
//! 1,2,3,5,6     Jan Feb Mar May Jun
//! ^11,^12       all except Nov and Dec
//! 1-10,^5       Jan–Oct except May
//! ^6-8          all except Jun, Jul, Aug
//! ```
//!
//! **`-` is always a range and `^` is always an exclusion.** Two spellings of
//! "except" was the alternative — a leading `-` is parseable, since the range
//! separator is always infix — and it was rejected: one character, one
//! meaning, and an error message can then say plainly what `^` does.
//!
//! **Exclusions alone imply every month.** `^11,^12` means January through
//! October, because that is what somebody writing it means. Requiring
//! `1-12,^11,^12` would make the shortest useful expression the longest one.

use std::fmt;

/// The months a rule runs, as a sorted, deduplicated set of 1–12.
///
/// A rule that runs every month stores `NULL` rather than all twelve, so
/// "seasonal" and "ordinary" stay distinguishable in the column itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Months(Vec<u32>);

/// What went wrong, in terms of the thing that was typed.
///
/// Each variant names the offending token rather than a position, because the
/// field is short and the form shows the message next to it — "month 13" is
/// what somebody needs to hear, not "error at byte 4".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MonthsError {
    Empty,
    NotANumber(String),
    OutOfRange(u32),
    BackwardsRange(u32, u32),
    NothingLeft,
}

impl fmt::Display for MonthsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MonthsError::Empty => write!(f, "empty"),
            MonthsError::NotANumber(t) => write!(f, "not a month number: {t}"),
            MonthsError::OutOfRange(n) => write!(f, "month {n} — months are 1 to 12"),
            MonthsError::BackwardsRange(a, b) => write!(f, "{a}-{b} runs backwards"),
            MonthsError::NothingLeft => write!(f, "that leaves no months at all"),
        }
    }
}

pub const ALL: [u32; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

impl Months {
    /// Parses the form's expression. See the module docs for the grammar.
    pub fn parse(expr: &str) -> Result<Months, MonthsError> {
        let expr = expr.trim();
        if expr.is_empty() {
            return Err(MonthsError::Empty);
        }

        let mut include: Vec<u32> = Vec::new();
        let mut exclude: Vec<u32> = Vec::new();

        for raw in expr.split(',') {
            let term = raw.trim();
            if term.is_empty() {
                return Err(MonthsError::Empty);
            }
            let (negated, body) = match term.strip_prefix('^') {
                Some(rest) => (true, rest.trim()),
                None => (false, term),
            };
            let into = if negated { &mut exclude } else { &mut include };
            for m in run(body)? {
                into.push(m);
            }
        }

        // Exclusions with no positive term mean "every month, minus these" —
        // which is what somebody writing `^11,^12` means.
        let mut out: Vec<u32> = if include.is_empty() {
            ALL.to_vec()
        } else {
            include
        };
        out.retain(|m| !exclude.contains(m));
        out.sort_unstable();
        out.dedup();

        if out.is_empty() {
            return Err(MonthsError::NothingLeft);
        }
        Ok(Months(out))
    }

    /// From the stored column: a sorted comma list, already validated on the
    /// way in. A value that has been edited into the database by hand and no
    /// longer parses degrades to "every month" rather than failing a page
    /// render — the ledger is what people read every day.
    pub fn from_storage(s: &str) -> Option<Months> {
        let mut out: Vec<u32> = s
            .split(',')
            .filter_map(|t| t.trim().parse::<u32>().ok())
            .filter(|m| (1..=12).contains(m))
            .collect();
        out.sort_unstable();
        out.dedup();
        (!out.is_empty()).then_some(Months(out))
    }

    pub fn to_storage(&self) -> String {
        self.0
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The canonical expression, with consecutive runs compressed back into
    /// ranges — so a rule stored as twelve numbers reads `1-10` in the form
    /// rather than `1,2,3,4,5,6,7,8,9,10`.
    ///
    /// Deliberately never emits `^`: an exclusion is a convenience for typing,
    /// not a fact about the rule, and rendering one would mean choosing
    /// between two correct spellings on every read.
    pub fn to_expression(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.0.len() {
            let start = self.0[i];
            let mut end = start;
            while i + 1 < self.0.len() && self.0[i + 1] == end + 1 {
                i += 1;
                end = self.0[i];
            }
            parts.push(if end > start + 1 {
                format!("{start}-{end}")
            } else if end == start + 1 {
                format!("{start},{end}")
            } else {
                start.to_string()
            });
            i += 1;
        }
        parts.join(",")
    }

    pub fn contains(&self, month: u32) -> bool {
        self.0.contains(&month)
    }

    pub fn is_all(&self) -> bool {
        self.0.len() == 12
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The months NOT in the set, for the read-back's "Nov and Dec skipped".
    pub fn skipped(&self) -> Vec<u32> {
        ALL.iter().copied().filter(|m| !self.contains(*m)).collect()
    }

    pub fn as_slice(&self) -> &[u32] {
        &self.0
    }
}

/// One term: `5` or `6-8`.
fn run(body: &str) -> Result<Vec<u32>, MonthsError> {
    if body.is_empty() {
        return Err(MonthsError::Empty);
    }
    match body.split_once('-') {
        Some((a, b)) => {
            let (a, b) = (month(a)?, month(b)?);
            if b < a {
                return Err(MonthsError::BackwardsRange(a, b));
            }
            Ok((a..=b).collect())
        }
        None => Ok(vec![month(body)?]),
    }
}

fn month(token: &str) -> Result<u32, MonthsError> {
    let t = token.trim();
    let n: u32 = t
        .parse()
        .map_err(|_| MonthsError::NotANumber(t.to_string()))?;
    if !(1..=12).contains(&n) {
        return Err(MonthsError::OutOfRange(n));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<u32> {
        Months::parse(s).unwrap().as_slice().to_vec()
    }

    #[test]
    fn the_four_shapes_from_the_spec() {
        assert_eq!(p("1-10"), vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(p("1,2,3,5,6"), vec![1, 2, 3, 5, 6]);
        assert_eq!(p("^11,^12"), vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(p("1-10,^5"), vec![1, 2, 3, 4, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn a_range_can_be_excluded_too() {
        assert_eq!(p("^6-8"), vec![1, 2, 3, 4, 5, 9, 10, 11, 12]);
    }

    #[test]
    fn exclusions_alone_mean_every_month_minus_those() {
        // `^11,^12` must not need `1-12,` in front of it: requiring an
        // explicit base would make the shortest useful expression the longest.
        assert_eq!(p("^12"), vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    }

    #[test]
    fn dash_is_always_a_range_and_caret_is_always_an_exclusion() {
        // The two never collide, which is the whole reason only one spelling
        // of "except" is accepted.
        assert_eq!(p("1-3,^2"), vec![1, 3]);
    }

    #[test]
    fn whitespace_and_repetition_are_forgiven() {
        assert_eq!(p(" 1 , 2 , 2 , 1 "), vec![1, 2]);
        assert_eq!(p("3-5, 4"), vec![3, 4, 5]);
    }

    #[test]
    fn the_errors_name_what_was_typed() {
        assert_eq!(Months::parse(""), Err(MonthsError::Empty));
        assert_eq!(Months::parse("1,,2"), Err(MonthsError::Empty));
        assert_eq!(
            Months::parse("jan"),
            Err(MonthsError::NotANumber("jan".into()))
        );
        assert_eq!(Months::parse("13"), Err(MonthsError::OutOfRange(13)));
        assert_eq!(Months::parse("0"), Err(MonthsError::OutOfRange(0)));
        assert_eq!(Months::parse("8-3"), Err(MonthsError::BackwardsRange(8, 3)));
    }

    #[test]
    fn an_expression_that_excludes_everything_is_an_error_not_an_empty_rule() {
        // A rule that runs in no month at all would materialize nothing,
        // forever, with no way to tell it apart from one that simply has not
        // reached its start date.
        assert_eq!(Months::parse("^1-12"), Err(MonthsError::NothingLeft));
        assert_eq!(Months::parse("1-3,^1-3"), Err(MonthsError::NothingLeft));
    }

    #[test]
    fn storage_round_trips() {
        let m = Months::parse("1-10").unwrap();
        assert_eq!(m.to_storage(), "1,2,3,4,5,6,7,8,9,10");
        assert_eq!(Months::from_storage(&m.to_storage()), Some(m));
    }

    #[test]
    fn the_expression_compresses_runs_back_into_ranges() {
        // Ten stored numbers must read `1-10` in the form, or editing a
        // seasonal rule means re-reading a list every time.
        assert_eq!(Months::parse("1-10").unwrap().to_expression(), "1-10");
        assert_eq!(Months::parse("^6-8").unwrap().to_expression(), "1-5,9-12");
        assert_eq!(Months::parse("1,3,5").unwrap().to_expression(), "1,3,5");
        // A run of exactly two is listed, not ranged: `1-2` is no shorter
        // than `1,2` and reads as a range where there is none.
        assert_eq!(Months::parse("1,2,7").unwrap().to_expression(), "1,2,7");
    }

    #[test]
    fn every_canonical_expression_parses_back_to_itself() {
        for expr in ["1-10", "^6-8", "1,3,5", "1,2,7", "^12", "1-10,^5"] {
            let once = Months::parse(expr).unwrap();
            let twice = Months::parse(&once.to_expression()).unwrap();
            assert_eq!(once, twice, "{expr} did not survive a round trip");
        }
    }

    #[test]
    fn a_hand_edited_column_degrades_to_every_month_rather_than_failing() {
        // The ledger is what people read every day; a bad value in one rule's
        // column must not take a page render down with it.
        assert_eq!(Months::from_storage("nonsense"), None);
        assert_eq!(Months::from_storage(""), None);
        // ...and a partly-bad one keeps what it can rather than guessing.
        assert_eq!(
            Months::from_storage("1,2,99,3").unwrap().as_slice(),
            &[1, 2, 3]
        );
    }

    #[test]
    fn skipped_is_the_complement() {
        assert_eq!(Months::parse("1-10").unwrap().skipped(), vec![11, 12]);
        assert!(Months::parse("1-12").unwrap().skipped().is_empty());
    }
}
