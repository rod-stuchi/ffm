//! Mutations, and the one response shape they all use.
//!
//! **This is a contract, not a feature.** Every mutation in this app follows
//! the same flow, and fixing it once here is what stops each later phase
//! inventing its own:
//!
//! 1. The form submits, and a skeleton row appears immediately — real geometry,
//!    so nothing shifts when the answer lands.
//! 2. The server replies with **one row fragment**: the real row, or an error
//!    row carrying **Try Again** that resubmits the original action.
//! 3. The statusline says what happened, through the same channel the pre-auth
//!    pages use.
//!
//! An error row says what went wrong *and that nothing was written*, because
//! the alternative — a half-applied change the user cannot see — is the thing
//! optimistic UI is most likely to get wrong.

use crate::db::changes::Actor;
use crate::db::{ledger, tx};
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::status::{RowFlags, RowState, TxKind, Visibility};
use crate::state::AppState;
use crate::ui::i18n::Catalog;
use crate::ui::row::{FilePresence, Gutter, LedgerRow, RowCtx, RowView};
use crate::web::ledger::{HouseTotals, PaneTotals};
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Form, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::Local;
use serde::Deserialize;

/// The fragment every mutation returns.
#[derive(Template)]
#[template(path = "partials/fragment.html")]
pub struct Fragment {
    v: RowView,
    /// The row partial renders its own copy — status text, file labels — so a
    /// fragment carries the catalog exactly as a full page does.
    t: Catalog,
    /// The numbers this change moved, swapped out of band. Absent only when
    /// there is nothing to recompute.
    pane_totals: Option<PaneTotals>,
    house_totals: Option<HouseTotals>,
    /// An out-of-band swap target when a row is being *replaced* from
    /// somewhere that does not know where it sits — `u`, most obviously, which
    /// can undo a row in any pane.
    oob: Option<String>,
}

/// The statusline channel, carried on the response rather than in the body:
/// the message is not part of the row, and a swap should not have to include
/// chrome to say something.
fn say(mut res: Response, message: &str, bad: bool) -> Response {
    let payload = serde_json::json!({ "ffm:say": { "message": message, "bad": bad } });
    if let Ok(v) = HeaderValue::from_str(&ascii_json(&payload.to_string())) {
        res.headers_mut().insert("HX-Trigger", v);
    }
    res
}

/// JSON with every non-ASCII character escaped as `\uXXXX`.
///
/// HTTP header values are bytes, and a browser reads them as latin-1 — so a
/// UTF-8 `·` arrives as `Â·` and a household's own bill names come back
/// mangled in the statusline. Escaping is not a workaround: `\uXXXX` is
/// exactly what JSON has for this, and `JSON.parse` puts the character back.
fn ascii_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            for unit in c.encode_utf16(&mut [0u16; 2]) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

fn ctx(currency: crate::domain::money::Currency, show_recurring: bool) -> RowCtx {
    RowCtx {
        today: Local::now().date_naive(),
        currency,
        show_recurring,
        show_delta: true,
        narrow: false,
    }
}

/// Re-reads one row through the **pane query**, so a fragment is rendered by
/// exactly the same path as a full page load. A second, simpler "just this row"
/// query is how a fragment starts quietly disagreeing with the page it lands in.
async fn render_row(
    st: &AppState,
    auth: &Auth,
    id: i64,
    oob: bool,
) -> Result<Fragment, &'static str> {
    let Ok(Some((member_id, period, _deleted))) = tx::one(&st.pool, id).await else {
        return Err("that row is gone");
    };
    let Ok(members) = ledger::members(&st.pool).await else {
        return Err("cannot read the household");
    };
    let Some(member) = members.into_iter().find(|m| m.id == member_id) else {
        return Err("cannot read the household");
    };
    let house = ledger::household(&st.pool).await.ok().flatten();
    let currency = house.as_ref().map(|h| h.currency).unwrap_or_default();
    let show_recurring = house.as_ref().is_none_or(|h| h.show_recurring);

    let Ok(pane) = ledger::pane(
        &st.pool,
        &member,
        period,
        ctx(currency, show_recurring),
        auth.viewer,
    )
    .await
    else {
        return Err("cannot read that month");
    };

    // Deleted rows are not in the pane query — the row stays in the database,
    // but the ledger reads live rows. Re-render it from what we know instead of
    // widening the query that every page load runs.
    let row = pane.rows.into_iter().find(|r| r.id == id);
    match row {
        Some(r) => {
            let (panes, house) = crate::web::ledger::totals(st, auth, period).await;
            Ok(Fragment {
                oob: oob.then(|| format!("outerHTML:#row-{id}")),
                v: r.view(&auth.catalog()),
                t: auth.catalog(),
                pane_totals: panes.into_iter().find(|p| p.id == member_id),
                house_totals: Some(house),
            })
        }
        None => {
            let mut deleted = deleted_row(st, auth, id, member_id, period, currency).await?;
            deleted.oob = oob.then(|| format!("outerHTML:#row-{id}"));
            let (panes, house) = crate::web::ledger::totals(st, auth, period).await;
            deleted.pane_totals = panes.into_iter().find(|p| p.id == member_id);
            deleted.house_totals = Some(house);
            Ok(deleted)
        }
    }
}

/// A soft-deleted row, dimmed and struck through with its deletion date. It
/// stays where it was: `u` clears the mark on this same row rather than
/// inserting a second, superseding one.
async fn deleted_row(
    st: &AppState,
    auth: &Auth,
    id: i64,
    member_id: i64,
    period: Period,
    currency: crate::domain::money::Currency,
) -> Result<Fragment, &'static str> {
    let row = sqlx::query!(
        r#"SELECT t.kind AS "kind!: String", t.title AS "title!: String",
                  t.amount_cents AS "amount_cents: i64",
                  t.visibility AS "visibility!: String",
                  t.rule_id AS "rule_id: i64",
                  m.uuid AS "uuid!: String", m.slug AS "slug!: String"
           FROM transactions t JOIN members m ON m.id = t.member_id
           WHERE t.id = ?1"#,
        id
    )
    .fetch_optional(&st.pool)
    .await
    .map_err(|_| "cannot read that row")?
    .ok_or("that row is gone")?;

    // The viewer still has to be allowed to see it — a private row must not
    // become visible by being deleted.
    if row.visibility == "private" && !auth.viewer.sees_private_of(member_id) {
        return Err("that row is gone");
    }

    let ledger_row = LedgerRow {
        id,
        rule_id: row.rule_id,
        period,
        kind: TxKind::parse(&row.kind).unwrap_or(TxKind::Out),
        title: row.title,
        amount_cents: row.amount_cents.map(Cents),
        date: None,
        due_date: None,
        visibility: Visibility::parse(&row.visibility).unwrap_or_default(),
        bucket: None,
        bill_name: None,
        manual_paid: false,
        tracked: false,
        instance: None,
        total_installments: None,
        spans_period: false,
        window_size: None,
        window_start: None,
        amount_in_filename: false,
        varies: false,
        member_uuid: row.uuid,
        member_slug: row.slug,
        prev_amount_cents: None,
        estimate_cents: None,
        installments_paid: None,
        files: FilePresence::default(),
        state: RowState::Ok,
        flags: RowFlags {
            deleted: true,
            recurring: row.rule_id.is_some(),
            private: row.visibility == "private",
            ..RowFlags::default()
        },
        gutter: Gutter::Blank,
        ctx: ctx(currency, true),
    };

    Ok(Fragment {
        oob: None,
        v: ledger_row.view(&auth.catalog()),
        t: auth.catalog(),
        pane_totals: None,
        house_totals: None,
    })
}

/// The error row: red ground, what happened, and that nothing was written.
/// **Try Again** resubmits the original action rather than reloading the page,
/// so a failed save does not cost the user what they typed.
fn error_row(t: &Catalog, message: &str) -> Response {
    #[derive(Template)]
    #[template(path = "partials/error_row.html")]
    struct ErrorRow<'a> {
        message: &'a str,
        t: &'a Catalog,
    }
    // 200, not 4xx: HTMX swaps a successful response by default, and this row
    // IS the answer. Signalling failure in the status code would mean the user
    // sees nothing at all.
    Html(ErrorRow { message, t }).into_response()
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct AddForm {
    /// **Sent as a parameter, never read from the session.** Which pane is
    /// focused is data, and a server-side idea of "current member" drifts the
    /// moment the app is open in two windows.
    member_id: i64,
    period: String,
    kind: String,
    title: String,
    #[serde(default)]
    amount: String,
    #[serde(default)]
    bucket: String,
}

pub async fn add(State(st): State<AppState>, auth: Auth, Form(form): Form<AddForm>) -> Response {
    let t = auth.catalog();
    let title = form.title.trim();
    if title.is_empty() {
        return say(
            error_row(&t, &t.field_required(t.row_title())),
            &t.field_required(t.row_title()),
            true,
        );
    }
    let Some(period) = Period::parse(&form.period) else {
        return say(
            error_row(&t, t.error_not_written()),
            t.error_not_written(),
            true,
        );
    };
    let kind = TxKind::parse(&form.kind).unwrap_or(TxKind::Out);

    // Attribution is a column, and it is checked: a member id from a form is
    // input, so it has to name somebody who exists in this household.
    let Ok(members) = ledger::members(&st.pool).await else {
        return say(
            error_row(&t, t.error_not_written()),
            t.error_not_written(),
            true,
        );
    };
    if !members.iter().any(|m| m.id == form.member_id) {
        return say(
            error_row(&t, t.error_not_written()),
            t.error_not_written(),
            true,
        );
    }

    let bucket = form.bucket.trim();
    let new = tx::NewTransaction {
        member_id: form.member_id,
        period,
        kind,
        title,
        amount: Cents::parse(&form.amount),
        date: Some(Local::now().date_naive()),
        bucket: (!bucket.is_empty()).then_some(bucket),
        visibility: Visibility::Household,
    };

    match tx::add(&st.pool, Actor::User(auth.viewer.member_id), new).await {
        Ok(id) => match render_row(&st, &auth, id, false).await {
            Ok(f) => say(Html(f).into_response(), &t.row_added(title), false),
            Err(e) => say(error_row(&t, e), e, true),
        },
        Err(_) => say(
            error_row(&t, t.error_not_written()),
            t.error_not_written(),
            true,
        ),
    }
}

// ---------------------------------------------------------------------------
// delete / undo
// ---------------------------------------------------------------------------

pub async fn delete(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    if !may_touch(&st, &auth, id).await {
        return say(
            error_row(&t, t.error_not_written()),
            t.error_not_written(),
            true,
        );
    }
    match tx::soft_delete(&st.pool, Actor::User(auth.viewer.member_id), id).await {
        Ok(Some(_)) => match render_row(&st, &auth, id, false).await {
            Ok(f) => say(Html(f).into_response(), t.row_deleted_say(), false),
            Err(e) => say(error_row(&t, e), e, true),
        },
        _ => say(
            error_row(&t, t.error_not_written()),
            t.error_not_written(),
            true,
        ),
    }
}

/// `u`. Undoes this member's most recent action, and says which one — an undo
/// that silently changes something off screen is worse than no undo.
pub async fn undo(State(st): State<AppState>, auth: Auth) -> Response {
    let t = auth.catalog();
    let Ok(Some(u)) = tx::last_undoable(&st.pool, auth.viewer.member_id).await else {
        // Nothing to undo is an answer, not an error: say so and change
        // nothing.
        return say(
            (StatusCode::OK, HeaderMap::new(), String::new()).into_response(),
            t.nothing_to_undo(),
            false,
        );
    };

    match tx::undo(&st.pool, Actor::User(auth.viewer.member_id), &u).await {
        Ok(true) => match render_row(&st, &auth, u.entity_id, true).await {
            Ok(f) => say(Html(f).into_response(), t.undone(), false),
            Err(e) => say(error_row(&t, e), e, true),
        },
        _ => say(
            error_row(&t, t.error_not_written()),
            t.error_not_written(),
            true,
        ),
    }
}

/// Whether this viewer may act on that row at all.
///
/// Privacy is not only a read rule: a row you cannot see is a row you cannot
/// delete, or its absence from your ledger would be contradicted by its
/// disappearance from somebody else's.
async fn may_touch(st: &AppState, auth: &Auth, id: i64) -> bool {
    let row = sqlx::query!(
        r#"SELECT member_id AS "member_id!: i64", visibility AS "visibility!: String"
           FROM transactions WHERE id = ?1"#,
        id
    )
    .fetch_optional(&st.pool)
    .await;

    match row {
        Ok(Some(r)) => r.visibility != "private" || auth.viewer.sees_private_of(r.member_id),
        _ => false,
    }
}
