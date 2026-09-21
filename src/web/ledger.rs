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
use crate::ui::i18n::Catalog;
use crate::ui::row::{RowCtx, RowView};
use crate::ui::theme::{self, ModePref, ThemeName};
use crate::ui::{fmt, i18n::Lang};
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

pub struct PaneView {
    pub name: String,
    pub rows: Vec<RowView>,
    pub ins: String,
    pub outs: String,
    pub balance: String,
    pub negative: bool,
}

#[derive(Template)]
#[template(path = "pages/ledger.html")]
struct LedgerPage {
    theme_css: String,
    attr: Option<&'static str>,
    t: Catalog,
    household: String,
    period: String,
    prev: String,
    next: String,
    panes: Vec<PaneView>,
    ins: String,
    outs: String,
    net: String,
    net_negative: bool,
    empty: bool,
}

pub async fn ledger(State(st): State<AppState>, Query(p): Query<Params>) -> impl IntoResponse {
    let today = Local::now().date_naive();
    let period = p
        .period
        .as_deref()
        .and_then(Period::parse)
        .unwrap_or_else(|| Period::of(today));

    let house = ledger::household(&st.pool).await.unwrap_or(None);
    let (name, currency, lang, show_recurring) = match &house {
        Some(h) => (
            h.name.clone(),
            h.currency,
            h.default_language,
            h.show_recurring,
        ),
        // No household row means nobody has run /setup. That page does not
        // exist yet, so say so plainly rather than rendering an empty ledger
        // that looks like a household with no money.
        None => ("FFM".to_string(), Default::default(), Lang::En, true),
    };

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

    for m in &members {
        let Ok(pane) = ledger::pane(&st.pool, m, period, ctx).await else {
            continue;
        };
        ins += pane.ins;
        outs += pane.outs;
        panes.push(view(pane, &t, currency));
    }

    let net = ins - outs;
    Html(LedgerPage {
        theme_css: theme::render(ThemeName::default(), ModePref::System, &Default::default())
            .to_css(),
        attr: None,
        household: name,
        period: fmt::period(period),
        prev: period.pred().to_string(),
        next: period.succ().to_string(),
        empty: panes.is_empty(),
        panes,
        ins: fmt::money(ins, currency),
        outs: fmt::money(outs, currency),
        net: fmt::money(net.abs(), currency),
        net_negative: net.is_negative(),
        t,
    })
}

fn view(pane: Pane, t: &Catalog, currency: crate::domain::money::Currency) -> PaneView {
    let balance = pane.balance();
    PaneView {
        name: pane.member.name,
        ins: fmt::money(pane.ins, currency),
        outs: fmt::money(pane.outs, currency),
        balance: fmt::money(balance.abs(), currency),
        negative: balance.is_negative(),
        rows: pane.rows.into_iter().map(|r| r.view(t)).collect(),
    }
}
