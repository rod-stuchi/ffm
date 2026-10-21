//! The geometry `:charts` is drawn with — pure arithmetic, no data, no copy.
//!
//! **A chart here is HTML boxes plus one SVG per line**, not one SVG for the
//! whole figure (see `design/ffm-charts-mockup.html`). A bar, a dot, a label
//! and an axis tick are elements placed by a percentage of the plot, so text
//! stays the size the stylesheet says at any width; a line or an area is a
//! path in a `0 0 100 100` box stretched over the plot with
//! `preserveAspectRatio="none"` and a non-scaling stroke. Every number this
//! module returns is a percentage of the plot, ready for a `style` attribute,
//! so the template does no arithmetic at all.
//!
//! **Colors are not here.** A mark carries a class; the class reads a
//! `var(--token)`, and a palette swap reaches it like anything else.

use crate::domain::money::{Cents, Currency};
use crate::ui::fmt;
use std::fmt::Write as _;

/// A value axis: bounds rounded out to "nice" ticks (1, 2, 2.5 or 5 × 10ⁿ),
/// always including zero when the data crosses or touches it.
#[derive(Clone, Debug, PartialEq)]
pub struct Scale {
    pub lo: i64,
    pub hi: i64,
    pub ticks: Vec<i64>,
}

impl Scale {
    /// `lo..=hi` (in cents) split into about `count` steps.
    pub fn nice(lo: i64, hi: i64, count: u32) -> Scale {
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        // A flat or empty series still gets an axis one currency unit tall,
        // rather than a division by zero.
        let span = (hi - lo).max(100) as f64;
        let raw = span / count.max(1) as f64;
        let p = 10f64.powf(raw.log10().floor());
        let step = [1.0, 2.0, 2.5, 5.0, 10.0]
            .iter()
            .map(|m| m * p)
            .find(|s| *s >= raw)
            .unwrap_or(10.0 * p)
            .max(1.0) as i64;
        let a = lo.div_euclid(step) * step;
        let b = -((-hi).div_euclid(step)) * step;
        let b = if b == a { a + step } else { b };
        let ticks = (0..)
            .map(|i| a + i * step)
            .take_while(|v| *v <= b)
            .collect();
        Scale {
            lo: a,
            hi: b,
            ticks,
        }
    }

    /// The top of the plot is 0%, its foot 100%.
    pub fn top(&self, v: i64) -> f64 {
        (self.hi - v) as f64 / (self.hi - self.lo).max(1) as f64 * 100.0
    }
}

/// `n` equal columns across the plot.
#[derive(Clone, Copy, Debug)]
pub struct Band {
    pub n: usize,
}

impl Band {
    pub fn width(self) -> f64 {
        100.0 / self.n.max(1) as f64
    }
    pub fn left(self, i: usize) -> f64 {
        i as f64 * self.width()
    }
    pub fn center(self, i: usize) -> f64 {
        (i as f64 + 0.5) * self.width()
    }
    /// A tooltip opens to the left past the middle, so it never runs off
    /// the plot's right edge.
    pub fn flip(self, i: usize) -> bool {
        self.center(i) > 60.0
    }
    /// Where column `i`'s tip may go: the whole plot, as a left and a width
    /// in percent of the column itself (the hover box is the tip's
    /// containing block), and the gap, in percent of the plot, from the
    /// plot's near edge to the column's far side. The gap shrinks before
    /// the tip leaves the plot, so a tip that fits beside its column sits
    /// there and one that does not — a wide column, a phone — slides back
    /// over it instead of scrolling the page sideways.
    pub fn room(self, i: usize) -> (f64, f64, f64) {
        let w = self.width();
        let gap = if self.flip(i) {
            100.0 - self.left(i)
        } else {
            self.left(i) + w
        };
        (-(i as f64) * 100.0, 100.0 / w * 100.0, gap)
    }
}

/// `12.35` — two decimals is far finer than a pixel at any width a chart is
/// drawn at, and keeps the markup short.
pub fn pct(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    format!("{:.2}", (v * 100.0).round() / 100.0)
}

/// A polyline through `pts` (x %, y %).
pub fn line(pts: &[(f64, f64)]) -> String {
    let mut d = String::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        let _ = write!(
            d,
            "{}{} {}",
            if i == 0 { 'M' } else { 'L' },
            pct(*x),
            pct(*y)
        );
    }
    d
}

/// The area under `pts`, closed along the horizontal line at `base` (y %).
pub fn area(pts: &[(f64, f64)], base: f64) -> String {
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else {
        return String::new();
    };
    let mut d = format!("M{} {}", pct(first.0), pct(base));
    for (x, y) in pts {
        let _ = write!(d, "L{} {}", pct(*x), pct(*y));
    }
    let _ = write!(d, "L{} {}Z", pct(last.0), pct(base));
    d
}

/// The band between two lines over the same xs: `a` forward, `b` back.
pub fn between(a: &[(f64, f64)], b: &[(f64, f64)]) -> String {
    if a.is_empty() || b.is_empty() {
        return String::new();
    }
    let mut d = line(a);
    for (x, y) in b.iter().rev() {
        let _ = write!(d, "L{} {}", pct(*x), pct(*y));
    }
    d.push('Z');
    d
}

/// An axis tick: `20k`, `−10k`, `0` — `money_compact` without the `+` a
/// picker cell needs, since the axis itself says which side of zero it is.
pub fn tick(v: i64, currency: Currency) -> String {
    let s = fmt::money_compact(Cents(v), currency);
    s.strip_prefix('+').map(str::to_string).unwrap_or(s)
}

/// `a` against `b` in whole percent, `None` when there is nothing to compare
/// against.
pub fn change_pct(now: i64, before: i64) -> Option<i64> {
    if before == 0 {
        return None;
    }
    Some(((now - before) as f64 / before.abs() as f64 * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_round_and_include_zero() {
        let s = Scale::nice(-2_275_000, 2_480_000, 6);
        assert_eq!(
            s.ticks,
            vec![
                -3_000_000, -2_000_000, -1_000_000, 0, 1_000_000, 2_000_000, 3_000_000
            ]
        );
        assert_eq!(s.top(s.hi), 0.0);
        assert_eq!(s.top(s.lo), 100.0);
        assert_eq!(s.top(0), 50.0);
    }

    #[test]
    fn a_positive_series_starts_at_zero_when_asked() {
        let s = Scale::nice(0, 6_372_800, 4);
        assert_eq!(s.lo, 0);
        assert_eq!(s.ticks, vec![0, 2_000_000, 4_000_000, 6_000_000, 8_000_000]);
    }

    #[test]
    fn steps_of_two_and_a_half() {
        let s = Scale::nice(0, 9_000, 4);
        assert_eq!(s.ticks, vec![0, 2_500, 5_000, 7_500, 10_000]);
    }

    #[test]
    fn a_flat_series_still_has_an_axis() {
        let s = Scale::nice(0, 0, 4);
        assert!(s.hi > s.lo);
        let s = Scale::nice(5_000, 5_000, 4);
        assert!(s.lo <= 5_000 && s.hi > 5_000);
    }

    #[test]
    fn bands_and_flip() {
        let b = Band { n: 4 };
        assert_eq!(b.width(), 25.0);
        assert_eq!(b.left(1), 25.0);
        assert_eq!(b.center(3), 87.5);
        assert!(!b.flip(1));
        assert!(b.flip(3));
        // Column 1 of 4: the plot starts one column to its left and is four
        // columns wide; its tip opens past its right side, at 50%.
        assert_eq!(b.room(1), (-100.0, 400.0, 50.0));
        // Column 3 flips: its tip ends before its left side, 25% from the right.
        assert_eq!(b.room(3), (-300.0, 400.0, 25.0));
        // A lone column is the whole plot.
        assert_eq!(Band { n: 1 }.room(0), (0.0, 100.0, 100.0));
    }

    #[test]
    fn paths() {
        let pts = [(10.0, 50.0), (30.0, 20.0)];
        assert_eq!(line(&pts), "M10.00 50.00L30.00 20.00");
        assert_eq!(
            area(&pts, 100.0),
            "M10.00 100.00L10.00 50.00L30.00 20.00L30.00 100.00Z"
        );
        assert_eq!(
            between(&pts, &[(10.0, 60.0), (30.0, 40.0)]),
            "M10.00 50.00L30.00 20.00L30.00 40.00L10.00 60.00Z"
        );
        assert_eq!(line(&[]), "");
        assert_eq!(area(&[], 0.0), "");
    }

    #[test]
    fn ticks_read_without_a_plus() {
        assert_eq!(tick(2_000_000, Currency::Brl), "20k");
        assert_eq!(tick(-1_000_000, Currency::Brl), "−10k");
        assert_eq!(tick(0, Currency::Brl), "0");
        assert_eq!(tick(250_000, Currency::Brl), "2,5k");
        assert_eq!(tick(250_000, Currency::Usd), "2.5k");
    }

    #[test]
    fn change_in_percent() {
        assert_eq!(change_pct(110, 100), Some(10));
        assert_eq!(change_pct(90, 100), Some(-10));
        assert_eq!(change_pct(5, 0), None);
        assert_eq!(change_pct(-50, -100), Some(50));
    }
}
