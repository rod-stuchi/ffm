//! The one place the app reads the time.
//!
//! "Today" is the process's local date (see CLAUDE.md's Materialization), and
//! every date-shaped decision reads it: the due ring, `scheduled overdue`, the
//! unresolved badge, the month the ledger opens on, the materializer's current
//! month. Going through here rather than `chrono::Local::now()` at each call
//! site is what lets a dev server be run AS OF another date.
//!
//! `FFM_TODAY=2026-10-15` (which `just dev 2026-10-15` sets) moves the clock by
//! whole days, and only in a debug build. It is an OFFSET, not a frozen date:
//! the clock keeps running from there, and the UTC timestamps written into the
//! change log and the sessions move with it. Faking only the date would make
//! them disagree — a change made a minute ago would read as "from before
//! today", and `u` would ask to confirm every step.

use std::sync::OnceLock;

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};

static OFFSET: OnceLock<Duration> = OnceLock::new();

fn offset() -> Duration {
    OFFSET.get().copied().unwrap_or_else(Duration::zero)
}

/// Now, in UTC — what every stored timestamp is written from.
pub fn now() -> DateTime<Utc> {
    Utc::now() + offset()
}

/// Now, in the process's local zone.
pub fn local() -> DateTime<Local> {
    Local::now() + offset()
}

/// Today's local date.
pub fn today() -> NaiveDate {
    local().date_naive()
}

/// `now()` in the one format every stored timestamp uses.
pub fn stamp() -> String {
    now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// How far to move a clock that reads `real` so that it reads `target`.
pub fn offset_to(target: NaiveDate, real: NaiveDate) -> Duration {
    Duration::days((target - real).num_days())
}

/// The demo date asked for in `FFM_TODAY`, if any. A value that is not an ISO
/// date stops the process rather than quietly running on the real clock — a
/// demo that silently isn't one is the confusing outcome.
pub fn demo_from_env() -> Result<Option<NaiveDate>, String> {
    parse_demo(std::env::var("FFM_TODAY").ok().as_deref())
}

fn parse_demo(raw: Option<&str>) -> Result<Option<NaiveDate>, String> {
    match raw.map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| format!("✗ FFM_TODAY = {s:?} — expected a date like 2026-10-15")),
    }
}

/// Run the clock as of `date`. Debug builds only, so a release binary has no
/// way to be anywhere but today. Set once, at startup.
#[cfg(debug_assertions)]
pub fn pretend(date: NaiveDate) {
    let _ = OFFSET.set(offset_to(date, Local::now().date_naive()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn the_offset_is_whole_days_either_way() {
        assert_eq!(
            offset_to(d(2026, 10, 15), d(2026, 9, 29)),
            Duration::days(16)
        );
        assert_eq!(
            offset_to(d(2026, 9, 1), d(2026, 9, 29)),
            Duration::days(-28)
        );
        assert_eq!(offset_to(d(2026, 9, 29), d(2026, 9, 29)), Duration::zero());
    }

    #[test]
    fn ffm_today_is_an_iso_date_or_nothing() {
        assert_eq!(parse_demo(None), Ok(None));
        assert_eq!(parse_demo(Some("  ")), Ok(None));
        assert_eq!(parse_demo(Some("2026-10-15")), Ok(Some(d(2026, 10, 15))));
        assert!(parse_demo(Some("15/10/2026")).is_err());
        assert!(parse_demo(Some("2026-02-30")).is_err());
    }
}
