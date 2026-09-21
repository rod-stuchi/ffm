//! The rule pane — `A` / `I` create, `E` edits.
//!
//! **A column in the ledger's grid, not a modal.** The rule form outgrew a
//! centred dialog, and a dialog hides the very ledger you are editing — the
//! same argument that made `:household` a vsplit. The pane sits right after the
//! member pane it belongs to and pushes later panes right; the ledger scrolls
//! sideways rather than hiding one, because a household can have more than two
//! members.
//!
//! A rule is always created **from scratch**: there is no "turn this row into a
//! recurring one". Saving inserts a new rule and immediately reconciles it, so
//! the periods it is responsible for appear in the same response.

use crate::db::changes::Actor;
use crate::db::ledger;
use crate::db::rules::{self, RuleForm};
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::slug::slugify;
use crate::domain::status::{TxKind, Visibility};
use crate::services::materializer;
use crate::state::AppState;
use crate::ui::i18n::Catalog;
use crate::ui::row::RowView;
use crate::web::ledger::{HouseTotals, PaneTotals};
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Form, Path, Query, State};
use axum::response::{IntoResponse, Response};
use chrono::{Local, NaiveDate};
use serde::Deserialize;

/// The cadences the form offers. Only the list is real so far: the
/// installments/end-date sync computes in whole months, which every one of
/// these is, but a household that wants fortnightly has nowhere to put it.
const CADENCES: [(i64, &str); 6] = [
    (1, "monthly"),
    (2, "bimonthly"),
    (3, "quarterly"),
    (4, "four"),
    (6, "half"),
    (12, "yearly"),
];

pub struct CadenceChoice {
    pub months: i64,
    pub label: String,
    pub selected: bool,
}

#[derive(Template)]
#[template(path = "partials/rule_pane.html")]
pub struct RulePane {
    t: Catalog,
    /// `None` when creating — the form posts to a different place, and there
    /// is no Delete for a rule that does not exist yet.
    id: Option<i64>,
    member_id: i64,
    kind: &'static str,
    income: bool,
    title: String,
    amount: String,
    bucket: String,
    bill_name: String,
    instance: String,
    buckets: Vec<String>,
    cadences: Vec<CadenceChoice>,
    start: String,
    due_day: String,
    end: String,
    installments: String,
    tracked: bool,
    no_barcode: bool,
    carry_forward: bool,
    varies: bool,
    estimate_basis: String,
    estimate_window: i64,
    amount_in_filename: bool,
    private: bool,
    /// The member's vault folder, so the computed path shown in the pane is
    /// the real one rather than a prefix-less approximation.
    vault: String,
}

fn cadences(t: &Catalog, selected: i64) -> Vec<CadenceChoice> {
    CADENCES
        .iter()
        .map(|(months, key)| CadenceChoice {
            months: *months,
            label: match *key {
                "monthly" => t.cadence_monthly(),
                "bimonthly" => t.cadence_bimonthly(),
                "quarterly" => t.cadence_quarterly(),
                "four" => t.cadence_four(),
                "half" => t.cadence_half(),
                _ => t.cadence_yearly(),
            }
            .to_string(),
            selected: *months == selected,
        })
        .collect()
}

#[derive(Deserialize)]
pub struct NewParams {
    member_id: i64,
    #[serde(default)]
    kind: String,
}

/// `A` and `I`. A blank editor with no anchor row at all — the Bucket starts
/// empty on purpose, because a fresh rule deserves an explicit choice rather
/// than an inherited guess.
pub async fn new_rule(
    State(st): State<AppState>,
    auth: Auth,
    Query(p): Query<NewParams>,
) -> Response {
    let kind = TxKind::parse(&p.kind).unwrap_or(TxKind::Out);
    let t = auth.catalog();
    let today = Local::now().date_naive();

    Html(RulePane {
        id: None,
        member_id: p.member_id,
        kind: kind.code(),
        income: kind.is_income(),
        title: String::new(),
        amount: String::new(),
        bucket: String::new(),
        bill_name: String::new(),
        instance: String::new(),
        buckets: rules::buckets(&st.pool, auth.viewer.member_id)
            .await
            .unwrap_or_default(),
        cadences: cadences(&t, 1),
        start: today.format("%Y-%m-%d").to_string(),
        due_day: String::new(),
        end: String::new(),
        installments: String::new(),
        tracked: false,
        no_barcode: false,
        // Checked by default: most bills drift slowly, and a household that
        // wants an explicit number every period says so.
        carry_forward: true,
        varies: false,
        estimate_basis: "lowest".into(),
        estimate_window: 3,
        amount_in_filename: false,
        private: false,
        vault: vault_of(&st, p.member_id).await,
        t,
    })
    .into_response()
}

/// `E`. A no-op with a statusline message on a plain row — the same treatment
/// `gd` gets there, rather than a different error path to learn.
pub async fn edit_rule(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    let Ok(Some(r)) = rules::get(&st.pool, id).await else {
        return crate::web::tx::say_only(t.rule_none_on_row(), true);
    };
    if !auth.viewer.sees_private_of(r.member_id) && r.visibility == Visibility::Private {
        return crate::web::tx::say_only(t.rule_none_on_row(), true);
    }

    Html(RulePane {
        id: Some(r.id),
        member_id: r.member_id,
        kind: r.kind.code(),
        income: r.kind.is_income(),
        title: r.title.clone(),
        amount: r
            .amount
            .map(|c| format!("{}", c.get() as f64 / 100.0))
            .unwrap_or_default(),
        bucket: r.bucket.clone().unwrap_or_default(),
        bill_name: r.bill_name.clone().unwrap_or_default(),
        instance: r.instance.clone().unwrap_or_default(),
        buckets: rules::buckets(&st.pool, auth.viewer.member_id)
            .await
            .unwrap_or_default(),
        cadences: cadences(&t, r.cadence_months),
        start: r.start_date.format("%Y-%m-%d").to_string(),
        due_day: r.due_day.map(|d| d.to_string()).unwrap_or_default(),
        end: r
            .end_date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
        installments: r
            .total_installments
            .map(|n| n.to_string())
            .unwrap_or_default(),
        tracked: r.tracked,
        no_barcode: r.no_barcode,
        carry_forward: r.carry_forward_amount,
        varies: r.varies,
        estimate_basis: r.estimate_basis.clone(),
        estimate_window: r.estimate_window,
        amount_in_filename: r.amount_in_filename,
        private: r.visibility == Visibility::Private,
        vault: vault_of(&st, r.member_id).await,
        t,
    })
    .into_response()
}

async fn vault_of(st: &AppState, member_id: i64) -> String {
    ledger::members(&st.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.id == member_id)
        .map(|m| format!("{}-{}", m.uuid, m.slug))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// saving
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RuleFormInput {
    member_id: i64,
    kind: String,
    title: String,
    #[serde(default)]
    amount: String,
    #[serde(default)]
    bucket: String,
    #[serde(default)]
    bill_name: String,
    #[serde(default)]
    instance: String,
    cadence_months: i64,
    start: String,
    #[serde(default)]
    due_day: String,
    #[serde(default)]
    end: String,
    #[serde(default)]
    installments: String,
    #[serde(default)]
    tracked: Option<String>,
    #[serde(default)]
    no_barcode: Option<String>,
    #[serde(default)]
    carry_forward: Option<String>,
    #[serde(default)]
    varies: Option<String>,
    #[serde(default)]
    estimate_basis: String,
    #[serde(default)]
    estimate_window: Option<i64>,
    #[serde(default)]
    amount_in_filename: Option<String>,
    #[serde(default)]
    private: Option<String>,
    period: String,
}

/// An unchecked checkbox sends nothing at all, so presence is the value.
fn on(v: &Option<String>) -> bool {
    v.is_some()
}

fn some_trimmed(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

impl RuleFormInput {
    fn to_form(&self) -> Option<RuleForm> {
        let kind = TxKind::parse(&self.kind)?;
        let start = NaiveDate::parse_from_str(&self.start, "%Y-%m-%d").ok()?;
        let installments = self
            .installments
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|n| *n > 0);
        let end = NaiveDate::parse_from_str(self.end.trim(), "%Y-%m-%d")
            .ok()
            // The two stay in sync, and the installment count wins when both
            // are present: it is the number a household actually knows about a
            // loan, and the date falls out of it.
            .or_else(|| {
                installments
                    .and_then(|n| rules::end_from_installments(start, self.cadence_months, n))
            });
        let tracked = on(&self.tracked);

        Some(RuleForm {
            member_id: self.member_id,
            kind,
            title: self.title.trim().to_string(),
            amount: Cents::parse(&self.amount),
            cadence_months: self.cadence_months,
            due_day: self
                .due_day
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|d| (1..=31).contains(d)),
            start_date: start,
            end_date: end,
            total_installments: installments,
            carry_forward_amount: on(&self.carry_forward),
            tracked,
            bucket: some_trimmed(&self.bucket),
            // Left empty it falls back to the title: for most bills the row's
            // name and the folder's name are the same word, and making
            // somebody type it twice to file a receipt is friction with no
            // decision behind it.
            bill_name: some_trimmed(&self.bill_name)
                .or_else(|| tracked.then(|| self.title.trim().to_string())),
            // `instance` only ever existed to disambiguate a receipt path, so
            // an untracked rule has no use for one.
            instance: tracked.then(|| some_trimmed(&self.instance)).flatten(),
            // The whole barcode apparatus is spend-only: money arriving has no
            // boleto to scan, and the schema has a CHECK saying so.
            no_barcode: !kind.is_income() && on(&self.no_barcode),
            varies: kind.is_income() && on(&self.varies),
            estimate_basis: match self.estimate_basis.as_str() {
                "average" | "last" => self.estimate_basis.clone(),
                _ => "lowest".into(),
            },
            estimate_window: self.estimate_window.unwrap_or(3).clamp(1, 12),
            amount_in_filename: tracked && on(&self.amount_in_filename),
            visibility: if on(&self.private) {
                Visibility::Private
            } else {
                Visibility::Household
            },
        })
    }
}

/// What a save sends back: the affected pane's rows, its totals, the
/// household's, and the statusline. The same shape a row mutation uses, one
/// level up — a rule save can create several rows at once, so it returns the
/// pane rather than a row.
pub async fn create(
    State(st): State<AppState>,
    auth: Auth,
    Form(input): Form<RuleFormInput>,
) -> Response {
    let t = auth.catalog();
    let Some(form) = input.to_form() else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    if form.title.is_empty() {
        return crate::web::tx::say_only(&t.field_required(t.row_title()), true);
    }
    // A tracked rule needs the two path segments it files under — the schema
    // has a CHECK, but saying so here means the person sees a message rather
    // than a constraint violation.
    if form.tracked && (form.bucket.is_none() || form.bill_name.is_none()) {
        return crate::web::tx::say_only(&t.field_required(t.rule_bucket()), true);
    }

    match rules::create(&st.pool, Actor::User(auth.viewer.member_id), &form).await {
        Ok(_) => {
            finish(
                &st,
                &auth,
                input.member_id,
                &input.period,
                t.rule_created(0),
            )
            .await
        }
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

pub async fn save(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<i64>,
    Form(input): Form<RuleFormInput>,
) -> Response {
    let t = auth.catalog();
    let Some(form) = input.to_form() else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    match rules::edit(&st.pool, Actor::User(auth.viewer.member_id), id, &form).await {
        Ok(true) => {
            finish(
                &st,
                &auth,
                input.member_id,
                &input.period,
                t.rule_saved().to_string(),
            )
            .await
        }
        _ => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

/// Deleting a rule stops future materialization. The occurrences it already
/// made stay: they are what happened.
pub async fn delete(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    let Ok(Some(r)) = rules::get(&st.pool, id).await else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    let period = Period::of(Local::now().date_naive()).to_string();
    match rules::soft_delete(&st.pool, Actor::User(auth.viewer.member_id), id).await {
        Ok(true) => {
            finish(
                &st,
                &auth,
                r.member_id,
                &period,
                t.rule_deleted().to_string(),
            )
            .await
        }
        _ => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

/// Reconcile, then re-render the pane whose rows just changed.
///
/// Materializing here rather than waiting for the hourly tick is what makes
/// "create a rule" feel like one action: the periods it is responsible for
/// appear in the same response. The tick is still what catches everything
/// else.
async fn finish(
    st: &AppState,
    auth: &Auth,
    member_id: i64,
    period: &str,
    message: String,
) -> Response {
    let today = Local::now().date_naive();
    let created = materializer::reconcile(&st.pool, today)
        .await
        .map(|r| r.created)
        .unwrap_or(0);

    let Some(period) = Period::parse(period) else {
        return crate::web::tx::say_only(&message, false);
    };

    let message = if created > 0 {
        auth.catalog().rule_created(created as i64)
    } else {
        message
    };

    crate::web::tx::pane_fragment(st, auth, member_id, period, &message).await
}

// ---------------------------------------------------------------------------
// the pane's rows, as a fragment
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "partials/pane_rows.html")]
pub struct PaneRows {
    pub t: Catalog,
    pub member_id: i64,
    pub rows: Vec<RowView>,
    pub pane_totals: Option<PaneTotals>,
    pub house_totals: Option<HouseTotals>,
}

/// The receipt path a tracked rule computes, for the pane's live preview.
///
/// Rendered server-side on open and recomputed in the pane as somebody types,
/// because renaming taxonomy has to show where the files will move *before*
/// the rule is saved.
pub fn computed_path(
    vault: &str,
    bucket: &str,
    bill_name: &str,
    instance: &str,
    period: Period,
) -> String {
    let mut parts = vec![vault.to_string(), slugify(bucket), slugify(bill_name)];
    if !instance.trim().is_empty() {
        parts.push(slugify(instance));
    }
    parts.push(period.path_year());
    format!("{}/{}.pdf", parts.join("/"), period.path_month())
}

/// Never used by a handler — the pane's JavaScript recomputes the same string
/// as you type. It exists so the rule is expressed once, in Rust, and the
/// template's version can be checked against it.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_computed_path_is_slugified_all_the_way_down() {
        let p = Period::parse("2026-09").unwrap();
        assert_eq!(
            computed_path("abc-rodrigo", "Consumo", "Energia Elétrica", "", p),
            "abc-rodrigo/consumo/energia_elétrica/2026/09.pdf"
        );
        // The instance segment is ABSENT when there is none, not empty — "a
        // distinction that does not exist yet needs no name".
        assert!(!computed_path("v", "a", "b", "", p).contains("//"));
        assert_eq!(
            computed_path("v", "Casa", "Condomínio", "House 33A", p),
            "v/casa/condomínio/house_33a/2026/09.pdf"
        );
    }
}
