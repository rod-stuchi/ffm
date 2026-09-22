//! The ledger, read-only.
//!
//! A month is a *view*: one continuous transactions table, sliced by `period`.
//! There is no "copy last month" operation and never will be, so navigating
//! months is a different `WHERE`, not a different set of rows.
//!
//! This is deliberately not the full app shell yet — no command bar, no month
//! picker, no keymap. It renders real panes from real rows, which is the whole
//! point of the phase: the row component stopped being a fixture.

use crate::db::ledger::{self, Pane};
use crate::domain::period::Period;
use crate::state::AppState;
use crate::ui::fmt;
use crate::ui::i18n::Catalog;
use crate::ui::row::{LineNumbers, RowCtx, RowView};
use crate::ui::theme;
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use chrono::{Local, NaiveDate};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Params {
    /// `YYYY-MM`. Absent means the current month — always, on every first load.
    /// Never the last-viewed month and never January.
    period: Option<String>,
}

/// A pane's three numbers, and the household's four, live in their own structs
/// because a mutation has to send them back too: a row that lands without its
/// totals moving is a ledger that disagrees with itself on screen.
/// One cell in the month picker.
pub struct MonthCell {
    pub key: String,
    pub label: String,
    pub current: bool,
    /// How many unresolved occurrences sit in this month. Zero means no dot —
    /// ambient like every other status in this app, rather than a toast that
    /// gets dismissed and then forgotten.
    pub unresolved: usize,
    pub title: String,
}

pub struct PaneTotals {
    pub id: i64,
    pub ins: String,
    pub outs: String,
    pub balance: String,
    pub negative: bool,
}

/// Money, with the tilde when any part of it is a projection.
///
/// The mark carries whether the money moved; the tilde carries whether the
/// number is a fact. One channel per fact, as everywhere else — and the two
/// are asked separately, so a confirmed income in an estimated pane still
/// reads as the fact it is.
fn money(
    amount: crate::domain::money::Cents,
    currency: crate::domain::money::Currency,
    estimated: bool,
) -> String {
    if estimated {
        fmt::money_estimated(amount, currency)
    } else {
        fmt::money(amount, currency)
    }
}

pub struct HouseTotals {
    pub ins: String,
    pub outs: String,
    pub net: String,
    pub negative: bool,
}

/// Every pane's totals for one period, plus the household's — read through the
/// same pane query a page load uses, so a fragment's numbers are computed by
/// exactly the same path as the page's.
pub async fn totals(st: &AppState, auth: &Auth, period: Period) -> (Vec<PaneTotals>, HouseTotals) {
    let house = ledger::household(&st.pool).await.ok().flatten();
    let currency = house.as_ref().map(|h| h.currency).unwrap_or_default();
    let show_recurring = house.as_ref().is_none_or(|h| h.show_recurring);
    let ctx = RowCtx {
        today: Local::now().date_naive(),
        sort: ledger::Sort::parse(&auth.settings.ledger_sort),
        currency,
        show_recurring,
        show_delta: auth.settings.ledger_show_delta,
        narrow: false,
        line_numbers: LineNumbers::parse(&auth.settings.line_numbers),
    };

    let mut panes = Vec::new();
    let mut ins = crate::domain::money::Cents::ZERO;
    let mut outs = crate::domain::money::Cents::ZERO;
    let mut any_estimated = false;

    for m in ledger::members(&st.pool).await.unwrap_or_default() {
        let Ok(pane) = ledger::pane_with_vault(
            &st.pool,
            &m,
            period,
            ctx,
            auth.viewer,
            Some(&st.cfg.receipts_root),
        )
        .await
        else {
            continue;
        };
        ins += pane.ins;
        outs += pane.outs;
        any_estimated |= pane.estimated;
        let balance = pane.balance();
        panes.push(PaneTotals {
            id: m.id,
            ins: money(pane.ins, currency, pane.estimated),
            // Only income varies, so OUTs are never a projection.
            outs: fmt::money(pane.outs, currency),
            balance: money(balance.abs(), currency, pane.estimated),
            negative: balance.is_negative(),
        });
    }

    let net = ins - outs;
    (
        panes,
        HouseTotals {
            ins: money(ins, currency, any_estimated),
            outs: fmt::money(outs, currency),
            net: money(net.abs(), currency, any_estimated),
            negative: net.is_negative(),
        },
    )
}

/// One `bucket` fold: a framed container with its name, a row count and a
/// subtotal that stays visible whether the fold is open or closed.
///
/// **That subtotal is why there is no separate report screen.** `zM` closes
/// every fold and the ledger already reads as a per-category summary —
/// browsing and summarising are one mechanism at two zoom levels.
pub struct Group {
    pub name: String,
    /// Already pluralised — the count is a number the catalog has to agree
    /// with, so it is rendered where the catalog lives rather than cast in a
    /// template.
    pub count: String,
    pub subtotal: String,
    pub negative: bool,
    pub rows: Vec<RowView>,
}

pub struct PaneView {
    /// Sent with every add, because which pane is focused is data — a
    /// server-side idea of "current member" drifts the moment the app is open
    /// in two windows.
    pub id: i64,
    pub name: String,
    pub rows: Vec<RowView>,
    /// Empty unless the member grouped the pane. Grouping never silently drops
    /// a row: anything with no bucket lands in `Uncategorized`.
    pub groups: Vec<Group>,
    pub grouped: bool,
    pub totals: PaneTotals,
}

#[derive(Template)]
#[template(path = "pages/ledger.html")]
struct LedgerPage {
    theme_css: String,
    attr: Option<&'static str>,
    me: String,
    admin: bool,
    previewing: bool,
    t: Catalog,
    household: String,
    period: String,
    prev: String,
    next: String,
    panes: Vec<PaneView>,
    /// `YYYY-MM`, for the forms. The label above is `fmt::period`.
    period_key: String,
    months: Vec<MonthCell>,
    /// The dot on the trigger: any month **other than this one** with
    /// something overdue and unreceipted.
    unresolved_total: usize,
    unresolved_title: String,
    /// The member's layout, finally applied. `auto` resolves in CSS from
    /// `min_split`, rendered into one media query — never on the server and
    /// never in Alpine, either of which would be wrong on first paint and on
    /// every resize.
    layout_panes: String,
    /// `relative` | `absolute` | `none`. The shell needs it because relative
    /// numbers are measured from the cursor, and the cursor is Alpine's.
    nu: &'static str,
    /// Shown in the statusline's two clickable cells, and the initial text
    /// before Alpine takes over — so the bar is not blank for a frame.
    nu_label: &'static str,
    sort: &'static str,
    sort_label: String,
    /// The signed-in member, for the statusline's own cell. The SESSION's
    /// member, never the focused pane's — focusing somebody else's pane does
    /// not make you them.
    me_name: String,
    me_admin: bool,
    /// The `:` bar's hint — **exactly** what `runCommand` handles, built here
    /// rather than written out in the template so the two cannot drift. The
    /// first version was copied from the mock and advertised `add`, `in`,
    /// `move`, `undo` and `help`, none of which exist: a hint that lists
    /// commands the app refuses is worse than no hint.
    ///
    /// Admin-only commands are absent for a non-admin, and for an admin
    /// previewing as one — the same rule that removes the Household button
    /// rather than disabling it.
    command_hint: String,
    /// The household's number format, for the amount mask — `{decimals,
    /// decimal, group}` as JSON. From `household_settings.locale_currency`,
    /// the one value every money display in this app reads, so a masked field
    /// cannot disagree with the rows above it.
    money_format: String,
    min_split: i64,
    max_width: i64,
    gap: i64,
    /// `E` on a row with no rule behind it says so in the statusline rather
    /// than doing nothing at all.
    not_a_rule: String,
    unsaved_tag: String,
    drafts_parked: String,
    no_pane_that_way: String,
    reattribute_title: String,
    details_none: String,
    no_files: String,
    months_word: String,
    /// The `?` modal, built from the same table `design/spec.md` documents —
    /// so a key that exists and a key that is written down stay the same set.
    keymap: Vec<(String, Vec<(&'static str, &'static str)>)>,
    /// The three "there is no such file" messages — `g1`/`g2`/`g3` on a row
    /// that has not got one. Three fields rather than a JSON blob, so each is
    /// escaped by the same filter as every other string handed to the page.
    /// `not a command: {cmd}` — with the placeholder left in, so the pane can
    /// fill it without the catalog having to know about JavaScript.
    not_a_command: String,
    file_not_tracked: String,
    no_bill: String,
    no_scheduled: String,
    no_receipt: String,
    house: HouseTotals,
    empty: bool,
}

pub async fn ledger(
    State(st): State<AppState>,
    auth: Auth,
    Query(p): Query<Params>,
) -> impl IntoResponse {
    let today = Local::now().date_naive();
    let period = p
        .period
        .as_deref()
        .and_then(Period::parse)
        .unwrap_or_else(|| Period::of(today));

    let house = ledger::household(&st.pool).await.unwrap_or(None);
    let (name, currency, show_recurring) = match &house {
        Some(h) => (h.name.clone(), h.currency, h.show_recurring),
        None => ("FFM".to_string(), Default::default(), true),
    };
    // Language is the SESSION member's, full stop — the pre-login cookie is
    // never read on an authenticated request. Currency is the household's,
    // because every member's rows feed the same totals. Two axes, two sources.
    let lang = auth.settings.language;

    let ctx = RowCtx {
        today,
        // The member's own choice — it lives in their settings row, which is
        // the same row `:sort date` and the :profile radio write.
        sort: ledger::Sort::parse(&auth.settings.ledger_sort),
        currency,
        show_recurring,
        show_delta: auth.settings.ledger_show_delta,
        narrow: false,
        line_numbers: LineNumbers::parse(&auth.settings.line_numbers),
    };
    let t = Catalog::new(lang);

    let members = ledger::members(&st.pool).await.unwrap_or_default();
    let mut panes = Vec::new();
    let mut ins = crate::domain::money::Cents::ZERO;
    let mut outs = crate::domain::money::Cents::ZERO;
    let mut any_estimated = false;
    // Grouping is a derived view exactly like sort: the moment rows are
    // re-clustered by category, J/K refuse for the same reason.
    let grouped = auth.settings.ledger_group == "bucket";

    for m in &members {
        // Privacy is applied inside the query, so the rows and the totals
        // below cannot disagree about what this viewer can see.
        let Ok(pane) = ledger::pane_with_vault(
            &st.pool,
            m,
            period,
            ctx,
            auth.viewer,
            Some(&st.cfg.receipts_root),
        )
        .await
        else {
            continue;
        };
        ins += pane.ins;
        outs += pane.outs;
        any_estimated |= pane.estimated;
        panes.push(view(pane, &t, currency, grouped));
    }

    let unresolved =
        ledger::unresolved_months(&st.pool, auth.viewer, today, Some(&st.cfg.receipts_root))
            .await
            .unwrap_or_default();

    let net = ins - outs;
    Html(LedgerPage {
        // The member's palette, the device's light/dark — two facts, two homes.
        theme_css: theme::render(
            auth.settings.theme,
            auth.mode,
            &auth.settings.theme_overrides,
        )
        .to_css(),
        attr: auth.mode.attr(),
        me: auth.session.member.name.clone(),
        admin: auth.viewer.is_admin(),
        previewing: auth.session.preview,
        household: name,
        period: fmt::period(period),
        period_key: period.to_string(),
        months: month_cells(&st, &auth, period, today).await,
        unresolved_total: unresolved.iter().map(|u| u.count).sum(),
        unresolved_title: t
            .unresolved_dot(unresolved.iter().map(|u| u.count).sum::<usize>() as i64),
        layout_panes: auth.settings.layout_panes.clone(),
        nu: LineNumbers::parse(&auth.settings.line_numbers).code(),
        nu_label: match LineNumbers::parse(&auth.settings.line_numbers) {
            LineNumbers::Relative => "rnu",
            LineNumbers::Absolute => "nu",
            LineNumbers::Off => "nonu",
        },
        sort: ledger::Sort::parse(&auth.settings.ledger_sort).code(),
        sort_label: ledger::Sort::parse(&auth.settings.ledger_sort)
            .code()
            .to_string(),
        me_name: auth.session.member.name.clone(),
        // Whether admin powers are actually IN EFFECT, not what the row says
        // — an admin previewing as a member should read as a member here, or
        // the badge would contradict what the rest of the app is doing.
        me_admin: auth.viewer.is_admin(),
        command_hint: command_hint(auth.viewer.is_admin(), auth.session.role_admin),
        money_format: money_format(currency),
        min_split: auth.settings.layout_min_split_width,
        max_width: auth.settings.layout_max_width,
        gap: auth.settings.layout_gap,
        not_a_rule: t.rule_none_on_row().to_string(),
        unsaved_tag: t.row_unsaved().to_string(),
        // The count is filled in by the pane, so the catalog keeps the plural
        // and the template keeps the number.
        drafts_parked: t.drafts_parked(2).replace('2', "{n}"),
        no_pane_that_way: t.no_pane_that_way().to_string(),
        reattribute_title: t.reattribute_private_title("{name}"),
        details_none: t.details_none().to_string(),
        no_files: t.file_no_files().to_string(),
        months_word: t.details_months().to_string(),
        keymap: keymap(&t),
        not_a_command: t.not_a_command("{cmd}"),
        file_not_tracked: t.file_not_tracked().to_string(),
        no_bill: t.file_none_bill().to_string(),
        no_scheduled: t.file_none_scheduled().to_string(),
        no_receipt: t.file_none_receipt().to_string(),
        prev: period.pred().to_string(),
        next: period.succ().to_string(),
        empty: panes.is_empty(),
        panes,
        house: HouseTotals {
            ins: money(ins, currency, any_estimated),
            outs: fmt::money(outs, currency),
            net: money(net.abs(), currency, any_estimated),
            negative: net.is_negative(),
        },
        t,
    })
}

/// The number format the amount mask types into, as JSON.
///
/// Only the three facts a mask needs — how many minor digits, and the two
/// separators. Not the symbol: the mask writes a bare number, and the symbol
/// is the row's job. Built here rather than written into the template so it
/// comes from `Currency::format` like every other money surface.
fn money_format(currency: crate::domain::money::Currency) -> String {
    let f = currency.format();
    format!(
        r#"{{"decimals":{},"decimal":"{}","group":"{}"}}"#,
        f.decimals, f.decimal_sep, f.group_sep
    )
}

/// What the `:` bar offers, in the order a household reaches for it.
///
/// One name per command — `:me` and `:config` still work as aliases, but a
/// hint listing both spellings of one thing is noise.
fn command_hint(admin: bool, holds_admin_role: bool) -> String {
    let mut names = vec!["profile", "sort", "group", "set", "theme", "lang", "grep"];
    if admin {
        names.extend(["household", "members", "invite", "currency"]);
    }
    // `preview` follows the ROLE, not whether its powers are in effect — it
    // is the toggle itself, the same category as the badge, which stays
    // visible so preview is reversible any time. Dropping it while previewing
    // would tell an admin there is no way back. A member who never held the
    // role never sees it, and the route refuses them anyway.
    if holds_admin_role {
        names.push("preview");
    }
    names.push("logout");
    names.join(" · ")
}

/// The keymap, as `?` shows it.
///
/// Lowercase acts on the **occurrence**, uppercase on the **rule or the
/// window** — which is the rule the whole table follows, and the reason it can
/// be read rather than memorised.
fn keymap(t: &Catalog) -> Vec<(String, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            t.keymap_move().to_string(),
            vec![
                ("j / k", "row down / up"),
                ("5j 7k", "counted motion"),
                ("gg / G", "first / last row"),
                ("H / L", "focus pane, left / right"),
                ("gt / gT", "next / previous member tab"),
                ("2gt", "jump straight to tab 2"),
            ],
        ),
        (
            t.keymap_act().to_string(),
            vec![
                ("a / i", "add a one-off spend / income"),
                ("A / I", "create a recurring bill / income"),
                ("e / E", "edit this row / its rule"),
                ("J / K", "move the row down / up"),
                ("Ctrl-H / Ctrl-L", "re-attribute to the pane left / right"),
                ("x", "select, for a batch operation"),
                ("p", "toggle private"),
                ("gd", "due, cadence and receipt details"),
                ("gl", "this row's history"),
            ],
        ),
        (
            t.keymap_find().to_string(),
            vec![
                ("/", "search this month — incremental, regex"),
                ("n / N", "next / previous match"),
                (":grep", "search every month"),
            ],
        ),
        (
            t.keymap_fold().to_string(),
            vec![
                ("za", "toggle the fold under the cursor"),
                ("zo / zc", "open / close it"),
                ("zR / zM", "open / close every fold"),
            ],
        ),
        (
            t.keymap_files().to_string(),
            vec![
                ("g1 / g2 / g3", "open the bill / scheduled / receipt"),
                ("gu1 / gu2 / gu3", "attach a bill / scheduled / receipt"),
                ("P", "preview this row's files"),
                ("1 2 3 · h l", "inside P: jump to a file · step"),
                ("+ - 0", "inside P: zoom in, out, reset"),
            ],
        ),
        (
            t.keymap_undo().to_string(),
            vec![
                ("dd", "delete — opens a confirmation"),
                ("y", "confirm in a modal"),
                ("u", "undo the last change"),
                ("Esc", "clear count, buffer, selection and search"),
            ],
        ),
        (
            t.keymap_type().to_string(),
            vec![
                (":add 50 mercado", "a spend, in one line"),
                (":profile · :household", "your settings · the household's"),
                (":theme · :lang · :sort", "one setting, one row"),
                ("?", "this"),
            ],
        ),
    ]
}

/// The twelve cells of the current year, each carrying whatever is unresolved
/// in it. Clicking one jumps to that month — the picker's existing behaviour,
/// so no new command is needed on top of it.
async fn month_cells(
    st: &AppState,
    auth: &Auth,
    showing: Period,
    today: NaiveDate,
) -> Vec<MonthCell> {
    let t = auth.catalog();
    let unresolved =
        ledger::unresolved_months(&st.pool, auth.viewer, today, Some(&st.cfg.receipts_root))
            .await
            .unwrap_or_default();

    let names: Vec<&str> = t.months_short().split_whitespace().collect();
    (1..=12)
        .filter_map(|m| Period::new(showing.year(), m))
        .map(|p| {
            let count = unresolved
                .iter()
                .find(|u| u.period == p)
                .map(|u| u.count)
                .unwrap_or(0);
            MonthCell {
                key: p.to_string(),
                label: names
                    .get(p.month() as usize - 1)
                    .map(|s| (*s).to_string())
                    .unwrap_or_else(|| p.month().to_string()),
                current: p == showing,
                unresolved: count,
                title: if count > 0 {
                    t.unresolved_dot(count as i64)
                } else {
                    String::new()
                },
            }
        })
        .collect()
}

/// Clusters a pane's rows by `bucket`, in the order the buckets first appear
/// — so a grouped pane still reads in something like the order the household
/// put it in, rather than alphabetically by a word they typed once.
///
/// Nesting stops at `bucket`: one level is enough for a household-sized
/// ledger, and a `bill_name` sub-fold would be a second gesture to learn for
/// a distinction most rows do not make.
pub fn group_rows(
    rows: &[crate::ui::row::LedgerRow],
    views: Vec<RowView>,
    t: &Catalog,
    currency: crate::domain::money::Currency,
) -> Vec<Group> {
    let mut order: Vec<String> = Vec::new();
    let mut buckets: std::collections::HashMap<
        String,
        (crate::domain::money::Cents, Vec<RowView>),
    > = std::collections::HashMap::new();

    for (row, view) in rows.iter().zip(views) {
        let name = match row
            .bucket
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
        {
            Some(b) => b.to_string(),
            // Grouping never silently drops a row.
            None => t.group_uncategorized().to_string(),
        };
        if !buckets.contains_key(&name) {
            order.push(name.clone());
        }
        let entry = buckets
            .entry(name)
            .or_insert((crate::domain::money::Cents::ZERO, Vec::new()));
        // A subtotal is what this bucket costs, so income counts against it —
        // a "Renda" group summing to its own income is the useful reading.
        if let Some(c) = row.effective_cents() {
            if row.kind.is_income() {
                entry.0 += c;
            } else {
                entry.0 -= c;
            }
        }
        entry.1.push(view);
    }

    order
        .into_iter()
        .filter_map(|name| {
            let (total, rows) = buckets.remove(&name)?;
            Some(Group {
                count: t.group_rows(rows.len() as i64),
                subtotal: fmt::money(total.abs(), currency),
                negative: total.is_negative(),
                name,
                rows,
            })
        })
        .collect()
}

fn view(
    pane: Pane,
    t: &Catalog,
    currency: crate::domain::money::Currency,
    grouped: bool,
) -> PaneView {
    let balance = pane.balance();
    PaneView {
        id: pane.member.id,
        name: pane.member.name,
        totals: PaneTotals {
            id: pane.member.id,
            ins: money(pane.ins, currency, pane.estimated),
            outs: fmt::money(pane.outs, currency),
            balance: money(balance.abs(), currency, pane.estimated),
            negative: balance.is_negative(),
        },
        rows: if grouped {
            Vec::new()
        } else {
            pane.rows.iter().map(|r| r.view(t)).collect()
        },
        groups: if grouped {
            let views = pane.rows.iter().map(|r| r.view(t)).collect();
            group_rows(&pane.rows, views, t, currency)
        } else {
            Vec::new()
        },
        grouped,
    }
}
