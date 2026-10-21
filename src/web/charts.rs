//! `:charts` / `gc` — where the money went, drawn. A full-viewport viewer in
//! the `P` carousel's frame, read-only, three tabs:
//!
//! * **Month** — the month on screen: its totals, spend by bucket against last
//!   month, who moved the money, fixed against one-off, what is still to pay.
//!   Read through `member_panes`, the very loop the ledger bar sums, so every
//!   figure here is the one the screen behind it shows.
//! * **Year** — a range of months (January to now by default, at most three
//!   years): cash flow, what was kept, whether income is growing, what moved,
//!   and every bucket as a small chart of its own.
//! * **Years** — admin only: the same months of every year side by side, so a
//!   part-year is never set against whole ones.
//!
//! Year and Years read `db::charts::rows`, one query for the range; see there
//! for why, and for how it is held to the pane's numbers.
//!
//! **Rows picked with `x` narrow every chart to them**, followed across months
//! the way the cursor follows a line: by rule, else by exact title, in the
//! same member's ledger. `s` switches between them and everything.
//!
//! **Everything is computed and formatted here.** The page sends ids and
//! periods; the stage that comes back is finished HTML, every position a
//! percentage (`ui::chart`), every color a class reading a token. The keys
//! inside the viewer only press the links the stage carries (`data-ck`), so
//! no decision about what to show lives in Alpine.

use crate::db::charts::{self, ChartRow};
use crate::db::ledger;
use crate::domain::money::{Cents, Currency};
use crate::domain::period::Period;
use crate::state::AppState;
use crate::ui::chart::{self, Band, Scale};
use crate::ui::fmt;
use crate::ui::i18n::Catalog;
use crate::ui::row::{LedgerRow, Tally};
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

/// The longest range the Year tab draws. Past three years a month is a few
/// pixels wide, and the Years tab is the view for that.
const MAX_MONTHS: i32 = 36;

/// How many years the Years tab opens on; its first-year field reaches the
/// rest.
const DEFAULT_YEARS: i32 = 10;

/// How many earlier years the cumulative cards draw with `lines=5`.
const MANY_OLD: usize = 4;

/// An earlier year's line, legend swatch and tip key, last year first: the
/// same `--ink-3`, fainter one year further back.
const OLD_LINE: [&str; MANY_OLD] = ["l-old", "l-old o2", "l-old o3", "l-old o4"];
const OLD_LEGEND: [&str; MANY_OLD] = ["lg-old", "lg-old o2", "lg-old o3", "lg-old o4"];
const OLD_KEY: [&str; MANY_OLD] = ["k-old", "k-old o2", "k-old o3", "k-old o4"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Month,
    Year,
    Years,
}

impl Tab {
    fn parse(s: &str) -> Tab {
        match s {
            "year" => Tab::Year,
            "years" => Tab::Years,
            _ => Tab::Month,
        }
    }
    fn code(self) -> &'static str {
        match self {
            Tab::Month => "month",
            Tab::Year => "year",
            Tab::Years => "years",
        }
    }
}

#[derive(Deserialize, Default)]
pub struct Params {
    #[serde(default)]
    tab: String,
    /// The Month tab's month, and the year the Year tab opens on.
    #[serde(default)]
    period: String,
    /// Empty for the household.
    #[serde(default)]
    member: String,
    /// The rows picked with `x`, from the focused pane of month `selp`.
    #[serde(default)]
    ids: String,
    #[serde(default)]
    selp: String,
    /// `0` shows everything though rows are selected (`s`).
    #[serde(default)]
    sel: String,
    #[serde(default)]
    from: String,
    #[serde(default)]
    to: String,
    /// The Years tab's first year.
    #[serde(default)]
    since: String,
    /// `5` draws the cumulative cards with this year and four before it;
    /// absent, this year against last (`v`).
    #[serde(default)]
    lines: String,
}

/// Everything a link inside the stage has to carry to land on the same view
/// with one thing changed.
#[derive(Clone)]
struct Q {
    tab: Tab,
    period: Period,
    member: Option<i64>,
    ids: String,
    selp: String,
    sel: bool,
    from: Period,
    to: Period,
    since: Option<i32>,
    many: bool,
}

impl Q {
    fn url(&self) -> String {
        let mut u = format!(
            "/charts?tab={}&period={}&from={}&to={}",
            self.tab.code(),
            self.period,
            self.from,
            self.to
        );
        if let Some(m) = self.member {
            u.push_str(&format!("&member={m}"));
        }
        if !self.ids.is_empty() {
            u.push_str(&format!("&selp={}&ids={}", self.selp, self.ids));
            if !self.sel {
                u.push_str("&sel=0");
            }
        }
        if let Some(y) = self.since {
            u.push_str(&format!("&since={y}"));
        }
        if self.many {
            u.push_str("&lines=5");
        }
        u
    }
    fn with(&self, f: impl FnOnce(&mut Q)) -> String {
        let mut q = self.clone();
        f(&mut q);
        q.url()
    }
}

// ---------------------------------------------------------------------------
// What the templates read
// ---------------------------------------------------------------------------

pub struct Link {
    pub label: String,
    pub url: String,
    pub on: bool,
    /// The key that presses it (`data-ck`), if any.
    pub key: &'static str,
}

/// A change against something earlier, colored by whether it is good news:
/// more money in is, more money out is not.
pub struct Delta {
    pub arrow: &'static str,
    pub text: String,
    pub class: &'static str,
}

pub struct TipRow {
    pub label: String,
    pub value: String,
    /// The key's color class: `k-in`, `k-out`, `k-net`, `k-neg`, `k-sel`, `k-old`.
    pub key: &'static str,
}

pub struct Tip {
    pub head: String,
    pub rows: Vec<TipRow>,
}

pub struct YTick {
    pub top: String,
    pub label: String,
    pub zero: bool,
}

pub struct XTick {
    pub left: String,
    pub label: String,
    pub cur: bool,
    /// Every other label past eight months, which a phone leaves out: twelve
    /// "Jan"s at the chart's type size do not fit across one.
    pub minor: bool,
}

/// A month's (or year's) pair of bars: IN up from zero, OUT down from it.
pub struct Col {
    pub left: String,
    pub width: String,
    pub in_top: String,
    pub in_h: String,
    pub in_est: bool,
    pub out_top: String,
    pub out_h: String,
}

pub struct PathD {
    pub class: &'static str,
    pub d: String,
}

pub struct Dot {
    pub left: String,
    pub top: String,
    pub class: &'static str,
}

pub struct Label {
    pub left: String,
    pub top: String,
    pub class: &'static str,
    pub text: String,
    /// Which way it sits off its point: `above`, `below`, or `end`.
    pub at: &'static str,
}

pub struct Hit {
    pub left: String,
    pub width: String,
    pub flip: bool,
    pub tip: Tip,
}

/// One plotted chart: axes, bars, lines, dots, labels and the hover boxes.
#[derive(Default)]
pub struct Plot {
    pub h: u32,
    pub yt: Vec<YTick>,
    pub xt: Vec<XTick>,
    pub cols: Vec<Col>,
    pub paths: Vec<PathD>,
    pub dots: Vec<Dot>,
    pub labels: Vec<Label>,
    pub hits: Vec<Hit>,
    /// Room on the right for a line's end label.
    pub pad: bool,
}

pub struct TRow {
    pub name: String,
    pub cells: Vec<String>,
}

/// Every chart's twin: the same numbers as a table, behind a `<details>`.
pub struct Table {
    pub head: Vec<String>,
    pub rows: Vec<TRow>,
}

pub struct Legend {
    pub class: &'static str,
    pub label: String,
}

pub struct PlotCard {
    pub wide: bool,
    pub title: String,
    pub sub: String,
    pub delta: Option<Delta>,
    pub legend: Vec<Legend>,
    pub plot: Plot,
    pub table: Table,
}

pub struct Tile {
    pub label: String,
    pub value: String,
    pub neg: bool,
    pub delta: Option<Delta>,
    pub note: String,
    pub meter: Option<String>,
}

/// A horizontal bar: a bucket's spend, or a bill still to pay. `f` is the
/// bar's share of the track; `at` where its value label starts, past the bar
/// or last month's tick, whichever is further.
pub struct HBar {
    pub name: String,
    pub unc: bool,
    pub f: String,
    pub at: String,
    pub prev: Option<String>,
    pub value: String,
    pub delta: Option<Delta>,
    pub note: String,
    /// `flat`, or `bad` for a bill past its due date.
    pub note_class: &'static str,
    pub muted: bool,
    pub tip: Option<Tip>,
}

pub struct MirRow {
    pub name: String,
    pub out_f: String,
    pub out_v: String,
    pub in_f: String,
    pub in_v: String,
    pub in_est: bool,
}

pub struct Stack {
    pub rec_f: String,
    pub rec_v: String,
    pub rec_pct: String,
    pub one_f: String,
    pub one_v: String,
    pub one_pct: String,
}

pub struct MoveRow {
    pub name: String,
    pub up: bool,
    pub f: String,
    pub amount: String,
    pub pct: String,
}

pub struct Spark {
    pub name: String,
    pub unc: bool,
    pub sel: bool,
    pub total: String,
    pub area: String,
    pub line: String,
    pub dot_left: String,
    pub dot_top: String,
    pub delta: Option<Delta>,
    pub first: String,
    pub last: String,
    pub hits: Vec<Hit>,
}

pub struct MonthView {
    pub tiles: Vec<Tile>,
    pub buckets: Vec<HBar>,
    pub buckets_sub: String,
    pub prev_label: String,
    pub buckets_table: Table,
    pub members: Vec<MirRow>,
    pub members_sub: String,
    pub members_table: Table,
    pub stack: Option<Stack>,
    pub to_pay: Vec<HBar>,
}

pub struct YearView {
    pub flow: Option<PlotCard>,
    pub kept: Option<PlotCard>,
    pub trend: Option<PlotCard>,
    pub moved: Vec<MoveRow>,
    /// Fewer than six months: nothing to set three against three.
    pub moved_short: bool,
    pub moved_sub: String,
    pub moved_table: Table,
    pub sparks: Vec<Spark>,
    pub sparks_sub: String,
    /// The selection: the rows together, then one small chart each.
    pub together: Option<PlotCard>,
    pub each: Vec<Spark>,
}

pub struct YearRow {
    pub year: String,
    pub cur: bool,
    pub ins: String,
    pub outs: String,
    pub net: String,
    pub neg: bool,
    pub kept: String,
    pub vs: Option<Delta>,
    pub whole: String,
}

pub struct YearsView {
    pub flow: PlotCard,
    pub spent: PlotCard,
    pub earned: PlotCard,
    pub rows: Vec<YearRow>,
    pub table_sub: String,
}

#[derive(Template)]
#[template(path = "partials/charts.html")]
pub struct ChartsStage {
    pub t: Catalog,
    pub tabs: Vec<Link>,
    pub chips: Vec<Link>,
    /// The hidden links `H` and `L` press: the member chip before and after.
    pub chip_prev: String,
    pub chip_next: String,
    /// The selection chip, and what `s` presses.
    pub sel_label: Option<String>,
    pub sel_url: String,
    pub tab: &'static str,
    pub period_label: String,
    pub prev_url: Option<String>,
    pub next_url: Option<String>,
    /// The Year tab's from/to fields: every month there is something to
    /// show for, newest first. A `<select>`, not `<input type="month">`,
    /// which Safari on a Mac renders as a bare text box.
    pub from: String,
    pub to: String,
    pub options: Vec<(String, String)>,
    /// The Years tab's first-year field, and every year it can name,
    /// newest first.
    pub since: String,
    pub since_options: Vec<String>,
    /// The cumulative cards' two modes, `v` pressing the one not on; empty
    /// while fewer than three years are in range, where both draw the same.
    pub lines: Vec<Link>,
    /// What the range form carries besides its own fields.
    pub hidden: Vec<(&'static str, String)>,
    pub month: Option<MonthView>,
    pub year: Option<YearView>,
    pub years: Option<YearsView>,
    pub refused: bool,
    pub empty: bool,
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

fn money(c: i64, currency: Currency) -> String {
    fmt::money(Cents(c), currency)
}

fn est_money(c: i64, currency: Currency, est: bool) -> String {
    crate::web::ledger::money(Cents(c), currency, est)
}

/// `+R$ 8.268,00` / `−R$ 1.350,00` — a NET, which has a side. A projection
/// leads with its tilde, `~+R$ …`, as the month picker writes `~+13k`.
fn signed(c: i64, currency: Currency, est: bool) -> String {
    format!(
        "{}{}{}",
        if est { "~" } else { "" },
        if c < 0 { "−" } else { "+" },
        money(c.abs(), currency)
    )
}

fn pct_text(p: i64) -> String {
    match p.signum() {
        1 => format!("+{p}%"),
        -1 => format!("−{}%", -p),
        _ => "0%".to_string(),
    }
}

/// `up_good`: whether a rise is good news (money in) or bad (money out).
fn delta(t: &Catalog, now: i64, before: i64, up_good: bool) -> Option<Delta> {
    let p = chart::change_pct(now, before)?;
    if p == 0 {
        return Some(Delta {
            arrow: "",
            text: t.charts_flat().to_string(),
            class: "flat",
        });
    }
    let good = (p > 0) == up_good;
    Some(Delta {
        arrow: if p > 0 { "▲" } else { "▼" },
        text: pct_text(p),
        class: if good { "good" } else { "bad" },
    })
}

fn month_names(t: &Catalog) -> Vec<String> {
    t.months_short()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn mname(names: &[String], p: Period) -> String {
    names
        .get(p.month() as usize - 1)
        .cloned()
        .unwrap_or_else(|| fmt::period(p))
}

/// `Jan–Oct`, or `01/2025 – 10/2026` when the range crosses a year.
fn range_label(names: &[String], from: Period, to: Period) -> String {
    if from.year() == to.year() {
        if from == to {
            mname(names, from)
        } else {
            format!("{}–{}", mname(names, from), mname(names, to))
        }
    } else {
        format!("{} – {}", fmt::period(from), fmt::period(to))
    }
}

/// What a month column is called on an axis: `Oct`, and the year too
/// wherever the range crosses one.
fn axis_label(names: &[String], p: Period, crosses: bool) -> String {
    if crosses {
        format!("{} {:02}", mname(names, p), p.year() % 100)
    } else {
        mname(names, p)
    }
}

fn bucket_name(b: Option<&str>) -> Option<String> {
    b.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn months(from: Period, to: Period) -> Vec<Period> {
    let mut out = Vec::new();
    let mut p = from;
    while p <= to {
        out.push(p);
        p = p.succ();
    }
    out
}

fn y_ticks(s: &Scale, currency: Currency) -> Vec<YTick> {
    s.ticks
        .iter()
        .map(|v| YTick {
            top: chart::pct(s.top(*v)),
            label: chart::tick(*v, currency),
            zero: *v == 0,
        })
        .collect()
}

/// A label under every column up to a year; past that, no more than eight —
/// every 2nd, 3rd, 4th or 6th month — counted back from the last, so the
/// latest month is always named, and the current month always is.
/// Thirty-six "Nov 23"s side by side ran into each other at any width.
fn x_ticks(labels: &[String], cur: Option<usize>) -> Vec<XTick> {
    let n = labels.len();
    let b = Band { n };
    let step = if n <= 12 {
        1
    } else {
        [2, 3, 4, 6, 12]
            .into_iter()
            .find(|s| n.div_ceil(*s) <= 8)
            .unwrap_or(12)
    };
    labels
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let is_cur = cur == Some(i);
            let back = n - 1 - i;
            let on_grid = back.is_multiple_of(step);
            // The current month off the grid pushes aside its neighbors.
            let crowds = cur.is_some_and(|c| c != i && c.abs_diff(i) < step);
            (is_cur || (on_grid && !crowds)).then(|| XTick {
                left: chart::pct(b.center(i)),
                label: l.clone(),
                cur: is_cur,
                minor: !is_cur && n > 8 && !(back / step).is_multiple_of(2),
            })
        })
        .collect()
}

fn hit(b: Band, i: usize, tip: Tip) -> Hit {
    Hit {
        left: chart::pct(b.left(i)),
        width: chart::pct(b.width()),
        flip: b.flip(i),
        tip,
    }
}

// ---------------------------------------------------------------------------
// Shared builders
// ---------------------------------------------------------------------------

/// IN up, OUT down, NET a line through dots — the Year tab's cash flow and the
/// Years tab's year-against-year.
#[allow(clippy::too_many_arguments)]
fn flow_card(
    t: &Catalog,
    currency: Currency,
    title: String,
    sub: String,
    labels: &[String],
    heads: &[String],
    ins: &[i64],
    outs: &[i64],
    est: &[bool],
    est_bars: bool,
    cur: Option<usize>,
) -> PlotCard {
    let nets: Vec<i64> = ins.iter().zip(outs).map(|(i, o)| i - o).collect();
    let s = Scale::nice(
        -outs.iter().copied().max().unwrap_or(0),
        ins.iter().copied().max().unwrap_or(0),
        6,
    );
    let b = Band { n: labels.len() };
    let z = s.top(0);
    let mut plot = Plot {
        h: 240,
        yt: y_ticks(&s, currency),
        xt: x_ticks(labels, cur),
        ..Plot::default()
    };
    let mut pts = Vec::new();
    for i in 0..labels.len() {
        let (top_in, top_out) = (s.top(ins[i]), s.top(-outs[i]));
        plot.cols.push(Col {
            left: chart::pct(b.left(i)),
            width: chart::pct(b.width()),
            in_top: chart::pct(top_in),
            in_h: chart::pct(z - top_in),
            in_est: est_bars && est[i],
            out_top: chart::pct(z),
            out_h: chart::pct(top_out - z),
        });
        let y = s.top(nets[i]);
        pts.push((b.center(i), y));
        plot.dots.push(Dot {
            left: chart::pct(b.center(i)),
            top: chart::pct(y),
            class: if nets[i] < 0 { "d-neg" } else { "" },
        });
        plot.hits.push(hit(
            b,
            i,
            Tip {
                head: heads[i].clone(),
                rows: vec![
                    TipRow {
                        label: t.charts_in().to_string(),
                        value: est_money(ins[i], currency, est[i]),
                        key: "k-in",
                    },
                    TipRow {
                        label: t.charts_out().to_string(),
                        value: money(outs[i], currency),
                        key: "k-out",
                    },
                    TipRow {
                        label: t.charts_net().to_string(),
                        value: signed(nets[i], currency, est[i]),
                        key: if nets[i] < 0 { "k-neg" } else { "k-net" },
                    },
                ],
            },
        ));
    }
    plot.paths.push(PathD {
        class: "l-net",
        d: chart::line(&pts),
    });
    PlotCard {
        wide: true,
        title,
        sub,
        delta: None,
        legend: vec![
            Legend {
                class: "lg-in",
                label: t.charts_in().to_string(),
            },
            Legend {
                class: "lg-out",
                label: t.charts_out().to_string(),
            },
            Legend {
                class: "lg-net",
                label: t.charts_net().to_string(),
            },
        ],
        plot,
        table: Table {
            head: vec![
                String::new(),
                t.charts_in().to_string(),
                t.charts_out().to_string(),
                t.charts_net().to_string(),
            ],
            rows: (0..labels.len())
                .map(|i| TRow {
                    name: heads[i].clone(),
                    cells: vec![
                        est_money(ins[i], currency, est[i]),
                        money(outs[i], currency),
                        signed(nets[i], currency, est[i]),
                    ],
                })
                .collect(),
        },
    }
}

/// One series as a small chart of its own, on a scale shared with its
/// neighbors so heights compare across cards.
#[allow(clippy::too_many_arguments)]
fn spark(
    t: &Catalog,
    currency: Currency,
    name: String,
    unc: bool,
    sel: bool,
    values: &[i64],
    max: i64,
    heads: &[String],
    first: String,
    last: String,
) -> Spark {
    let n = values.len();
    let b = Band { n };
    let y = |v: i64| (1.0 - v as f64 / max.max(1) as f64) * 100.0;
    let pts: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| (b.center(i), y(*v)))
        .collect();
    let (dot_left, dot_top) = pts
        .last()
        .map(|(x, y)| (chart::pct(*x), chart::pct(*y)))
        .unwrap_or_default();
    let delta = match (values.first(), values.last()) {
        (Some(a), Some(z)) if n > 1 => delta(t, *z, *a, false),
        _ => None,
    };
    Spark {
        name: name.clone(),
        unc,
        sel,
        total: money(values.iter().sum(), currency),
        area: chart::area(&pts, 100.0),
        line: chart::line(&pts),
        dot_left,
        dot_top,
        delta,
        first,
        last,
        hits: values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                hit(
                    b,
                    i,
                    Tip {
                        head: heads[i].clone(),
                        rows: vec![TipRow {
                            label: name.clone(),
                            value: money(*v, currency),
                            key: if sel { "k-sel" } else { "k-out" },
                        }],
                    },
                )
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// The selection: lines followed across months
// ---------------------------------------------------------------------------

/// "The same line" in another month: the same rule in the same member's
/// ledger, else the same exact title there — the order the cursor follows a
/// row across months, and the MoM delta's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Line {
    Rule(i64, i64),
    Title(i64, String),
}

impl Line {
    pub fn of(member: i64, rule: Option<i64>, title: &str) -> Line {
        match rule {
            Some(r) => Line::Rule(member, r),
            None => Line::Title(member, title.to_string()),
        }
    }
}

/// The selected ids, resolved in the month they were picked in to the lines
/// they are — only rows this viewer can see, in ledger order, each line once.
async fn selected_lines(
    st: &AppState,
    auth: &Auth,
    selp: Option<Period>,
    ids: &HashSet<i64>,
) -> Vec<(Line, String)> {
    let Some(p) = selp.filter(|_| !ids.is_empty()) else {
        return Vec::new();
    };
    let rows = charts::rows(&st.pool, None, auth.viewer, p, p)
        .await
        .unwrap_or_default();
    let mut out: Vec<(Line, String)> = Vec::new();
    for r in rows.iter().filter(|r| ids.contains(&r.id)) {
        let line = Line::of(r.member_id, r.rule_id, &r.title);
        if !out.iter().any(|(l, _)| *l == line) {
            out.push((line, r.title.clone()));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The handler
// ---------------------------------------------------------------------------

/// `GET /charts` — the whole stage: tabs, member chips, and the charts.
pub async fn view(State(st): State<AppState>, auth: Auth, Query(p): Query<Params>) -> Response {
    let t = auth.catalog();
    let today = crate::clock::today();
    let current = Period::of(today);
    let currency = ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();
    let names = month_names(&t);

    let tab = Tab::parse(&p.tab);
    let period = Period::parse(&p.period).unwrap_or(current).min(current);
    let members = ledger::members(&st.pool).await.unwrap_or_default();
    let member = p
        .member
        .parse::<i64>()
        .ok()
        .filter(|id| members.iter().any(|m| m.id == *id));
    let ids: HashSet<i64> = p
        .ids
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let ids_s = {
        let mut v: Vec<i64> = ids.iter().copied().collect();
        v.sort_unstable();
        v.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
    };
    let selp = Period::parse(&p.selp);

    // The Year tab's range: January of the year on screen to now, unless the
    // fields say otherwise — never past this month, never longer than three
    // years, and never backwards.
    let year_end = Period::new(period.year(), 12)
        .unwrap_or(period)
        .min(current);
    let mut to = Period::parse(&p.to).unwrap_or(year_end).min(current);
    let mut from =
        Period::parse(&p.from).unwrap_or_else(|| Period::new(to.year(), 1).unwrap_or(to));
    if from > to {
        std::mem::swap(&mut from, &mut to);
    }
    if from.months_until(to) >= MAX_MONTHS {
        from = to.pred_n(MAX_MONTHS as u32 - 1);
    }

    let lines = selected_lines(&st, &auth, selp, &ids).await;
    let sel_on = !lines.is_empty() && p.sel != "0";

    let q = Q {
        tab,
        period,
        member,
        ids: if lines.is_empty() {
            String::new()
        } else {
            ids_s
        },
        selp: selp.map(|p| p.to_string()).unwrap_or_default(),
        sel: sel_on,
        from,
        to,
        since: p.since.parse().ok(),
        many: p.lines == "5",
    };

    let is_admin = auth.viewer.is_admin();
    let mut tabs = vec![
        Link {
            label: t.charts_tab_month().to_string(),
            url: q.with(|q| q.tab = Tab::Month),
            on: tab == Tab::Month,
            key: "1",
        },
        Link {
            label: t.charts_tab_year().to_string(),
            url: q.with(|q| q.tab = Tab::Year),
            on: tab == Tab::Year,
            key: "2",
        },
    ];
    // Admin-only, and absent rather than disabled under preview — the
    // Household button's rule.
    if is_admin {
        tabs.push(Link {
            label: t.charts_tab_years().to_string(),
            url: q.with(|q| q.tab = Tab::Years),
            on: tab == Tab::Years,
            key: "3",
        });
    }

    let mut ring: Vec<Option<i64>> = vec![None];
    ring.extend(members.iter().map(|m| Some(m.id)));
    let at = ring.iter().position(|m| *m == member).unwrap_or(0);
    let step = |d: isize| {
        let n = ring.len() as isize;
        let next = ring[((at as isize + d).rem_euclid(n)) as usize];
        q.with(|q| q.member = next)
    };
    let chips = ring
        .iter()
        .map(|m| Link {
            label: match m {
                None => t.charts_household().to_string(),
                Some(id) => members
                    .iter()
                    .find(|x| x.id == *id)
                    .map(|x| fmt::name(&x.name))
                    .unwrap_or_default(),
            },
            url: q.with(|q| q.member = *m),
            on: *m == member,
            key: "",
        })
        .collect();

    let sel_label = (!lines.is_empty()).then(|| {
        if sel_on {
            t.charts_selected(lines.len() as i64)
        } else {
            t.charts_selected_off(lines.len() as i64)
        }
    });

    let (prev_url, next_url, period_label) = match tab {
        Tab::Month => (
            Some(q.with(|q| q.period = period.pred())),
            (period < current).then(|| q.with(|q| q.period = period.succ())),
            fmt::period(period),
        ),
        Tab::Year => {
            let earlier = q.with(|q| {
                q.from = from.pred_n(12);
                q.to = to.pred_n(12);
                q.period = to.pred_n(12);
            });
            let later = (to < current).then(|| {
                q.with(|q| {
                    q.to = to.succ_n(12).min(current);
                    q.from = q.to.pred_n(from.months_until(to) as u32);
                    q.period = q.to;
                })
            });
            (Some(earlier), later, range_label(&names, from, to))
        }
        Tab::Years => (None, None, String::new()),
    };

    let mut stage = ChartsStage {
        tabs,
        chips,
        chip_prev: step(-1),
        chip_next: step(1),
        sel_label,
        sel_url: q.with(|q| q.sel = !q.sel),
        tab: tab.code(),
        period_label,
        prev_url,
        next_url,
        from: from.to_string(),
        to: to.to_string(),
        options: {
            let first = charts::first_period(&st.pool, auth.viewer)
                .await
                .ok()
                .flatten()
                .unwrap_or(current)
                .min(from);
            let mut v: Vec<(String, String)> = months(first, current)
                .into_iter()
                .map(|p| (p.to_string(), fmt::period(p)))
                .collect();
            v.reverse();
            v
        },
        since: String::new(),
        since_options: Vec::new(),
        lines: Vec::new(),
        hidden: {
            let mut h = vec![
                ("tab", tab.code().to_string()),
                ("period", period.to_string()),
            ];
            if let Some(m) = member {
                h.push(("member", m.to_string()));
            }
            if !q.ids.is_empty() {
                h.push(("selp", q.selp.clone()));
                h.push(("ids", q.ids.clone()));
                if !sel_on {
                    h.push(("sel", "0".to_string()));
                }
            }
            // Each tab's form carries the other's fields, so a change on one
            // does not reset the other.
            if tab == Tab::Years {
                h.push(("from", from.to_string()));
                h.push(("to", to.to_string()));
            } else if let Some(y) = q.since {
                h.push(("since", y.to_string()));
            }
            if q.many {
                h.push(("lines", "5".to_string()));
            }
            h
        },
        month: None,
        year: None,
        years: None,
        refused: false,
        empty: false,
        t,
    };
    let t = &stage.t;
    let keep: Option<HashSet<Line>> =
        sel_on.then(|| lines.iter().map(|(l, _)| l.clone()).collect());

    match tab {
        Tab::Month => {
            let ctx = crate::web::ledger::session_ctx(&st, &auth).await;
            let now = crate::web::ledger::member_panes(&st, &auth, period, ctx).await;
            let before = crate::web::ledger::member_panes(&st, &auth, period.pred(), ctx).await;
            let pick = |panes: Vec<ledger::Pane>| -> Vec<(ledger::Member, Vec<LedgerRow>)> {
                panes
                    .into_iter()
                    .filter(|p| member.is_none_or(|m| p.member.id == m))
                    .map(|p| {
                        let id = p.member.id;
                        let rows = p
                            .rows
                            .into_iter()
                            .filter(|r| {
                                keep.as_ref()
                                    .is_none_or(|k| k.contains(&Line::of(id, r.rule_id, &r.title)))
                            })
                            .collect();
                        (p.member, rows)
                    })
                    .collect()
            };
            let (now, before) = (pick(now), pick(before));
            stage.empty = now.iter().all(|(_, r)| r.is_empty());
            stage.month = Some(month_view(t, currency, period, today, &now, &before));
        }
        Tab::Year => {
            let rows = charts::rows(&st.pool, Some(&st.cfg.receipts_root), auth.viewer, from, to)
                .await
                .unwrap_or_default();
            let rows: Vec<ChartRow> = rows
                .into_iter()
                .filter(|r| member.is_none_or(|m| r.member_id == m))
                .collect();
            stage.empty = rows.is_empty();
            stage.year = Some(year_view(
                t,
                currency,
                &names,
                from,
                to,
                current,
                &rows,
                keep.as_ref().map(|_| lines.as_slice()),
            ));
        }
        Tab::Years if !is_admin => stage.refused = true,
        Tab::Years => {
            let first = charts::first_period(&st.pool, auth.viewer)
                .await
                .ok()
                .flatten()
                .unwrap_or(current)
                .min(current)
                .year();
            let since = years_since(q.since, first, current.year());
            let start = Period::new(since, 1).unwrap_or(current);
            stage.since = since.to_string();
            stage.since_options = (first..=current.year())
                .rev()
                .map(|y| y.to_string())
                .collect();
            if current.year() - since >= 2 {
                stage.lines = [
                    (false, t.charts_lines_last()),
                    (true, t.charts_lines_five()),
                ]
                .into_iter()
                .map(|(many, label)| Link {
                    label: label.to_string(),
                    url: q.with(|q| q.many = many),
                    on: q.many == many,
                    key: if q.many == many { "" } else { "v" },
                })
                .collect();
            }
            let rows = charts::rows(
                &st.pool,
                Some(&st.cfg.receipts_root),
                auth.viewer,
                start,
                current,
            )
            .await
            .unwrap_or_default();
            let rows: Vec<ChartRow> = rows
                .into_iter()
                .filter(|r| member.is_none_or(|m| r.member_id == m))
                .collect();
            stage.empty = rows.is_empty();
            stage.years = Some(years_view(
                t, currency, &names, current, &rows, since, q.many,
            ));
        }
    }
    Html(stage).into_response()
}

// ---------------------------------------------------------------------------
// Month
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
struct Sums {
    ins: i64,
    outs: i64,
    est: bool,
    estimates: i64,
}

fn sums(rows: &[&LedgerRow]) -> Sums {
    let mut s = Sums::default();
    for r in rows {
        let Some(c) = r.effective_cents() else {
            continue;
        };
        if r.kind.is_income() {
            s.ins += c.get();
            if r.is_estimated() {
                s.est = true;
                s.estimates += 1;
            }
        } else {
            s.outs += c.get();
        }
    }
    s
}

/// Spend per bucket, in first-seen order; `None` is Uncategorized.
fn by_bucket<'a>(rows: impl Iterator<Item = &'a LedgerRow>) -> Vec<(Option<String>, i64)> {
    let mut out: Vec<(Option<String>, i64)> = Vec::new();
    for r in rows.filter(|r| !r.kind.is_income()) {
        let Some(c) = r.effective_cents() else {
            continue;
        };
        let name = bucket_name(r.bucket.as_deref());
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some((_, v)) => *v += c.get(),
            None => out.push((name, c.get())),
        }
    }
    out
}

fn month_view(
    t: &Catalog,
    currency: Currency,
    period: Period,
    today: chrono::NaiveDate,
    now: &[(ledger::Member, Vec<LedgerRow>)],
    before: &[(ledger::Member, Vec<LedgerRow>)],
) -> MonthView {
    let all_now: Vec<&LedgerRow> = now.iter().flat_map(|(_, r)| r).collect();
    let all_before: Vec<&LedgerRow> = before.iter().flat_map(|(_, r)| r).collect();
    let s = sums(&all_now);
    let b = sums(&all_before);
    let prev = period.pred();
    let prev_label = fmt::period(prev);
    let net = s.ins - s.outs;

    // What is still to pay: the fold ring's open slices — a bill with a rule
    // and no proof yet, late or not. A plain untracked row and a one-off are
    // left out for the ring's reason: nothing could ever settle them.
    let mut due: Vec<&LedgerRow> = all_now
        .iter()
        .copied()
        .filter(|r| !r.kind.is_income())
        .filter(|r| {
            matches!(
                r.tally(),
                Some(Tally::Open | Tally::Late | Tally::Scheduled)
            )
        })
        .collect();
    due.sort_by_key(|r| r.due_date);
    let due_total: i64 = due
        .iter()
        .filter_map(|r| r.effective_cents())
        .map(|c| c.get())
        .sum();
    let next_due = due
        .iter()
        .filter_map(|r| r.due_date)
        .find(|d| *d >= today)
        .or_else(|| due.iter().find_map(|r| r.due_date));

    let mut tiles = vec![
        Tile {
            label: t.charts_in().to_string(),
            value: est_money(s.ins, currency, s.est),
            neg: false,
            delta: delta(t, s.ins, b.ins, true),
            note: t.charts_vs(&prev_label),
            meter: None,
        },
        Tile {
            label: t.charts_out().to_string(),
            value: money(s.outs, currency),
            neg: false,
            delta: delta(t, s.outs, b.outs, false),
            note: t.charts_vs(&prev_label),
            meter: None,
        },
        Tile {
            label: t.charts_net().to_string(),
            value: signed(net, currency, s.est),
            neg: net < 0,
            delta: None,
            note: if s.est {
                t.charts_includes_estimate(s.estimates)
            } else {
                String::new()
            },
            meter: None,
        },
    ];
    // A side with nothing in either month says nothing — a selection of
    // bills has no IN, and `R$ 0,00 vs 09/2026` is noise.
    if s.ins == 0 && b.ins == 0 {
        tiles.remove(0);
    } else if s.outs == 0 && b.outs == 0 {
        tiles.remove(1);
    }
    if s.ins > 0 {
        let kept = (net as f64 / s.ins as f64 * 100.0).round() as i64;
        tiles.push(Tile {
            label: t.charts_kept().to_string(),
            value: format!("{}%", kept.to_string().replace('-', "−")),
            neg: kept < 0,
            delta: None,
            note: String::new(),
            meter: Some(chart::pct(kept.clamp(0, 100) as f64)),
        });
    }
    tiles.push(Tile {
        label: t.charts_to_pay().to_string(),
        value: money(due_total, currency),
        neg: false,
        delta: None,
        note: match (due.len(), next_due) {
            (0, _) => t.charts_to_pay_none().to_string(),
            (n, Some(d)) => t.charts_to_pay_note(n as i64, &fmt::date(d)[..5]),
            (n, None) => t.charts_bills(n as i64),
        },
        meter: None,
    });

    // Spend by bucket, biggest first, Uncategorized last — the folds' rule.
    let mut buckets = by_bucket(all_now.iter().copied());
    buckets.sort_by(|a, b| match (&a.0, &b.0) {
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        _ => b.1.cmp(&a.1),
    });
    let earlier: HashMap<Option<String>, i64> =
        by_bucket(all_before.iter().copied()).into_iter().collect();
    let max = buckets
        .iter()
        .map(|(n, v)| (*v).max(*earlier.get(n).unwrap_or(&0)))
        .max()
        .unwrap_or(0)
        .max(1);
    let label = |n: &Option<String>| {
        n.clone()
            .unwrap_or_else(|| t.group_uncategorized().to_string())
    };
    let bucket_bars = buckets
        .iter()
        .map(|(n, v)| {
            let p = *earlier.get(n).unwrap_or(&0);
            let f = |x: i64| chart::pct(x as f64 / max as f64);
            HBar {
                name: label(n),
                unc: n.is_none(),
                f: f(*v),
                at: f((*v).max(p)),
                prev: (p > 0).then(|| f(p)),
                value: money(*v, currency),
                delta: if p > 0 { delta(t, *v, p, false) } else { None },
                note: if p > 0 {
                    String::new()
                } else {
                    t.charts_new().to_string()
                },
                note_class: "flat",
                muted: false,
                tip: Some(Tip {
                    head: label(n),
                    rows: vec![
                        TipRow {
                            label: fmt::period(period),
                            value: money(*v, currency),
                            key: "k-out",
                        },
                        TipRow {
                            label: prev_label.clone(),
                            value: money(p, currency),
                            key: "k-old",
                        },
                    ],
                }),
            }
        })
        .collect();
    let buckets_table = Table {
        head: vec![
            t.charts_bucket().to_string(),
            fmt::period(period),
            prev_label.clone(),
        ],
        rows: buckets
            .iter()
            .map(|(n, v)| TRow {
                name: label(n),
                cells: vec![
                    money(*v, currency),
                    money(*earlier.get(n).unwrap_or(&0), currency),
                ],
            })
            .collect(),
    };

    // Who moved the money — only worth drawing with more than one member.
    let per: Vec<(String, Sums)> = now
        .iter()
        .map(|(m, rows)| (fmt::name(&m.name), sums(&rows.iter().collect::<Vec<_>>())))
        .filter(|(_, s)| s.ins > 0 || s.outs > 0)
        .collect();
    let mmax = per
        .iter()
        .map(|(_, s)| s.ins.max(s.outs))
        .max()
        .unwrap_or(0)
        .max(1);
    let members = if per.len() > 1 {
        per.iter()
            .map(|(n, s)| MirRow {
                name: n.clone(),
                out_f: chart::pct(s.outs as f64 / mmax as f64),
                out_v: money(s.outs, currency),
                in_f: chart::pct(s.ins as f64 / mmax as f64),
                in_v: est_money(s.ins, currency, s.est),
                in_est: s.est,
            })
            .collect()
    } else {
        Vec::new()
    };
    let members_table = Table {
        head: vec![
            String::new(),
            t.charts_in().to_string(),
            t.charts_out().to_string(),
        ],
        rows: per
            .iter()
            .map(|(n, s)| TRow {
                name: n.clone(),
                cells: vec![est_money(s.ins, currency, s.est), money(s.outs, currency)],
            })
            .collect(),
    };

    // Fixed against one-off: what a rule materialized, and what was typed.
    let rec: i64 = all_now
        .iter()
        .filter(|r| !r.kind.is_income() && r.rule_id.is_some())
        .filter_map(|r| r.effective_cents())
        .map(|c| c.get())
        .sum();
    let one = s.outs - rec;
    let stack = (s.outs > 0 && rec > 0 && one > 0).then(|| {
        let rp = rec as f64 / s.outs as f64 * 100.0;
        Stack {
            rec_f: chart::pct(rp),
            rec_v: money(rec, currency),
            rec_pct: format!("{}%", rp.round() as i64),
            one_f: chart::pct(100.0 - rp),
            one_v: money(one, currency),
            one_pct: format!("{}%", (100.0 - rp).round() as i64),
        }
    });

    let due_max = due
        .iter()
        .filter_map(|r| r.effective_cents())
        .map(|c| c.get())
        .max()
        .unwrap_or(0)
        .max(1);
    let to_pay = due
        .iter()
        .take(10)
        .map(|r| {
            let v = r.effective_cents().map(|c| c.get()).unwrap_or(0);
            let f = chart::pct(v as f64 / due_max as f64);
            HBar {
                name: r.title.clone(),
                unc: false,
                f: f.clone(),
                at: f,
                prev: None,
                value: r
                    .effective_cents()
                    .map(|c| fmt::money(c, currency))
                    .unwrap_or_else(|| fmt::money_placeholder(currency)),
                delta: None,
                note: match (r.tally(), r.due_date) {
                    (Some(Tally::Scheduled), _) => t.charts_scheduled().to_string(),
                    (Some(Tally::Late), Some(d)) => t.charts_late(&fmt::date(d)[..5]),
                    (_, Some(d)) => t.charts_due(&fmt::date(d)[..5]),
                    _ => String::new(),
                },
                note_class: if matches!(r.tally(), Some(Tally::Late)) {
                    "bad"
                } else {
                    "flat"
                },
                muted: true,
                tip: None,
            }
        })
        .collect();

    MonthView {
        tiles,
        buckets: bucket_bars,
        buckets_sub: t.charts_buckets_sub(&fmt::period(period)),
        prev_label,
        buckets_table,
        members,
        members_sub: t.charts_members_sub(&fmt::period(period)),
        members_table,
        stack,
        to_pay,
    }
}

// ---------------------------------------------------------------------------
// Year
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn year_view(
    t: &Catalog,
    currency: Currency,
    names: &[String],
    from: Period,
    to: Period,
    current: Period,
    rows: &[ChartRow],
    sel: Option<&[(Line, String)]>,
) -> YearView {
    let ms = months(from, to);
    let n = ms.len();
    let crosses = from.year() != to.year();
    let labels: Vec<String> = ms.iter().map(|p| axis_label(names, *p, crosses)).collect();
    let heads: Vec<String> = ms.iter().map(|p| fmt::period(*p)).collect();
    let cur = ms.iter().position(|p| *p == current);
    let idx = |p: Period| ms.iter().position(|m| *m == p);
    let range = range_label(names, from, to);

    let mut view = YearView {
        flow: None,
        kept: None,
        trend: None,
        moved: Vec::new(),
        moved_short: n < 6,
        moved_sub: String::new(),
        moved_table: Table {
            head: Vec::new(),
            rows: Vec::new(),
        },
        sparks: Vec::new(),
        sparks_sub: t.charts_each_bucket_sub(&range),
        together: None,
        each: Vec::new(),
    };

    // The selection replaces every other chart: the lines, followed.
    if let Some(lines) = sel {
        let mut series: Vec<(String, Vec<i64>, bool)> = lines
            .iter()
            .map(|(_, title)| (title.clone(), vec![0; n], false))
            .collect();
        for r in rows {
            let line = Line::of(r.member_id, r.rule_id, &r.title);
            let (Some(k), Some(i), Some(c)) = (
                lines.iter().position(|(l, _)| *l == line),
                idx(r.period),
                r.cents,
            ) else {
                continue;
            };
            series[k].1[i] += c.get();
            series[k].2 |= r.is_income();
        }
        let max = series
            .iter()
            .flat_map(|(_, v, _)| v.iter().copied())
            .max()
            .unwrap_or(0);
        let (first, last) = (
            labels.first().cloned().unwrap_or_default(),
            labels.last().cloned().unwrap_or_default(),
        );
        view.each = series
            .iter()
            .map(|(name, v, _)| {
                spark(
                    t,
                    currency,
                    name.clone(),
                    false,
                    true,
                    v,
                    max,
                    &heads,
                    first.clone(),
                    last.clone(),
                )
            })
            .collect();
        // Added together only when they are the same kind of money: an income
        // plus a bill is not a figure anybody reads.
        let same_kind = series.iter().all(|s| s.2) || series.iter().all(|s| !s.2);
        if series.len() > 1 && same_kind {
            let sum: Vec<i64> = (0..n)
                .map(|i| series.iter().map(|s| s.1[i]).sum())
                .collect();
            let s = Scale::nice(0, sum.iter().copied().max().unwrap_or(0), 4);
            let b = Band { n };
            let pts: Vec<(f64, f64)> = sum
                .iter()
                .enumerate()
                .map(|(i, v)| (b.center(i), s.top(*v)))
                .collect();
            let mut plot = Plot {
                h: 200,
                yt: y_ticks(&s, currency),
                xt: x_ticks(&labels, cur),
                ..Plot::default()
            };
            plot.paths.push(PathD {
                class: "a-sel",
                d: chart::area(&pts, s.top(0)),
            });
            plot.paths.push(PathD {
                class: "l-sel",
                d: chart::line(&pts),
            });
            for (i, (x, y)) in pts.iter().enumerate() {
                plot.dots.push(Dot {
                    left: chart::pct(*x),
                    top: chart::pct(*y),
                    class: "sm d-sel",
                });
                let mut tip_rows: Vec<TipRow> = series
                    .iter()
                    .map(|(name, v, _)| TipRow {
                        label: name.clone(),
                        value: money(v[i], currency),
                        key: "k-sel",
                    })
                    .collect();
                tip_rows.push(TipRow {
                    label: t.charts_together().to_string(),
                    value: money(sum[i], currency),
                    key: "k-net",
                });
                plot.hits.push(hit(
                    b,
                    i,
                    Tip {
                        head: heads[i].clone(),
                        rows: tip_rows,
                    },
                ));
            }
            let mut head = vec![String::new()];
            head.extend(series.iter().map(|s| s.0.clone()));
            head.push(t.charts_together().to_string());
            view.together = Some(PlotCard {
                wide: true,
                title: t.charts_together_title(series.len() as i64),
                sub: t.charts_together_sub(&range),
                delta: None,
                legend: Vec::new(),
                plot,
                table: Table {
                    head,
                    rows: (0..n)
                        .map(|i| {
                            let mut cells: Vec<String> =
                                series.iter().map(|s| money(s.1[i], currency)).collect();
                            cells.push(money(sum[i], currency));
                            TRow {
                                name: heads[i].clone(),
                                cells,
                            }
                        })
                        .collect(),
                },
            });
        }
        return view;
    }

    let mut ins = vec![0i64; n];
    let mut outs = vec![0i64; n];
    let mut est = vec![false; n];
    let mut buckets: Vec<(Option<String>, Vec<i64>)> = Vec::new();
    for r in rows {
        let (Some(i), Some(c)) = (idx(r.period), r.cents) else {
            continue;
        };
        if r.is_income() {
            ins[i] += c.get();
            est[i] |= r.estimated;
        } else {
            outs[i] += c.get();
            let name = bucket_name(r.bucket.as_deref());
            match buckets.iter_mut().find(|(b, _)| *b == name) {
                Some((_, v)) => v[i] += c.get(),
                None => {
                    let mut v = vec![0; n];
                    v[i] = c.get();
                    buckets.push((name, v));
                }
            }
        }
    }
    let nets: Vec<i64> = (0..n).map(|i| ins[i] - outs[i]).collect();

    view.flow = Some(flow_card(
        t,
        currency,
        t.charts_flow_title().to_string(),
        t.charts_flow_sub(&range),
        &labels,
        &heads,
        &ins,
        &outs,
        &est,
        true,
        cur,
    ));

    // Kept so far: NET added up.
    {
        let mut acc = 0;
        let cum: Vec<i64> = nets
            .iter()
            .map(|v| {
                acc += v;
                acc
            })
            .collect();
        let s = Scale::nice(
            cum.iter().copied().min().unwrap_or(0).min(0),
            cum.iter().copied().max().unwrap_or(0).max(0),
            4,
        );
        let b = Band { n };
        let pts: Vec<(f64, f64)> = cum
            .iter()
            .enumerate()
            .map(|(i, v)| (b.center(i), s.top(*v)))
            .collect();
        let mut plot = Plot {
            h: 190,
            yt: y_ticks(&s, currency),
            xt: x_ticks(&labels, cur),
            ..Plot::default()
        };
        plot.paths.push(PathD {
            class: "a-net",
            d: chart::area(&pts, s.top(0)),
        });
        plot.paths.push(PathD {
            class: "l-net",
            d: chart::line(&pts),
        });
        let any_est = est.iter().any(|e| *e);
        if let (Some((x, y)), Some(last)) = (pts.last(), cum.last()) {
            plot.dots.push(Dot {
                left: chart::pct(*x),
                top: chart::pct(*y),
                class: if *last < 0 { "d-neg" } else { "" },
            });
            plot.labels.push(Label {
                left: chart::pct(*x),
                top: chart::pct(*y),
                class: "money",
                text: signed(*last, currency, any_est),
                // Off the side the line came from: above a line that rose to
                // its end, below one that fell.
                at: if *last >= cum[0] { "above" } else { "below" },
            });
        }
        let mut acc_est = false;
        for i in 0..n {
            acc_est |= est[i];
            plot.hits.push(hit(
                b,
                i,
                Tip {
                    head: heads[i].clone(),
                    rows: vec![
                        TipRow {
                            label: t.charts_kept_so_far().to_string(),
                            value: signed(cum[i], currency, acc_est),
                            key: "k-net",
                        },
                        TipRow {
                            label: t.charts_that_month().to_string(),
                            value: signed(nets[i], currency, est[i]),
                            key: if nets[i] < 0 { "k-neg" } else { "k-old" },
                        },
                    ],
                },
            ));
        }
        view.kept = Some(PlotCard {
            wide: false,
            title: t.charts_kept_title().to_string(),
            sub: t.charts_kept_sub().to_string(),
            delta: None,
            legend: Vec::new(),
            plot,
            table: Table {
                head: vec![String::new(), t.charts_kept_so_far().to_string()],
                rows: (0..n)
                    .map(|i| TRow {
                        name: heads[i].clone(),
                        cells: vec![signed(cum[i], currency, est[..=i].iter().any(|e| *e))],
                    })
                    .collect(),
            },
        });
    }

    // Is income growing? Each month a dot, the 3-month average a line.
    {
        let avg: Vec<Option<i64>> = (0..n)
            .map(|i| (i >= 2).then(|| (ins[i] + ins[i - 1] + ins[i - 2]) / 3))
            .collect();
        let lo = ins.iter().copied().min().unwrap_or(0);
        let hi = ins.iter().copied().max().unwrap_or(0);
        // Not from zero: a household's income moves by a few percent, and a
        // zero baseline would draw that as a flat line.
        let pad = ((hi - lo) / 6).max(hi / 50);
        let s = Scale::nice((lo - pad).max(0), hi + pad, 4);
        let b = Band { n };
        let mut plot = Plot {
            h: 190,
            yt: y_ticks(&s, currency),
            xt: x_ticks(&labels, cur),
            ..Plot::default()
        };
        let pts: Vec<(f64, f64)> = avg
            .iter()
            .enumerate()
            .filter_map(|(i, a)| a.map(|v| (b.center(i), s.top(v))))
            .collect();
        plot.paths.push(PathD {
            class: "l-avg",
            d: chart::line(&pts),
        });
        for i in 0..n {
            plot.dots.push(Dot {
                left: chart::pct(b.center(i)),
                top: chart::pct(s.top(ins[i])),
                class: if est[i] { "sm d-pos d-est" } else { "sm d-pos" },
            });
            let mut tip_rows = vec![TipRow {
                label: t.charts_in().to_string(),
                value: est_money(ins[i], currency, est[i]),
                key: "k-in",
            }];
            if let Some(a) = avg[i] {
                tip_rows.push(TipRow {
                    label: t.charts_avg().to_string(),
                    value: est_money(a, currency, est[i.saturating_sub(2)..=i].iter().any(|e| *e)),
                    key: "k-in",
                });
            }
            plot.hits.push(hit(
                b,
                i,
                Tip {
                    head: heads[i].clone(),
                    rows: tip_rows,
                },
            ));
        }
        let (sub, d) = match (avg.get(2).copied().flatten(), avg.last().copied().flatten()) {
            (Some(a), Some(z)) if n > 3 => (t.charts_trend_sub(&labels[2]), delta(t, z, a, true)),
            _ => (t.charts_trend_short().to_string(), None),
        };
        view.trend = Some(PlotCard {
            wide: false,
            title: t.charts_trend_title().to_string(),
            sub,
            delta: d,
            legend: vec![
                Legend {
                    class: "lg-dot",
                    label: t.charts_month().to_string(),
                },
                Legend {
                    class: "lg-avg",
                    label: t.charts_avg().to_string(),
                },
            ],
            plot,
            table: Table {
                head: vec![
                    String::new(),
                    t.charts_in().to_string(),
                    t.charts_avg().to_string(),
                ],
                rows: (0..n)
                    .map(|i| TRow {
                        name: heads[i].clone(),
                        cells: vec![
                            est_money(ins[i], currency, est[i]),
                            avg[i]
                                .map(|a| money(a, currency))
                                .unwrap_or_else(|| "—".into()),
                        ],
                    })
                    .collect(),
            },
        });
    }

    // Buckets, biggest first, Uncategorized last.
    buckets.sort_by(|a, b| match (&a.0, &b.0) {
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        _ => b.1.iter().sum::<i64>().cmp(&a.1.iter().sum::<i64>()),
    });
    let label = |b: &Option<String>| {
        b.clone()
            .unwrap_or_else(|| t.group_uncategorized().to_string())
    };

    // What moved: the last three months against the three before, in money
    // per month — a percentage put an R$ 76 line at the top of the list.
    if n >= 6 {
        let avg3 = |v: &[i64], end: usize| v[end - 3..end].iter().sum::<i64>() / 3;
        let mut moved: Vec<(String, i64, i64, i64)> = buckets
            .iter()
            .map(|(b, v)| (label(b), avg3(v, n - 3), avg3(v, n), 0))
            .map(|(name, before, now, _)| (name, before, now, now - before))
            .filter(|(_, before, _, change)| *before > 0 && change.abs() >= 5_000)
            .collect();
        moved.sort_by_key(|m| -m.3);
        // The biggest rises, then the biggest falls — never only one side
        // because seven of one crowded the other out.
        let (up, down): (Vec<_>, Vec<_>) = moved.into_iter().partition(|m| m.3 > 0);
        let mut moved: Vec<_> = up.into_iter().take(4).collect();
        let mut down: Vec<_> = down.into_iter().rev().take(4).collect();
        down.reverse();
        moved.extend(down);
        let mx = moved.iter().map(|m| m.3.abs()).max().unwrap_or(1).max(1);
        view.moved = moved
            .iter()
            .map(|(name, before, _, change)| MoveRow {
                name: name.clone(),
                up: *change > 0,
                f: chart::pct(change.abs() as f64 / mx as f64),
                amount: t.charts_per_month(&format!(
                    "{}{}",
                    if *change > 0 { "+" } else { "−" },
                    money(change.abs(), currency)
                )),
                pct: chart::change_pct(before + change, *before)
                    .map(pct_text)
                    .unwrap_or_default(),
            })
            .collect();
        let (now_l, before_l) = (
            range_label(names, ms[n - 3], ms[n - 1]),
            range_label(names, ms[n - 6], ms[n - 4]),
        );
        view.moved_sub = t.charts_moved_sub(&now_l, &before_l);
        view.moved_table = Table {
            head: vec![
                t.charts_bucket().to_string(),
                before_l,
                now_l,
                t.charts_change().to_string(),
            ],
            rows: moved
                .iter()
                .map(|(name, before, now, change)| TRow {
                    name: name.clone(),
                    cells: vec![
                        money(*before, currency),
                        money(*now, currency),
                        format!(
                            "{}{}",
                            if *change > 0 { "+" } else { "−" },
                            money(change.abs(), currency)
                        ),
                    ],
                })
                .collect(),
        };
    }

    let max = buckets
        .iter()
        .flat_map(|(_, v)| v.iter().copied())
        .max()
        .unwrap_or(0);
    let (first, last) = (
        labels.first().cloned().unwrap_or_default(),
        labels.last().cloned().unwrap_or_default(),
    );
    view.sparks = buckets
        .iter()
        .map(|(b, v)| {
            spark(
                t,
                currency,
                label(b),
                b.is_none(),
                false,
                v,
                max,
                &heads,
                first.clone(),
                last.clone(),
            )
        })
        .collect();
    view
}

// ---------------------------------------------------------------------------
// Years
// ---------------------------------------------------------------------------

/// The Years tab's first year: the field's, else the last ten — never
/// before the first row, never past this year.
fn years_since(asked: Option<i32>, first: i32, this_year: i32) -> i32 {
    asked
        .unwrap_or(this_year - (DEFAULT_YEARS - 1))
        .clamp(first.min(this_year), this_year)
}

fn years_view(
    t: &Catalog,
    currency: Currency,
    names: &[String],
    current: Period,
    rows: &[ChartRow],
    since: i32,
    many: bool,
) -> YearsView {
    let last_month = current.month();
    let years: Vec<i32> = (since.min(current.year())..=current.year()).collect();
    let ny = years.len();

    // [year][month] in and out, and whether any of it is a projection.
    let mut ins = vec![[0i64; 12]; ny];
    let mut outs = vec![[0i64; 12]; ny];
    let mut est = vec![[false; 12]; ny];
    for r in rows {
        let (Some(y), Some(c)) = (years.iter().position(|y| *y == r.period.year()), r.cents) else {
            continue;
        };
        let m = r.period.month() as usize - 1;
        if r.is_income() {
            ins[y][m] += c.get();
            est[y][m] |= r.estimated;
        } else {
            outs[y][m] += c.get();
        }
    }
    let ytd = |a: &[i64; 12]| a[..last_month as usize].iter().sum::<i64>();
    let ytd_in: Vec<i64> = ins.iter().map(ytd).collect();
    let ytd_out: Vec<i64> = outs.iter().map(ytd).collect();
    let ytd_est: Vec<bool> = est
        .iter()
        .map(|e| e[..last_month as usize].iter().any(|x| *x))
        .collect();
    let labels: Vec<String> = years.iter().map(i32::to_string).collect();
    let span = range_label(
        names,
        Period::new(current.year(), 1).unwrap_or(current),
        current,
    );

    let flow = flow_card(
        t,
        currency,
        t.charts_years_flow_title().to_string(),
        t.charts_years_flow_sub(&span),
        &labels,
        &labels,
        &ytd_in,
        &ytd_out,
        &ytd_est,
        // A part-year is still a real bar; its `~` is in the tip and table.
        false,
        Some(ny - 1),
    );

    let cum = |kind_in: bool, title: String| -> PlotCard {
        let src = if kind_in { &ins } else { &outs };
        let run = |a: &[i64]| {
            let mut acc = 0;
            a.iter()
                .map(|v| {
                    acc += v;
                    acc
                })
                .collect::<Vec<i64>>()
        };
        let now = run(&src[ny - 1][..last_month as usize]);
        // The earlier years drawn, last year first: one, or with `many` up
        // to four, never before the range's first year. Each one back is
        // fainter; only last year is shaded against and labeled.
        let olds: Vec<(i32, Vec<i64>)> = (1..ny)
            .take(if many { MANY_OLD } else { 1 })
            .map(|k| (years[ny - 1 - k], run(&src[ny - 1 - k])))
            .collect();
        let was = olds.first().map(|(_, w)| w);
        let hi = now
            .iter()
            .chain(olds.iter().flat_map(|(_, w)| w))
            .copied()
            .max()
            .unwrap_or(0);
        let s = Scale::nice(0, hi, 4);
        let b = Band { n: 12 };
        let pts = |v: &[i64]| -> Vec<(f64, f64)> {
            v.iter()
                .enumerate()
                .map(|(i, v)| (b.center(i), s.top(*v)))
                .collect()
        };
        let pn = pts(&now);
        let cur_y = current.year();
        let mut plot = Plot {
            h: 210,
            yt: y_ticks(&s, currency),
            xt: x_ticks(names, Some(last_month as usize - 1)),
            pad: true,
            ..Plot::default()
        };
        let lm = last_month as usize - 1;
        let d = was.and_then(|w| delta(t, now[lm], w[lm], kind_in));
        // Oldest first, so last year's line sits on top of the rest.
        for (k, (_, w)) in olds.iter().enumerate().skip(1).rev() {
            plot.paths.push(PathD {
                class: OLD_LINE[k],
                d: chart::line(&pts(w)),
            });
        }
        if let Some(w) = was {
            let pw = pts(w);
            let good = d.as_ref().is_none_or(|d| d.class != "bad");
            plot.paths.push(PathD {
                class: if good { "a-good" } else { "a-bad" },
                d: chart::between(&pn, &pw[..pn.len()]),
            });
            plot.paths.push(PathD {
                class: OLD_LINE[0],
                d: chart::line(&pw),
            });
            plot.dots.push(Dot {
                left: chart::pct(pw[lm].0),
                top: chart::pct(pw[lm].1),
                class: "sm d-old",
            });
            if let Some((x, y)) = pw.last() {
                plot.labels.push(Label {
                    left: chart::pct(*x),
                    top: chart::pct(*y),
                    class: "old",
                    text: (cur_y - 1).to_string(),
                    at: "end",
                });
            }
        }
        plot.paths.push(PathD {
            class: "l-cur",
            d: chart::line(&pn),
        });
        if let Some((x, y)) = pn.last() {
            plot.dots.push(Dot {
                left: chart::pct(*x),
                top: chart::pct(*y),
                class: "",
            });
            let below = was.is_some_and(|w| now[lm] < w[lm]);
            plot.labels.push(Label {
                left: chart::pct(*x),
                top: chart::pct(*y),
                class: "",
                text: format!("{cur_y} · {}", chart::tick(now[lm], currency)),
                at: if below { "below" } else { "above" },
            });
        }
        for i in 0..12 {
            let mut tip_rows = Vec::new();
            if i < now.len() {
                tip_rows.push(TipRow {
                    label: cur_y.to_string(),
                    value: money(now[i], currency),
                    key: "k-net",
                });
            }
            for (k, (y, w)) in olds.iter().enumerate() {
                tip_rows.push(TipRow {
                    label: y.to_string(),
                    value: money(w[i], currency),
                    key: OLD_KEY[k],
                });
            }
            plot.hits.push(hit(
                b,
                i,
                Tip {
                    head: names[i].clone(),
                    rows: tip_rows,
                },
            ));
        }
        let mut legend = vec![Legend {
            class: "lg-cur",
            label: cur_y.to_string(),
        }];
        let mut head = vec![String::new(), cur_y.to_string()];
        for (k, (y, _)) in olds.iter().enumerate() {
            legend.push(Legend {
                class: OLD_LEGEND[k],
                label: y.to_string(),
            });
            head.push(y.to_string());
        }
        PlotCard {
            wide: false,
            title,
            sub: if was.is_some() {
                t.charts_by_month_against(&names[lm], &(cur_y - 1).to_string())
            } else {
                String::new()
            },
            delta: d,
            legend,
            plot,
            table: Table {
                head,
                rows: (0..12)
                    .map(|i| {
                        let mut cells = vec![
                            now.get(i)
                                .map(|v| money(*v, currency))
                                .unwrap_or_else(|| "—".into()),
                        ];
                        cells.extend(olds.iter().map(|(_, w)| money(w[i], currency)));
                        TRow {
                            name: names[i].clone(),
                            cells,
                        }
                    })
                    .collect(),
            },
        }
    };

    let rows_out = (0..ny)
        .rev()
        .map(|y| {
            let net = ytd_in[y] - ytd_out[y];
            let vs = (y > 0)
                .then(|| delta(t, net, ytd_in[y - 1] - ytd_out[y - 1], true))
                .flatten();
            let whole = (years[y] < current.year()).then(|| {
                signed(
                    ins[y].iter().sum::<i64>() - outs[y].iter().sum::<i64>(),
                    currency,
                    false,
                )
            });
            YearRow {
                year: labels[y].clone(),
                cur: y == ny - 1,
                ins: est_money(ytd_in[y], currency, ytd_est[y]),
                outs: money(ytd_out[y], currency),
                net: signed(net, currency, ytd_est[y]),
                neg: net < 0,
                kept: if ytd_in[y] > 0 {
                    format!(
                        "{}%",
                        (net as f64 / ytd_in[y] as f64 * 100.0).round() as i64
                    )
                    .replace('-', "−")
                } else {
                    "—".into()
                },
                vs,
                whole: whole.unwrap_or_else(|| "—".into()),
            }
        })
        .collect();

    YearsView {
        flow,
        spent: cum(false, t.charts_spent_so_far().to_string()),
        earned: cum(true, t.charts_earned_so_far().to_string()),
        rows: rows_out,
        table_sub: t.charts_years_table_sub(&span),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(n: usize, cur: Option<usize>) -> Vec<(String, bool)> {
        let labels: Vec<String> = (0..n).map(|i| i.to_string()).collect();
        x_ticks(&labels, cur)
            .into_iter()
            .map(|x| (x.label, x.minor))
            .collect()
    }

    #[test]
    fn a_long_axis_names_every_few_months_from_the_last() {
        // Up to a year, every month — a phone every other one.
        let t = named(12, Some(11));
        assert_eq!(t.len(), 12);
        assert_eq!(t.iter().filter(|(_, minor)| *minor).count(), 6);
        assert!(named(8, Some(7)).iter().all(|(_, minor)| !minor));
        // Three years: every 6th, ending on the last; a phone, one a year.
        let t = named(36, Some(35));
        assert_eq!(t.len(), 6);
        assert_eq!(t.last(), Some(&("35".to_string(), false)));
        assert_eq!(t[t.len() - 2], ("29".to_string(), true));
        assert_eq!(named(24, Some(23)).len(), 8);
        // A current month off the grid is named, its crowding neighbors not.
        let t = named(18, Some(10));
        let names: Vec<&str> = t.iter().map(|(l, _)| l.as_str()).collect();
        assert!(names.contains(&"10"));
        assert!(!names.contains(&"9") && !names.contains(&"11"));
        assert!(named(0, None).is_empty());
    }

    #[test]
    fn a_line_is_its_rule_else_its_title_in_one_ledger() {
        assert_eq!(
            Line::of(1, Some(7), "Água"),
            Line::of(1, Some(7), "Água · Sabesp")
        );
        assert_ne!(Line::of(1, Some(7), "Água"), Line::of(2, Some(7), "Água"));
        assert_eq!(
            Line::of(1, None, "Mercado"),
            Line::Title(1, "Mercado".into())
        );
        assert_ne!(
            Line::of(1, None, "Mercado"),
            Line::of(1, Some(3), "Mercado")
        );
    }

    #[test]
    fn ranges_read_as_months_or_periods() {
        let names: Vec<String> = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec"
            .split(' ')
            .map(String::from)
            .collect();
        let p = |s| Period::parse(s).unwrap();
        assert_eq!(range_label(&names, p("2026-01"), p("2026-10")), "Jan–Oct");
        assert_eq!(range_label(&names, p("2026-10"), p("2026-10")), "Oct");
        assert_eq!(
            range_label(&names, p("2025-11"), p("2026-02")),
            "11/2025 – 02/2026"
        );
    }

    #[test]
    fn the_years_open_on_the_last_ten() {
        assert_eq!(years_since(None, 2010, 2026), 2017);
        assert_eq!(years_since(None, 2024, 2026), 2024);
        assert_eq!(years_since(Some(2012), 2010, 2026), 2012);
        assert_eq!(years_since(Some(2001), 2010, 2026), 2010);
        assert_eq!(years_since(Some(2030), 2010, 2026), 2026);
        // A first row dated after today is still drawn from this year.
        assert_eq!(years_since(None, 2027, 2026), 2026);
    }

    fn years_of(since: i32, many: bool) -> YearsView {
        use crate::domain::status::TxKind;
        use crate::ui::i18n::Lang;
        let rows: Vec<ChartRow> = (2019..=2026)
            .flat_map(|y| (1..=12).map(move |m| (y, m)))
            .filter(|(y, m)| *y < 2026 || *m <= 10)
            .filter(|(y, _)| *y >= since)
            .flat_map(|(y, m)| {
                [TxKind::In, TxKind::Out].map(|kind| ChartRow {
                    id: 0,
                    period: Period::new(y, m).unwrap(),
                    member_id: 1,
                    kind,
                    cents: Some(Cents(100_000 * (y as i64 - 2018))),
                    estimated: false,
                    title: String::new(),
                    bucket: None,
                    rule_id: None,
                })
            })
            .collect();
        let names: Vec<String> = (1..=12).map(|m| m.to_string()).collect();
        years_view(
            &Catalog::new(Lang::En),
            Currency::Brl,
            &names,
            Period::new(2026, 10).unwrap(),
            &rows,
            since,
            many,
        )
    }

    fn old_lines(c: &PlotCard) -> Vec<&str> {
        c.plot
            .paths
            .iter()
            .map(|p| p.class)
            .filter(|c| c.starts_with("l-old"))
            .collect()
    }

    #[test]
    fn the_years_start_where_asked() {
        let v = years_of(2024, false);
        assert_eq!(v.flow.plot.cols.len(), 3);
        assert_eq!(v.rows.len(), 3);
        assert_eq!(years_of(2019, false).rows.len(), 8);
    }

    #[test]
    fn two_lines_or_up_to_five() {
        let two = years_of(2019, false);
        assert_eq!(old_lines(&two.spent), ["l-old"]);
        assert_eq!(two.spent.table.head, ["", "2026", "2025"]);

        let five = years_of(2019, true);
        // Oldest first, so last year's line is drawn on top.
        assert_eq!(
            old_lines(&five.earned),
            ["l-old o4", "l-old o3", "l-old o2", "l-old"]
        );
        assert_eq!(five.earned.legend.len(), 5);
        assert_eq!(five.earned.plot.hits[0].tip.rows.len(), 5);
        assert_eq!(
            five.earned.table.head,
            ["", "2026", "2025", "2024", "2023", "2022"]
        );
        // Last year alone is shaded against and labeled.
        assert_eq!(
            five.earned
                .plot
                .paths
                .iter()
                .filter(|p| p.class.starts_with("a-"))
                .count(),
            1
        );

        // Never before the range's first year.
        assert_eq!(old_lines(&years_of(2024, true).spent).len(), 2);
        assert!(old_lines(&years_of(2026, true).spent).is_empty());
    }

    #[test]
    fn signed_money_has_a_side() {
        assert_eq!(signed(826_800, Currency::Brl, false), "+R$ 8.268,00");
        assert_eq!(signed(-135_000, Currency::Brl, false), "−R$ 1.350,00");
        assert_eq!(signed(100, Currency::Brl, true), "~+R$ 1,00");
    }
}
