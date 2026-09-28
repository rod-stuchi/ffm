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
use crate::ui::row::{FilePresence, FileView, Gutter, LedgerRow, LineNumbers, RowCtx, RowView};
use crate::web::form::FieldErrors;
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
pub(crate) fn say(mut res: Response, message: &str, bad: bool) -> Response {
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
pub fn ascii_json(s: &str) -> String {
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

fn ctx(auth: &Auth, currency: crate::domain::money::Currency, show_recurring: bool) -> RowCtx {
    RowCtx {
        today: Local::now().date_naive(),
        // A fragment is rendered by the same path a page load uses, including
        // the order the member chose — otherwise a row swapped into a
        // date-sorted pane would land where manual order would have put it.
        sort: crate::db::ledger::Sort::parse(&auth.settings.ledger_sort),
        currency,
        show_recurring,
        show_delta: auth.settings.ledger_show_delta,
        narrow: false,
        line_numbers: LineNumbers::parse(&auth.settings.line_numbers),
    }
}

/// A statusline message and nothing else — for the refusals that change no
/// data: `E` on a plain row, a `u` with nothing behind it.
pub fn say_only(message: &str, bad: bool) -> Response {
    say(
        (StatusCode::OK, HeaderMap::new(), String::new()).into_response(),
        message,
        bad,
    )
}

/// One row plus the totals it moved — the shape every row-level mutation
/// answers with, exposed so the file handlers use the identical path rather
/// than a second one that renders "the same" row slightly differently.
pub async fn row_fragment(
    st: &AppState,
    auth: &Auth,
    id: i64,
    _member_id: i64,
    _period: Period,
    message: &str,
) -> Response {
    match render_row(st, auth, id, true).await {
        Ok(f) => say(Html(f).into_response(), message, false),
        Err(e) => say(error_row(&auth.catalog(), e), e, true),
    }
}

/// A whole pane's rows plus the totals, out of band.
///
/// A row mutation answers with one row; a rule save can create several periods
/// at once, so it answers with the pane. Same contract, one level up — and the
/// rows come from the same `ledger::pane` call a page load uses, so privacy
/// filtering cannot come apart from what is rendered.
pub async fn pane_fragment(
    st: &AppState,
    auth: &Auth,
    member_id: i64,
    period: Period,
    message: &str,
) -> Response {
    let Ok(members) = ledger::members(&st.pool).await else {
        return say_only(auth.catalog().error_not_written(), true);
    };
    let Some(member) = members.into_iter().find(|m| m.id == member_id) else {
        return say_only(auth.catalog().error_not_written(), true);
    };
    let house = ledger::household(&st.pool).await.ok().flatten();
    let currency = house.as_ref().map(|h| h.currency).unwrap_or_default();
    let show_recurring = house.as_ref().is_none_or(|h| h.show_recurring);

    let Ok(pane) = ledger::pane_with_vault(
        &st.pool,
        &member,
        period,
        ctx(auth, currency, show_recurring),
        auth.viewer,
        Some(&st.cfg.receipts_root),
    )
    .await
    else {
        return say_only(auth.catalog().error_not_written(), true);
    };

    let t = auth.catalog();
    // Built by the page's own `view`, grouped exactly when the page groups —
    // this fragment used to render the rows flat, which un-grouped a grouped
    // pane on every answer that re-rendered it.
    let group = crate::web::ledger::GroupOrder::of(&auth.settings);
    let writable = auth.viewer.may_write(member_id);
    let p = crate::web::ledger::view(pane, &t, currency, group, writable);
    let (panes, totals) = crate::web::ledger::totals(st, auth, period).await;

    say(
        Html(crate::web::rules::PaneRows {
            t,
            p,
            pane_totals: panes.into_iter().find(|p| p.id == member_id),
            house_totals: Some(totals),
        })
        .into_response(),
        message,
        false,
    )
}

/// Whether the focused ordering is one the SERVER derives — grouped by
/// bucket, or sorted by date or amount — rather than the stored manual order.
///
/// In a derived view, where a row sits follows from what it says: change its
/// bucket and it belongs in another fold, change its amount under
/// `sort = amount` and it belongs elsewhere in the list. A single row swapped
/// back in place is then in the wrong spot — "Venda" filed under `Abc` stayed
/// in `Sem categoria` until a reload — so such an answer re-renders the pane.
fn derived_view(auth: &Auth) -> bool {
    crate::db::ledger::Sort::parse(&auth.settings.ledger_sort).is_derived()
        || auth.settings.ledger_group != "none"
}

/// The pane, re-rendered, for a change that may have moved `id` — with the
/// cursor put back on that row once the pane has settled, and no swap of the
/// requesting element (the row it targeted may now live in another fold).
///
/// The cursor rides `HX-Trigger-After-Settle`, not `HX-Trigger`: the latter
/// fires as the response arrives, before the pane it points into exists.
async fn pane_at(
    st: &AppState,
    auth: &Auth,
    id: i64,
    member_id: i64,
    period: Period,
    message: &str,
) -> Response {
    let mut res = pane_fragment(st, auth, member_id, period, message).await;
    let cursor = serde_json::json!({ "ffm:cursor": { "id": id } }).to_string();
    if let Ok(v) = HeaderValue::from_str(&cursor) {
        res.headers_mut().insert("HX-Trigger-After-Settle", v);
    }
    res.headers_mut()
        .insert("HX-Reswap", HeaderValue::from_static("none"));
    res
}

/// Re-reads one row through the **pane query**, so a fragment is rendered by
/// exactly the same path as a full page load. A second, simpler "just this row"
/// query is how a fragment starts quietly disagreeing with the page it lands in.
/// One live row, read through the same pane query a page load uses — so its
/// files, status and privacy filtering are exactly what the ledger shows.
/// `None` for a row the pane does not list (deleted, or not visible).
pub(crate) async fn live_row(
    st: &AppState,
    auth: &Auth,
    id: i64,
) -> Result<
    (
        Option<LedgerRow>,
        i64,
        Period,
        crate::domain::money::Currency,
    ),
    &'static str,
> {
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

    let Ok(pane) = ledger::pane_with_vault(
        &st.pool,
        &member,
        period,
        ctx(auth, currency, show_recurring),
        auth.viewer,
        Some(&st.cfg.receipts_root),
    )
    .await
    else {
        return Err("cannot read that month");
    };
    Ok((
        pane.rows.into_iter().find(|r| r.id == id),
        member_id,
        period,
        currency,
    ))
}

async fn render_row(
    st: &AppState,
    auth: &Auth,
    id: i64,
    oob: bool,
) -> Result<Fragment, &'static str> {
    // Deleted rows are not in the pane query — the row stays in the database,
    // but the ledger reads live rows. Re-render it from what we know instead of
    // widening the query that every page load runs.
    let (row, member_id, period, currency) = live_row(st, auth, id).await?;
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
                  r.deleted_at AS "rule_deleted_at?: String",
                  m.slug AS "slug!: String"
           FROM transactions t JOIN members m ON m.id = t.member_id
           LEFT JOIN recurring_rules r ON r.id = t.rule_id
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
        cadence_months: None,
        rule_deleted_on: row
            .rule_deleted_at
            .as_deref()
            .and_then(crate::db::ledger::local_day),
        member_slug: row.slug,
        prev_amount_cents: None,
        estimate_cents: None,
        installments_paid: None,
        files: FilePresence::default(),
        state: RowState::Ok,
        flags: RowFlags {
            deleted: true,
            recurring: row.rule_id.is_some() && row.rule_deleted_at.is_none(),
            private: row.visibility == "private",
            ..RowFlags::default()
        },
        gutter: Gutter::Blank,
        ctx: ctx(auth, currency, true),
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
    // IS the answer. Signaling failure in the status code would mean the user
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
    if let Err(refused) = may_write(&st, &auth, form.member_id).await {
        return refused;
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
        // In a derived view the new row belongs where its bucket, date or
        // amount puts it — not before the skeleton at the foot of the pane.
        Ok(id) if derived_view(&auth) => {
            pane_at(&st, &auth, id, form.member_id, period, &t.row_added(title)).await
        }
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
    if let Err(refused) = may_touch(&st, &auth, id).await {
        return refused;
    }
    let actor = Actor::User(auth.viewer.member_id);
    match tx::soft_delete(&st.pool, actor, id).await {
        Ok(Some(_)) => {
            // The row's files follow it into the member's trash — a deleted
            // bill left standing in the vault reads as live to anybody
            // browsing it. `u` brings both back together.
            files_follow_row(&st, actor, id, false).await;
            match render_row(&st, &auth, id, false).await {
                Ok(f) => say(Html(f).into_response(), t.row_deleted_say(), false),
                Err(e) => say(error_row(&t, e), e, true),
            }
        }
        _ => say(
            error_row(&t, t.error_not_written()),
            t.error_not_written(),
            true,
        ),
    }
}

/// Moves a row's files to the member's trash when the row is deleted, or puts
/// them back when it is restored — whichever way `dd` or `u` just went.
///
/// A failure here is logged, not reported as the row failing: the row change
/// is already committed and correct, and the disk still decides what the row
/// shows.
async fn files_follow_row(st: &AppState, actor: Actor, id: i64, restored: bool) {
    let root = &st.cfg.receipts_root;
    let done = if restored {
        crate::services::receipts::restore_with_row(&st.pool, root, actor, id).await
    } else {
        crate::services::receipts::retire_with_row(&st.pool, root, actor, id).await
    };
    if let Err(e) = done {
        tracing::error!("files of row {id} did not follow it: {e}");
    }
}

/// `J` / `K` — move the row, or refuse and say why.
///
/// You cannot reorder a derived view: under `sort = date` the rows are in an
/// order the household did not put them in, so moving one has nowhere to be
/// recorded. The app says that instead of pretending, which is the same
/// treatment `E` gets on a plain row.
pub async fn move_row(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<i64>,
    Form(form): Form<MoveForm>,
) -> Response {
    let t = auth.catalog();
    let sort = crate::db::ledger::Sort::parse(&auth.settings.ledger_sort);
    if sort.is_derived() {
        return say_only(&t.sort_refuses_reorder(sort.code()), true);
    }
    if auth.settings.ledger_group != "none" {
        return say_only(&t.group_refuses_reorder(&auth.settings.ledger_group), true);
    }
    if let Err(refused) = may_touch(&st, &auth, id).await {
        return refused;
    }

    match tx::reorder(
        &st.pool,
        Actor::User(auth.viewer.member_id),
        id,
        form.delta,
        None,
    )
    .await
    {
        Ok(true) => match tx::one(&st.pool, id).await {
            Ok(Some((member_id, period, _))) => {
                let moved = t.rows_moved(
                    1,
                    &if form.delta > 0 {
                        t.moved_down().to_string()
                    } else {
                        t.moved_up().to_string()
                    },
                );
                pane_fragment(&st, &auth, member_id, period, &moved).await
            }
            _ => say_only(t.error_not_written(), true),
        },
        // Already at the end it is being moved toward: a quiet no-op, not an
        // error worth a red statusline.
        Ok(false) => say_only("", false),
        Err(_) => say_only(t.error_not_written(), true),
    }
}

#[derive(Deserialize)]
pub struct MoveForm {
    delta: i64,
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

    // Undo is a write like any other: to a row this viewer may still change,
    // and a re-attribution only by an admin, exactly as the gesture itself.
    let target = if u.entity == "file" {
        file_row(&st, u.entity_id).await
    } else {
        Some(u.entity_id)
    };
    if let Some(id) = target
        && let Err(refused) = may_touch(&st, &auth, id).await
    {
        return refused;
    }
    if u.action == "reattribute" && !auth.viewer.is_admin() {
        return refuse(t.reattribute_admin_only(), true);
    }

    // A file change is undone by the vault service, which is the only thing
    // that may move a file; a transaction change by the ledger. `u` does not
    // care which it was, and neither should the person pressing it.
    let (ok, row_id) = if u.entity == "file" {
        (
            undo_file(&st, &auth, &u).await,
            file_row(&st, u.entity_id).await,
        )
    } else {
        let actor = Actor::User(auth.viewer.member_id);
        let ok = tx::undo(&st.pool, actor, &u).await.unwrap_or(false);
        // The row's files come and go with it: undoing a delete restores
        // them, undoing an add (or a restore) deletes the row, so they move.
        match u.action.as_str() {
            "delete" if ok => files_follow_row(&st, actor, u.entity_id, true).await,
            "create" | "restore" if ok => files_follow_row(&st, actor, u.entity_id, false).await,
            _ => {}
        }
        (ok, Some(u.entity_id))
    };

    // Undoing a re-attribution moves the row between two panes, and a single
    // row fragment could only ever tell half of that — the same reason the
    // re-attribution itself answers with a refresh rather than a row.
    if ok && u.action == "reattribute" {
        let mut res = say(
            (StatusCode::OK, HeaderMap::new(), String::new()).into_response(),
            t.undone(),
            false,
        );
        res.headers_mut()
            .insert("HX-Refresh", axum::http::HeaderValue::from_static("true"));
        return res;
    }

    // Undoing a reorder changes where a row sits among its neighbors, which
    // no single-row fragment can express — the whole pane comes back, exactly
    // as `J`/`K` themselves answer.
    if ok
        && u.action == "reorder"
        && let Ok(Some((member_id, period, _))) = tx::one(&st.pool, u.entity_id).await
    {
        return pane_fragment(&st, &auth, member_id, period, t.undone()).await;
    }

    // Undoing an edit puts back a bucket, amount or date — which, in a derived
    // view, can put the row back somewhere else.
    if ok
        && u.entity == "transaction"
        && u.action == "edit"
        && derived_view(&auth)
        && let Ok(Some((member_id, period, _))) = tx::one(&st.pool, u.entity_id).await
    {
        return pane_at(&st, &auth, u.entity_id, member_id, period, t.undone()).await;
    }

    match (ok, row_id) {
        (true, Some(id)) => match render_row(&st, &auth, id, true).await {
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

/// Which row a `receipts` entry belongs to, so an undone file change can be
/// rendered back into the ledger it came from.
async fn file_row(st: &AppState, receipt_id: i64) -> Option<i64> {
    sqlx::query_scalar!(
        r#"SELECT transaction_id AS "t!: i64" FROM receipts WHERE id = ?1"#,
        receipt_id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten()
}

/// Reverses a file change: a delete puts the file back, an upload takes it
/// away again. The vault is only ever touched through `services::receipts`.
async fn undo_file(st: &AppState, auth: &Auth, u: &tx::Undoable) -> bool {
    let actor = Actor::User(auth.viewer.member_id);
    let Some(tx_id) = file_row(st, u.entity_id).await else {
        tracing::error!("undo: receipts row {} has no transaction", u.entity_id);
        return false;
    };
    let Some((slot, _, _)) = crate::web::files::slot_for(st, auth, tx_id).await else {
        tracing::error!("undo: row {tx_id} has nothing to file under");
        return false;
    };

    let result = match u.action.as_str() {
        "file_delete" => {
            crate::services::receipts::restore(
                &st.pool,
                &st.cfg.receipts_root,
                actor,
                u.entity_id,
                Some(u.change_id),
            )
            .await
        }
        "file_upload" => {
            let kind: Option<String> = sqlx::query_scalar!(
                r#"SELECT kind AS "k!: String" FROM receipts WHERE id = ?1"#,
                u.entity_id
            )
            .fetch_optional(&st.pool)
            .await
            .ok()
            .flatten();
            let Some(kind) = kind
                .as_deref()
                .and_then(crate::domain::receipt_path::Kind::parse)
            else {
                return false;
            };
            crate::services::receipts::soft_delete(
                &st.pool,
                &st.cfg.receipts_root,
                actor,
                &slot,
                kind,
                "manual",
                Some(u.change_id),
            )
            .await
        }
        _ => return false,
    };

    match result {
        Ok(ok) => ok,
        // A failure that only ever becomes a statusline message is invisible
        // to whoever has to fix it. The person gets the short version; the log
        // gets the reason.
        Err(e) => {
            tracing::error!("undo of file {}: {e}", u.entity_id);
            false
        }
    }
}

/// Whether this viewer may act on that row at all — or the refusal to send.
///
/// Privacy comes first, and is not only a read rule: a row you cannot see is
/// a row you cannot delete, or its absence from your ledger would be
/// contradicted by its disappearance from somebody else's. A row you *can*
/// see is still somebody else's to change unless it is yours, or you are an
/// admin — see `Viewer::may_write`.
async fn may_touch(st: &AppState, auth: &Auth, id: i64) -> Result<(), Response> {
    let t = auth.catalog();
    let row = sqlx::query!(
        r#"SELECT member_id AS "member_id!: i64", visibility AS "visibility!: String"
           FROM transactions WHERE id = ?1"#,
        id
    )
    .fetch_optional(&st.pool)
    .await;

    match row {
        Ok(Some(r)) if r.visibility != "private" || auth.viewer.sees_private_of(r.member_id) => {
            may_write(st, auth, r.member_id).await
        }
        _ => Err(refuse(t.error_not_written(), true)),
    }
}

/// Whether this viewer may change what `owner` owns — or the refusal, naming
/// whose ledger it is. The one server-side answer every write asks: rows,
/// rules and files alike.
pub(crate) async fn may_write(st: &AppState, auth: &Auth, owner: i64) -> Result<(), Response> {
    if auth.viewer.may_write(owner) {
        return Ok(());
    }
    let t = auth.catalog();
    let who = ledger::members(&st.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.id == owner)
        .map(|m| m.name)
        .unwrap_or_default();
    Err(refuse(&t.read_only_ledger(&who), true))
}

/// A refusal with nothing to swap. Most row gestures target the row itself
/// with `outerHTML`, and an empty body swapped there would remove the very
/// thing that was refused.
pub(crate) fn refuse(message: &str, bad: bool) -> Response {
    let mut res = say_only(message, bad);
    res.headers_mut()
        .insert("HX-Reswap", axum::http::HeaderValue::from_static("none"));
    res
}

// ---------------------------------------------------------------------------
// `e` — editing an occurrence in place
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "partials/edit_row.html")]
struct EditRow {
    t: Catalog,
    id: i64,
    /// The row's files, each with Open and Remove. Removing one swaps only
    /// this strip, so whatever is being typed in the form survives it.
    files: Vec<FileView>,
    title: String,
    amount: String,
    /// The stored integer, which the mask seeds from. The formatted `value`
    /// stays too, so the field reads correctly for the frame before Alpine
    /// runs — but the mask never parses it back, which is the drift this
    /// whole arrangement exists to prevent.
    amount_cents: i64,
    day: String,
    bucket: String,
    buckets: Vec<String>,
    private: bool,
    /// A recurring row gets no Day field: its due day lives on the rule.
    recurring: bool,
    /// What the row said when this form was rendered. Goes back with the save
    /// as a hidden field, and the UPDATE matches on it — so a row somebody
    /// else has changed since refuses instead of silently overwriting them.
    version: i64,
    errors: FieldErrors,
}

/// The files the edit form lists — the row's own rail, read through the same
/// pane query, so the form offers exactly the files the ledger shows.
pub(crate) async fn edit_files(st: &AppState, auth: &Auth, id: i64) -> Vec<FileView> {
    match live_row(st, auth, id).await {
        Ok((Some(r), ..)) => r.view(&auth.catalog()).files,
        _ => Vec::new(),
    }
}

/// The answer to a file removed from inside the edit form: the strip alone,
/// plus the totals it may have moved (a payslip is what turns an estimate
/// into a fact). The form around the strip is untouched, draft and all.
#[derive(Template)]
#[template(path = "partials/edit_files_answer.html")]
pub(crate) struct EditFilesAnswer {
    pub t: Catalog,
    pub id: i64,
    pub files: Vec<FileView>,
    pub pane_totals: Option<PaneTotals>,
    pub house_totals: Option<HouseTotals>,
}

pub(crate) async fn edit_files_answer(
    st: &AppState,
    auth: &Auth,
    id: i64,
    message: &str,
) -> Response {
    let (row, member_id, period, _) = match live_row(st, auth, id).await {
        Ok(x) => x,
        Err(e) => return say_only(e, true),
    };
    let files = row
        .map(|r| r.view(&auth.catalog()).files)
        .unwrap_or_default();
    let (panes, house) = crate::web::ledger::totals(st, auth, period).await;
    say(
        Html(EditFilesAnswer {
            t: auth.catalog(),
            id,
            files,
            pane_totals: panes.into_iter().find(|p| p.id == member_id),
            house_totals: Some(house),
        })
        .into_response(),
        message,
        false,
    )
}

/// Opens the row into its own form. The row is replaced rather than covered —
/// the rest of the ledger does not move.
pub async fn edit_form(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    if let Err(refused) = may_touch(&st, &auth, id).await {
        return refused;
    }

    let row = sqlx::query!(
        r#"SELECT title AS "title!: String", amount_cents AS "amount_cents: i64",
                  date AS "date: String", bucket AS "bucket: String",
                  visibility AS "visibility!: String", rule_id AS "rule_id: i64",
                  version AS "version!: i64"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten();

    let Some(row) = row else {
        return say_only(t.error_not_written(), true);
    };

    let files = edit_files(&st, &auth, id).await;
    Html(EditRow {
        id,
        files,
        // Shown the way the amount field expects it back, not the way the
        // ledger displays it: the field takes digits and a separator, and
        // round-tripping a rendered "R$ 343,62" through it is how a currency
        // symbol ends up in a number.
        amount: row
            .amount_cents
            .map(|c| format!("{},{:02}", c / 100, (c % 100).abs()))
            .unwrap_or_default(),
        amount_cents: row.amount_cents.unwrap_or_default(),
        title: row.title,
        day: row
            .date
            .as_deref()
            .and_then(|d| d.get(8..10))
            .unwrap_or("")
            .trim_start_matches('0')
            .to_string(),
        bucket: row.bucket.unwrap_or_default(),
        buckets: crate::db::rules::buckets(&st.pool, auth.viewer.member_id)
            .await
            .unwrap_or_default(),
        private: row.visibility == "private",
        recurring: row.rule_id.is_some(),
        version: row.version,
        errors: FieldErrors::new(),
        t,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct EditForm {
    title: String,
    #[serde(default)]
    amount: String,
    #[serde(default)]
    day: String,
    #[serde(default)]
    bucket: String,
    #[serde(default)]
    private: Option<String>,
    /// The version the form was rendered with. Absent or unparseable means
    /// "do not check" rather than "check against zero" — a form that lost its
    /// hidden field should behave like the old unguarded save, not refuse
    /// every row whose version has ever moved.
    #[serde(default)]
    version: String,
}

pub async fn save_edit(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<i64>,
    Form(form): Form<EditForm>,
) -> Response {
    let t = auth.catalog();
    if let Err(refused) = may_touch(&st, &auth, id).await {
        return refused;
    }
    let title = form.title.trim();
    let bucket = form.bucket.trim();

    let mut errors = FieldErrors::new();
    if title.is_empty() {
        errors.add("title", t.field_required(t.row_title()));
    }
    if !form.day.trim().is_empty()
        && !form
            .day
            .trim()
            .parse::<u32>()
            .is_ok_and(|d| (1..=31).contains(&d))
    {
        errors.add("day", t.edit_day_bad());
    }
    if errors.is_bad() {
        return reopen(&st, &auth, id, &form, errors).await;
    }

    let ok = tx::edit(
        &st.pool,
        Actor::User(auth.viewer.member_id),
        id,
        tx::EditFields {
            title,
            amount: Cents::parse(&form.amount),
            day: form
                .day
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|d| (1..=31).contains(d)),
            bucket: (!bucket.is_empty()).then_some(bucket),
            visibility: if form.private.is_some() {
                Visibility::Private
            } else {
                Visibility::Household
            },
        },
        None,
        form.version.trim().parse::<i64>().ok(),
    )
    .await;

    match ok {
        Ok(tx::Saved::Ok) => match tx::one(&st.pool, id).await {
            Ok(Some((member_id, period, _))) if derived_view(&auth) => {
                pane_at(&st, &auth, id, member_id, period, t.edit_saved()).await
            }
            Ok(Some((member_id, period, _))) => {
                row_fragment(&st, &auth, id, member_id, period, t.edit_saved()).await
            }
            _ => say_only(t.error_not_written(), true),
        },
        // Somebody saved this row while the form was open. Nothing was
        // written; the row comes back read-only showing THEIR version, and
        // the draft stays parked so nothing typed is lost. The marker header
        // is what tells the form which of those two it is — a 200 with a row
        // in it looks identical from the client's side otherwise.
        Ok(tx::Saved::Stale) => {
            let who = last_editor(&st, id).await;
            stale(
                &st,
                &auth,
                id,
                match who {
                    Some(name) => t.edit_stale(&name),
                    None => t.edit_stale_unknown().to_string(),
                },
            )
            .await
        }
        Ok(tx::Saved::Gone) => stale(&st, &auth, id, t.edit_gone().to_string()).await,
        _ => say_only(t.error_not_written(), true),
    }
}

/// Who last changed a row, for the copy. The change log is already the record
/// of exactly this, so there is nothing to add — just the most recent entry's
/// actor, skipping the system ones (the materializer is not somebody who got
/// there first).
async fn last_editor(st: &AppState, id: i64) -> Option<String> {
    sqlx::query_scalar!(
        r#"SELECT m.name AS "name!: String"
           FROM change c JOIN members m ON m.id = c.member_id
           WHERE c.entity = 'transaction' AND c.entity_id = ?1 AND c.source = 'user'
           ORDER BY c.id DESC LIMIT 1"#,
        id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten()
}

/// A refused save: the row as it actually stands now, plus a header saying
/// this was a refusal rather than a success, so the form keeps the draft
/// instead of clearing it.
async fn stale(st: &AppState, auth: &Auth, id: i64, message: String) -> Response {
    let mut res = match render_row(st, auth, id, false).await {
        Ok(f) => say(Html(f).into_response(), &message, true),
        Err(e) => return say(error_row(&auth.catalog(), e), e, true),
    };
    res.headers_mut()
        .insert("FFM-Stale", axum::http::HeaderValue::from_static("1"));
    res
}

/// A rejected edit comes back as **the form again**, with what was typed
/// still in it and the message beside its field.
///
/// The marker header matters: the shell's swap handler clears `editing` and
/// the draft whenever a row that held a form is swapped, which is right for a
/// save that worked and wrong for one that was refused and is handing the
/// form straight back.
async fn reopen(
    st: &AppState,
    auth: &Auth,
    id: i64,
    form: &EditForm,
    errors: FieldErrors,
) -> Response {
    let t = auth.catalog();
    let version = sqlx::query_scalar!(
        r#"SELECT version AS "v!: i64" FROM transactions WHERE id = ?1"#,
        id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten()
    .unwrap_or_default();

    let message = errors.messages().first().unwrap_or(&"").to_string();
    let files = edit_files(st, auth, id).await;
    let mut res = say(
        Html(EditRow {
            id,
            files,
            title: form.title.clone(),
            amount: form.amount.clone(),
            amount_cents: Cents::parse(&form.amount)
                .map(|c| c.get())
                .unwrap_or_default(),
            day: form.day.clone(),
            bucket: form.bucket.clone(),
            buckets: crate::db::rules::buckets(&st.pool, auth.viewer.member_id)
                .await
                .unwrap_or_default(),
            private: form.private.is_some(),
            recurring: form.day.trim().is_empty(),
            version,
            errors,
            t,
        })
        .into_response(),
        &message,
        true,
    );
    res.headers_mut()
        .insert("FFM-Reopen", axum::http::HeaderValue::from_static("1"));
    res
}

/// One row, read-only — what Cancel swaps back in.
pub async fn one_row(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    match render_row(&st, &auth, id, false).await {
        Ok(f) => Html(f).into_response(),
        Err(e) => say_only(e, true),
    }
}

/// `p` — toggle private.
pub async fn privacy(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    if let Err(refused) = may_touch(&st, &auth, id).await {
        return refused;
    }
    match tx::toggle_privacy(&st.pool, Actor::User(auth.viewer.member_id), id, None).await {
        Ok(Some(now_private)) => {
            let msg = if now_private {
                t.row_now_private()
            } else {
                t.row_now_shared()
            };
            match tx::one(&st.pool, id).await {
                Ok(Some((member_id, period, _))) => {
                    row_fragment(&st, &auth, id, member_id, period, msg).await
                }
                _ => say_only(t.error_not_written(), true),
            }
        }
        _ => say_only(t.error_not_written(), true),
    }
}

#[derive(Deserialize)]
pub struct MoveMemberForm {
    to: i64,
}

/// `Ctrl-H` / `Ctrl-L` — re-attribute to another member's pane.
///
/// Fully undoable, and the statusline says where it went — which is why an
/// ordinary row does not confirm. A **private** row does: handing a hidden
/// transaction to somebody makes *them* its owner, so the person who hid it
/// stops seeing it, and that is invisible in the gesture.
pub async fn reattribute(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<i64>,
    Form(form): Form<MoveMemberForm>,
) -> Response {
    let t = auth.catalog();
    if let Err(refused) = may_touch(&st, &auth, id).await {
        return refused;
    }
    // Handing a row to somebody writes into *their* ledger, so it is an
    // admin's gesture even on a row of your own.
    if !auth.viewer.is_admin() {
        return refuse(t.reattribute_admin_only(), true);
    }

    // `to` is input: it has to name somebody who exists in this household.
    let Some(to_name) = ledger::members(&st.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.id == form.to)
        .map(|m| m.name)
    else {
        return refuse(t.error_not_written(), true);
    };

    match tx::reattribute(
        &st.pool,
        Actor::User(auth.viewer.member_id),
        id,
        form.to,
        None,
    )
    .await
    {
        Ok(true) => match tx::one(&st.pool, id).await {
            Ok(Some((_, period, _))) => {
                // Both panes changed, so the whole month comes back rather
                // than one row: the row left one pane and joined another, and
                // a single fragment could only tell half of that.
                let msg = t.row_reattributed(&to_name);
                let mut res = pane_fragment(&st, &auth, form.to, period, &msg).await;
                res.headers_mut()
                    .insert("HX-Refresh", axum::http::HeaderValue::from_static("true"));
                res
            }
            _ => say_only(t.error_not_written(), true),
        },
        _ => say_only(t.error_not_written(), true),
    }
}
