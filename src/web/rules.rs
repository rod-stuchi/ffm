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
use crate::domain::months::{Months, MonthsError};
use crate::domain::period::Period;
use crate::domain::slug::slugify;
use crate::domain::status::{TxKind, Visibility};
use crate::services::materializer;
use crate::state::AppState;
use crate::ui::i18n::Catalog;
use crate::ui::row::RowView;
use crate::web::form::FieldErrors;
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
    /// The stored integer the mask seeds from — see EditRow for why the
    /// formatted string is never parsed back.
    amount_cents: i64,
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
    spans_period: bool,
    window_size: String,
    window_start: String,
    carry_forward: bool,
    varies: bool,
    estimate_basis: String,
    estimate_window: i64,
    amount_in_filename: bool,
    /// `true` when the rule skips some months. Separate from the expression
    /// so the field can be revealed without inventing a value for it: a
    /// seasonal switch that is off means `months = NULL`, which is a
    /// different fact from "runs in all twelve".
    seasonal: bool,
    /// The canonical expression (`1-10`), not the stored list — ten numbers
    /// would make editing a seasonal rule a re-reading exercise every time.
    months: String,
    /// Rendered server-side at open, then re-rendered by `hx-get` as the
    /// field is typed in. Never computed in JS: the whole point of the read
    /// back is that it agrees with the parser that will actually store it,
    /// which is the same argument the theme picker's server-rendered preview
    /// already makes.
    months_readback: MonthsReadback,
    private: bool,
    /// The member's vault folder, so the computed path shown in the pane is
    /// the real one rather than a prefix-less approximation.
    vault: String,
    /// Shown read-only once a bill has been attached; absent until then — the
    /// same "absent, not a blank field" treatment `instance` gets when a rule
    /// does not need one.
    client_code: Option<String>,
    /// One ready-made projection per basis: `(basis, "~R$ 5.033,33",
    /// "08/2026 · 07/2026 · 06/2026")`.
    ///
    /// Computed and **formatted server-side**, by the same `domain::estimate`
    /// and `ui::fmt::money` the ledger uses. The pane only chooses which one
    /// to show as the radio changes — it never does the arithmetic, and it
    /// certainly never formats money, which would put the household's currency
    /// convention in a second place to drift.
    estimates: Vec<(String, String, String)>,
    /// What the estimate box says with nothing behind it yet.
    no_history: String,
    /// Empty on a fresh open. On a rejected save the pane comes back with the
    /// submitted values still in it and the messages beside their fields.
    errors: FieldErrors,
}

/// The live read-back under the months field: which months it resolved to,
/// and a sentence saying so.
///
/// Rendered by the server on every keystroke, through `hx-get`. That is the
/// same choice the theme picker makes and for the same reason — a preview
/// computed in JavaScript is a second implementation of the parser, free to
/// disagree with the one that actually stores the value, and this one decides
/// which months a bill exists in.
#[derive(Template)]
#[template(path = "partials/months_readback.html")]
pub struct MonthsReadback {
    /// One entry per calendar month: its initial, and whether it is on.
    marks: Vec<(&'static str, bool)>,
    /// The sentence: "ten months a year · Nov and Dec skipped", or the parse
    /// error, or "every month".
    line: String,
    /// Colours the line as a refusal rather than a description.
    bad: bool,
}

const INITIALS: [&str; 12] = ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"];

impl MonthsReadback {
    /// What an unparsed or empty expression reads as: every month, nothing
    /// skipped. Not an error — a rule that runs every month is the ordinary
    /// case, and the seasonal switch being off is how you say it.
    pub fn every_month(t: &Catalog) -> MonthsReadback {
        MonthsReadback {
            marks: INITIALS.iter().map(|i| (*i, true)).collect(),
            line: t.months_every().to_string(),
            bad: false,
        }
    }

    pub fn of(t: &Catalog, expr: &str) -> MonthsReadback {
        if expr.trim().is_empty() {
            return MonthsReadback::every_month(t);
        }
        match Months::parse(expr) {
            Ok(m) => {
                let skipped = m.skipped();
                MonthsReadback {
                    marks: INITIALS
                        .iter()
                        .enumerate()
                        .map(|(i, ini)| (*ini, m.contains(i as u32 + 1)))
                        .collect(),
                    line: if skipped.is_empty() {
                        t.months_every().to_string()
                    } else {
                        t.months_count_skipped(m.len() as i64, &month_names(t, &skipped).join(", "))
                    },
                    bad: false,
                }
            }
            // A bad expression shows NO months lit rather than the last good
            // shape: leaving the previous answer on screen under a rejected
            // expression is the read-back agreeing with something nobody
            // typed.
            Err(e) => MonthsReadback {
                marks: INITIALS.iter().map(|i| (*i, false)).collect(),
                line: months_error(t, &e),
                bad: true,
            },
        }
    }
}

/// The month names, from the catalog — the one place in this feature where
/// language reaches the output at all. The expression itself is digits, and
/// stays digits in every locale.
fn month_names(t: &Catalog, months: &[u32]) -> Vec<String> {
    months
        .iter()
        .map(|m| t.month_short(*m as i64).to_string())
        .collect()
}

fn months_error(t: &Catalog, e: &MonthsError) -> String {
    match e {
        MonthsError::Empty => t.months_err_empty().to_string(),
        MonthsError::NotANumber(tok) => t.months_err_not_a_number(tok),
        MonthsError::OutOfRange(n) => t.months_err_out_of_range(*n as i64),
        MonthsError::BackwardsRange(a, b) => t.months_err_backwards(&format!("{a}-{b}")),
        MonthsError::NothingLeft => t.months_err_nothing_left().to_string(),
    }
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
        amount_cents: 0,
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
        spans_period: false,
        window_size: String::new(),
        window_start: String::new(),
        // Checked by default: most bills drift slowly, and a household that
        // wants an explicit number every period says so.
        carry_forward: true,
        varies: false,
        estimate_basis: "lowest".into(),
        estimate_window: 3,
        amount_in_filename: false,
        // A new rule runs every month until somebody says otherwise: seasonal
        // is the unusual shape, and `months = NULL` is how "no opinion" is
        // stored rather than a list of all twelve.
        seasonal: false,
        months: String::new(),
        months_readback: MonthsReadback::every_month(&t),
        private: false,
        vault: vault_of(&st, p.member_id).await,
        client_code: None,
        // A brand-new rule has no history to project from; the box says so
        // rather than showing a number with nothing behind it.
        estimates: Vec::new(),
        no_history: t.estimate_no_history().to_string(),
        errors: FieldErrors::new(),
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
        amount_cents: r.amount.map(|c| c.get()).unwrap_or_default(),
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
        spans_period: r.spans_period,
        window_size: r.window_size.map(|n| n.to_string()).unwrap_or_default(),
        window_start: r.window_start.map(|p| p.to_string()).unwrap_or_default(),
        carry_forward: r.carry_forward_amount,
        varies: r.varies,
        estimate_basis: r.estimate_basis.clone(),
        estimate_window: r.estimate_window,
        amount_in_filename: r.amount_in_filename,
        seasonal: r.months.is_some(),
        months: r
            .months
            .as_ref()
            .map(|m| m.to_expression())
            .unwrap_or_default(),
        months_readback: match &r.months {
            Some(m) => MonthsReadback::of(&t, &m.to_expression()),
            None => MonthsReadback::every_month(&t),
        },
        private: r.visibility == Visibility::Private,
        vault: vault_of(&st, r.member_id).await,
        client_code: r.client_code.clone(),
        estimates: estimates_of(&st, &auth, &r).await,
        no_history: t.estimate_no_history().to_string(),
        errors: FieldErrors::new(),
        t,
    })
    .into_response()
}

/// The confirmed periods behind a varying income, formatted for the pane.
///
/// Read through the same pane query the ledger uses, so "confirmed" means
/// exactly what it means everywhere else: a payslip on disk for a tracked
/// rule, the manual tick for an untracked one.
async fn estimates_of(
    st: &AppState,
    auth: &Auth,
    rule: &rules::Rule,
) -> Vec<(String, String, String)> {
    if !rule.varies {
        return Vec::new();
    }
    let Ok(members) = ledger::members(&st.pool).await else {
        return Vec::new();
    };
    let Some(member) = members.into_iter().find(|m| m.id == rule.member_id) else {
        return Vec::new();
    };
    let house = ledger::household(&st.pool).await.ok().flatten();
    let currency = house.map(|h| h.currency).unwrap_or_default();

    let today = Local::now().date_naive();
    let mut history: Vec<crate::domain::estimate::Confirmed> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    // A year back is enough to fill any window the form offers, and bounded —
    // the same instinct as the unresolved badge's calendar-year lookback.
    let mut period = Period::of(today);
    for _ in 0..12 {
        period = period.pred();
        let ctx = crate::ui::row::RowCtx {
            today,
            sort: Default::default(),
            currency,
            show_recurring: true,
            show_delta: false,
            narrow: false,
            // Nothing here is rendered — this walks back through months to
            // find confirmed amounts for the estimate box — so the gutter
            // never reaches a screen and the default will do.
            line_numbers: Default::default(),
        };
        let Ok(pane) = ledger::pane_with_vault(
            &st.pool,
            &member,
            period,
            ctx,
            auth.viewer,
            Some(&st.cfg.receipts_root),
        )
        .await
        else {
            continue;
        };
        for row in pane.rows.iter().filter(|r| r.rule_id == Some(rule.id)) {
            if let (true, Some(cents)) = (row.is_confirmed(), row.amount_cents) {
                history.push(crate::domain::estimate::Confirmed {
                    period: period.to_string(),
                    amount: cents,
                });
                labels.push(crate::ui::fmt::period(period));
            }
        }
    }

    let window = rule.estimate_window.clamp(1, 12) as usize;
    let from = labels
        .iter()
        .take(window)
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ");

    [
        crate::domain::estimate::Basis::Lowest,
        crate::domain::estimate::Basis::Average,
        crate::domain::estimate::Basis::Last,
    ]
    .into_iter()
    .filter_map(|basis| {
        let p = crate::domain::estimate::project(&history, basis, window, rule.amount)?;
        Some((
            basis.code().to_string(),
            crate::ui::fmt::money_estimated(p.amount, currency),
            from.clone(),
        ))
    })
    .collect()
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
    spans_period: Option<String>,
    #[serde(default)]
    window_size: Option<i64>,
    #[serde(default)]
    window_start: String,
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
    #[serde(default)]
    seasonal: Option<String>,
    #[serde(default)]
    months: String,
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
    /// Validates the whole submission at once and reports **every** field
    /// that failed, not just the first. A form that rejects one box per round
    /// trip makes somebody submit four times to find out four things.
    ///
    /// The order of the checks is the order the fields sit on the form, which
    /// is what makes the summary read top to bottom and `focus()` land on the
    /// first box somebody would reach.
    fn to_form(&self, t: &Catalog) -> Result<RuleForm, FieldErrors> {
        let mut errors = FieldErrors::new();

        let kind = TxKind::parse(&self.kind).unwrap_or(TxKind::Out);
        let start = match NaiveDate::parse_from_str(&self.start, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => {
                errors.add("start", t.field_required(t.rule_start()));
                // Nothing downstream can be checked without it — the end
                // date and the installment count are both derived from it —
                // so this is the one field that stops the pass.
                return Err(errors);
            }
        };

        if self.title.trim().is_empty() {
            errors.add("title", t.field_required(t.row_title()));
        }

        let tracked_now = on(&self.tracked);
        if tracked_now && some_trimmed(&self.bucket).is_none() {
            // The schema has a CHECK for this. Saying it here means somebody
            // sees a message beside the box rather than a constraint
            // violation in the statusline.
            errors.add("bucket", t.field_required(t.rule_bucket()));
        }

        // Seasonal months. An unparseable expression is a field error, never
        // a silently-ignored value: a rule that quietly kept running every
        // month because its expression did not compile is the worst outcome
        // here — nothing on screen would say so.
        let months = match (on(&self.seasonal), self.months.trim()) {
            (false, _) => None,
            (true, "") => {
                errors.add("months", t.field_required(t.months_label()));
                None
            }
            (true, expr) => match Months::parse(expr) {
                Ok(m) => Some(m),
                Err(e) => {
                    errors.add("months", months_error(t, &e));
                    None
                }
            },
        };

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
        let tracked = tracked_now;

        if errors.is_bad() {
            return Err(errors);
        }

        Ok(RuleForm {
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
            // A window belongs to a tracked SPEND with a barcode: the schema
            // has a CHECK for each of those, and building the form value the
            // same way means a contradictory rule never reaches it.
            spans_period: tracked
                && !kind.is_income()
                && !on(&self.no_barcode)
                && on(&self.spans_period)
                && self.window_size.is_some()
                && !self.window_start.trim().is_empty(),
            window_size: self.window_size.filter(|n| (1..=120).contains(n)),
            window_start: Period::parse(self.window_start.trim()),
            varies: kind.is_income() && on(&self.varies),
            estimate_basis: match self.estimate_basis.as_str() {
                "average" | "last" => self.estimate_basis.clone(),
                _ => "lowest".into(),
            },
            estimate_window: self.estimate_window.unwrap_or(3).clamp(1, 12),
            amount_in_filename: tracked && on(&self.amount_in_filename),
            months,
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
    let form = match input.to_form(&t) {
        Ok(f) => f,
        Err(errors) => return rejected(&st, &auth, &input, None, errors).await,
    };

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
    let form = match input.to_form(&t) {
        Ok(f) => f,
        Err(errors) => return rejected(&st, &auth, &input, Some(id), errors).await,
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

/// A rejected save comes back as **the pane it was submitted from**, with
/// every value still in it and the messages beside their fields.
///
/// Not the statusline, which is where this used to go: a one-line refusal at
/// the bottom of the screen cannot say which of a dozen boxes is wrong, and
/// re-rendering from the database would throw away everything typed.
async fn rejected(
    st: &AppState,
    auth: &Auth,
    input: &RuleFormInput,
    id: Option<i64>,
    errors: FieldErrors,
) -> Response {
    let t = auth.catalog();
    let kind = TxKind::parse(&input.kind).unwrap_or(TxKind::Out);
    let cadence = input.cadence_months;
    // The read-back follows the expression that was rejected, so the months
    // box and the message under it describe the same text.
    let months_readback = if on(&input.seasonal) {
        MonthsReadback::of(&t, &input.months)
    } else {
        MonthsReadback::every_month(&t)
    };

    let rule = match id {
        Some(id) => rules::get(&st.pool, id).await.ok().flatten(),
        None => None,
    };

    let mut res = Html(RulePane {
        id,
        member_id: input.member_id,
        kind: kind.code(),
        income: kind.is_income(),
        title: input.title.clone(),
        amount: input.amount.clone(),
        amount_cents: Cents::parse(&input.amount)
            .map(|c| c.get())
            .unwrap_or_default(),
        bucket: input.bucket.clone(),
        bill_name: input.bill_name.clone(),
        instance: input.instance.clone(),
        buckets: rules::buckets(&st.pool, auth.viewer.member_id)
            .await
            .unwrap_or_default(),
        cadences: cadences(&t, cadence),
        start: input.start.clone(),
        due_day: input.due_day.clone(),
        end: input.end.clone(),
        installments: input.installments.clone(),
        tracked: on(&input.tracked),
        no_barcode: on(&input.no_barcode),
        spans_period: on(&input.spans_period),
        window_size: input.window_size.map(|n| n.to_string()).unwrap_or_default(),
        window_start: input.window_start.clone(),
        carry_forward: on(&input.carry_forward),
        varies: on(&input.varies),
        estimate_basis: if input.estimate_basis.is_empty() {
            "lowest".into()
        } else {
            input.estimate_basis.clone()
        },
        estimate_window: input.estimate_window.unwrap_or(3),
        amount_in_filename: on(&input.amount_in_filename),
        seasonal: on(&input.seasonal),
        months: input.months.clone(),
        months_readback,
        private: on(&input.private),
        vault: vault_of(st, input.member_id).await,
        // These two are facts about the stored rule, not about the
        // submission, so they come from the database even here — a client
        // code cannot be typed, and the estimates are computed from history.
        client_code: rule.as_ref().and_then(|r| r.client_code.clone()),
        estimates: match &rule {
            Some(r) => estimates_of(st, auth, r).await,
            None => Vec::new(),
        },
        no_history: t.estimate_no_history().to_string(),
        errors,
        t,
    })
    .into_response();

    // The form posts with `hx-swap="delete"`, because the ordinary answer to
    // a save is "the pane has done its job, take it away". A rejection is the
    // one response that must land IN the pane instead, so it retargets
    // itself rather than making the form carry a second, conditional swap it
    // would be wrong about on every successful save.
    res.headers_mut().insert(
        "HX-Reswap",
        axum::http::HeaderValue::from_static("outerHTML"),
    );
    res.headers_mut().insert(
        "HX-Retarget",
        axum::http::HeaderValue::from_static("#rulepane"),
    );
    res
}

/// `GET /rules/months?expr=…` — the read-back, on its own.
///
/// One round trip per keystroke, which is what buys the guarantee that the
/// preview and the parser are the same code. It is a pure GET with no session
/// requirement beyond being signed in: there is nothing here but arithmetic
/// on twelve numbers.
pub async fn months_preview(auth: Auth, Query(q): Query<MonthsQuery>) -> Response {
    Html(MonthsReadback::of(&auth.catalog(), &q.months)).into_response()
}

/// Named `months` rather than `expr` because HTMX sends an input's own
/// `name` as the parameter, and renaming it here would mean the field could
/// not simply carry `hx-get` — it would need an `hx-vals` saying the same
/// thing twice.
#[derive(Deserialize)]
pub struct MonthsQuery {
    #[serde(default)]
    months: String,
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
