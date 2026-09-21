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
use crate::ui::row::{RowCtx, RowView};
use crate::ui::theme;
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use chrono::Local;
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
        currency,
        show_recurring,
        show_delta: true,
        narrow: false,
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

pub struct PaneView {
    /// Sent with every add, because which pane is focused is data — a
    /// server-side idea of "current member" drifts the moment the app is open
    /// in two windows.
    pub id: i64,
    pub name: String,
    pub rows: Vec<RowView>,
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
    /// `E` on a row with no rule behind it says so in the statusline rather
    /// than doing nothing at all.
    not_a_rule: String,
    /// The three "there is no such file" messages — `g1`/`g2`/`g3` on a row
    /// that has not got one. Three fields rather than a JSON blob, so each is
    /// escaped by the same filter as every other string handed to the page.
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
        currency,
        show_recurring,
        show_delta: true,
        narrow: false,
    };
    let t = Catalog::new(lang);

    let members = ledger::members(&st.pool).await.unwrap_or_default();
    let mut panes = Vec::new();
    let mut ins = crate::domain::money::Cents::ZERO;
    let mut outs = crate::domain::money::Cents::ZERO;
    let mut any_estimated = false;

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
        panes.push(view(pane, &t, currency));
    }

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
        not_a_rule: t.rule_none_on_row().to_string(),
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

fn view(pane: Pane, t: &Catalog, currency: crate::domain::money::Currency) -> PaneView {
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
        rows: pane.rows.into_iter().map(|r| r.view(t)).collect(),
    }
}
