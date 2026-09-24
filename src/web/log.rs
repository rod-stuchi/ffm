//! `gl` — the change log for one row, in a side pane.
//!
//! The same 420px column, arrow and cursor-follows behavior the rule editor
//! uses, so nothing new has to be learned to read it. **Read-only**: no
//! drafts, no save, nothing here writes.
//!
//! A row with nothing recorded still opens the pane, saying so. An empty
//! history is an answer, not an error worth refusing on.

use crate::db::changes::{self, Filter};
use crate::db::ledger;
use crate::state::AppState;
use crate::ui::i18n::Catalog;
use crate::ui::log::{self as render, DayGroup};
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use chrono::Local;
use serde::Deserialize;

#[derive(Template)]
#[template(path = "partials/log_pane.html")]
struct LogPane {
    t: Catalog,
    title: String,
    period: String,
    days: Vec<DayGroup>,
    filter: &'static str,
    /// Absent on a row with no rule, so the `rule` chip is not offered for
    /// something that cannot have any — the same reasoning that makes `E` a
    /// no-op there rather than an error.
    has_rule: bool,
    row_id: i64,
}

#[derive(Deserialize)]
pub struct Params {
    #[serde(default)]
    filter: String,
}

pub async fn log(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<i64>,
    Query(p): Query<Params>,
) -> Response {
    let t = auth.catalog();
    let row = sqlx::query!(
        r#"SELECT t.title AS "title!: String", t.period AS "period!: String",
                  t.member_id AS "member_id!: i64", t.rule_id AS "rule_id: i64",
                  t.visibility AS "visibility!: String"
           FROM transactions t WHERE t.id = ?1"#,
        id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten();

    let Some(row) = row else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    // A private row you cannot see has no history you may read: its absence
    // from your ledger would otherwise be contradicted by a pane full of it.
    if row.visibility == "private" && !auth.viewer.sees_private_of(row.member_id) {
        return crate::web::tx::say_only(t.error_not_written(), true);
    }

    // File entries name a `receipts` row rather than the transaction, so the
    // ids that belong to this occurrence are gathered here and passed in.
    let receipt_ids: Vec<i64> = sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM receipts WHERE transaction_id = ?1"#,
        id
    )
    .fetch_all(&st.pool)
    .await
    .unwrap_or_default();

    let filter = Filter::parse(&p.filter);
    let entries = changes::history(&st.pool, id, row.rule_id, filter, &receipt_ids)
        .await
        .unwrap_or_default();

    let currency = ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();

    Html(LogPane {
        title: row.title,
        period: crate::domain::period::Period::parse(&row.period)
            .map(crate::ui::fmt::period)
            .unwrap_or(row.period),
        days: render::render(&entries, &t, currency, Local::now().date_naive()),
        filter: filter.code(),
        has_rule: row.rule_id.is_some(),
        row_id: id,
        t,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// `:grep`
// ---------------------------------------------------------------------------

pub struct HitView {
    pub period: String,
    pub label: String,
    pub member: String,
    pub title: String,
    pub amount: String,
    pub is_income: bool,
}

#[derive(Template)]
#[template(path = "partials/grep_pane.html")]
struct GrepPane {
    t: Catalog,
    term: String,
    count: String,
    hits: Vec<HitView>,
}

#[derive(Deserialize)]
pub struct GrepParams {
    #[serde(default)]
    q: String,
}

pub async fn grep(State(st): State<AppState>, auth: Auth, Query(p): Query<GrepParams>) -> Response {
    let t = auth.catalog();
    let currency = ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();

    let hits = ledger::grep(&st.pool, auth.viewer, &p.q)
        .await
        .unwrap_or_default();

    Html(GrepPane {
        count: t.grep_count(hits.len() as i64),
        hits: hits
            .into_iter()
            .map(|h| HitView {
                period: h.period.to_string(),
                label: crate::ui::fmt::period(h.period),
                member: h.member,
                title: h.title,
                amount: h
                    .amount
                    .map(|c| crate::ui::fmt::money(c, currency))
                    .unwrap_or_else(|| crate::ui::fmt::money_placeholder(currency)),
                is_income: h.is_income,
            })
            .collect(),
        term: p.q,
        t,
    })
    .into_response()
}
