//! `Ctrl-u` / `:trash` — everything deleted, each one restorable on its own.
//!
//! `u` walks back through your own last actions in order; this is the other
//! shape, a list of what is gone with a checkbox each. See `db::trash` for
//! why only deletions belong in it and whose ledger it reads.
//!
//! **A restore from here is an ordinary action, not an undo.** It writes
//! `restore` (a row) or `file_upload` (a file) with no `undo_of`, so `u`
//! straight afterwards puts back what was just restored — the last thing you
//! did is the last thing `u` reverses, which is the one promise `u` makes.

use crate::db::changes::Actor;
use crate::db::trash::{self, Item, What};
use crate::db::{ledger, tx};
use crate::domain::period::Period;
use crate::state::AppState;
use crate::ui::i18n::Catalog;
use crate::web::session::Auth;
use crate::web::template::Html;
use crate::web::tx::{files_follow_row, pane_rows, say, say_only};
use askama::Template;
use axum::extract::{Form, Query, State};
use axum::response::{IntoResponse, Response};
use chrono::Local;
use serde::Deserialize;

/// How much of the trash one opening shows. A household deletes a handful of
/// rows a month; past this the pane says it is not the whole trash.
const LIMIT: i64 = 200;

pub struct ItemView {
    /// `t12` for a row, `f42` for a file — what the checkbox submits.
    pub pick: String,
    pub is_file: bool,
    /// `Bill`, `Scheduled`, `Receipt` … for a file; empty for a row.
    pub kind: String,
    pub title: String,
    pub member: String,
    pub period: String,
    pub amount: String,
    pub is_income: bool,
    /// `18:39 · Rodrigo`, or `replaced 18:39` — when, and why it is here.
    pub when: String,
    /// `+ 2 files`, for a row whose files went with it.
    pub files: Option<String>,
}

pub struct DayView {
    pub label: String,
    pub items: Vec<ItemView>,
}

#[derive(Template)]
#[template(path = "partials/trash_pane.html")]
pub struct TrashPane {
    pub t: Catalog,
    pub days: Vec<DayView>,
    pub count: String,
    pub empty: bool,
    /// Only the newest `LIMIT` are shown.
    pub more: bool,
    /// The member column only earns its width when more than one ledger is
    /// listed — an admin's trash, not a member's own.
    pub show_member: bool,
    /// The month on screen, carried back on restore so the answer can swap
    /// the panes a restored row lands in.
    pub period: String,
}

#[derive(Deserialize)]
pub struct TrashParams {
    #[serde(default)]
    period: String,
}

/// `GET /trash`.
pub async fn view(
    State(st): State<AppState>,
    auth: Auth,
    Query(p): Query<TrashParams>,
) -> Response {
    match pane(&st, &auth, &p.period).await {
        Some(pane) => Html(pane).into_response(),
        None => say_only(auth.catalog().error_not_written(), true),
    }
}

async fn pane(st: &AppState, auth: &Auth, period: &str) -> Option<TrashPane> {
    let (mut items, more) = trash::list(&st.pool, auth.viewer, LIMIT).await.ok()?;
    // The trash is what can come back. A removed file whose copy is no longer
    // in the trash on disk — moved by hand — has nothing to restore, and
    // listing it would offer a restore that brings back an empty slot.
    let root = &st.cfg.receipts_root;
    items.retain(|it| match (&it.what, &it.path) {
        (What::File { .. }, Some(rel)) => {
            crate::services::receipts::deleted_copy(root, rel, it.id).is_some()
        }
        (What::File { .. }, None) => false,
        _ => true,
    });
    let currency = ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();
    Some(build(
        auth.catalog(),
        &items,
        more,
        auth.viewer.member_id,
        period,
        currency,
        Local::now().date_naive(),
    ))
}

/// The pane from what the trash holds — pure, so the sandbox renders the
/// same template from fixtures that a real `GET /trash` renders from rows.
pub fn build(
    t: Catalog,
    items: &[Item],
    more: bool,
    me: i64,
    period: &str,
    currency: crate::domain::money::Currency,
    today: chrono::NaiveDate,
) -> TrashPane {
    let show_member = items.iter().any(|i| i.member_id != me);

    let mut days: Vec<DayView> = Vec::new();
    for it in items {
        let day = crate::ui::log::local_date(&it.deleted_at).unwrap_or(today);
        let label = crate::ui::log::day_label(day, today, &t);
        let view = item_view(it, &t, currency);
        match days.last_mut() {
            Some(d) if d.label == label => d.items.push(view),
            _ => days.push(DayView {
                label,
                items: vec![view],
            }),
        }
    }

    TrashPane {
        count: t.trash_count(items.len() as i64),
        empty: items.is_empty(),
        more,
        show_member,
        period: period.to_string(),
        days,
        t,
    }
}

fn item_view(it: &Item, t: &Catalog, currency: crate::domain::money::Currency) -> ItemView {
    let time = crate::ui::log::local_time(&it.deleted_at);
    let (pick, is_file, kind, when, files) = match &it.what {
        What::Row { files } => (
            format!("t{}", it.id),
            false,
            String::new(),
            match &it.deleted_by {
                Some(who) => t.trash_deleted_by(&time, who),
                None => t.trash_deleted(&time),
            },
            (*files > 0).then(|| t.trash_with_files(*files)),
        ),
        What::File { kind, replaced } => (
            format!("f{}", it.id),
            true,
            file_kind(t, kind, it.is_income).to_string(),
            if *replaced {
                t.trash_replaced(&time)
            } else {
                match &it.deleted_by {
                    Some(who) => t.trash_deleted_by(&time, who),
                    None => t.trash_deleted(&time),
                }
            },
            None,
        ),
    };
    ItemView {
        pick,
        is_file,
        kind,
        title: it.title.clone(),
        member: it.member.clone(),
        period: crate::ui::fmt::period(it.period),
        amount: it
            .amount
            .map(|c| crate::ui::fmt::money(c, currency))
            .unwrap_or_else(|| crate::ui::fmt::money_placeholder(currency)),
        is_income: it.is_income,
        when,
        files,
    }
}

/// The word the file rail uses for this slot — a spend's Bill is an income's
/// Invoice, the same match `ui::row` makes.
fn file_kind(t: &Catalog, code: &str, income: bool) -> &'static str {
    match (code, income) {
        ("barcode", false) => t.file_bill(),
        ("barcode", true) => t.file_invoice(),
        ("scheduled", _) => t.file_scheduled(),
        ("receipt", false) => t.file_receipt(),
        (_, _) => t.file_payslip(),
    }
}

enum Picked {
    Row(i64),
    File(i64),
}

fn parse_pick(s: &str) -> Option<Picked> {
    let (tag, id) = s.split_at_checked(1)?;
    let id = id.parse().ok()?;
    match tag {
        "t" => Some(Picked::Row(id)),
        "f" => Some(Picked::File(id)),
        _ => None,
    }
}

/// `POST /trash/restore` — every checked item, rows before files.
///
/// Each pick is checked against the same rule every write asks
/// (`Viewer::may_write`, and privacy before it), and one that fails is
/// skipped and counted rather than failing the batch: the page listed only
/// what this viewer may restore, so a refusal here means the ground moved
/// (preview switched on in another window), not a mistake worth losing the
/// rest over.
pub async fn restore(
    State(st): State<AppState>,
    auth: Auth,
    // Repeated keys — `pick=t12&pick=f42` — which a struct cannot take.
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let t = auth.catalog();
    let actor = Actor::User(auth.viewer.member_id);
    let period_key = pairs
        .iter()
        .find(|(k, _)| k == "period")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let viewed = Period::parse(&period_key);

    let mut rows = Vec::new();
    let mut files = Vec::new();
    let mut failed = 0i64;
    for (k, v) in &pairs {
        if k != "pick" {
            continue;
        }
        let found = match parse_pick(v) {
            Some(Picked::Row(id)) => trash::deleted_row(&st.pool, id)
                .await
                .ok()
                .flatten()
                .map(|p| (id, p, false)),
            Some(Picked::File(id)) => trash::deleted_file(&st.pool, id)
                .await
                .ok()
                .flatten()
                .map(|p| (id, p, true)),
            None => None,
        };
        let Some((id, pick, is_file)) = found else {
            // Already back — restored from another window, or by `u`.
            continue;
        };
        let visible = pick.visibility != "private" || auth.viewer.sees_private_of(pick.member_id);
        if !visible || !auth.viewer.may_write(pick.member_id) {
            failed += 1;
            continue;
        }
        if is_file {
            files.push((id, pick));
        } else {
            rows.push((id, pick));
        }
    }

    if rows.is_empty() && files.is_empty() && failed == 0 {
        return say_only(t.trash_nothing_picked(), false);
    }

    // Two versions of one file cannot both be the live one. Picking which is
    // the person's call, and the server guessing — newest wins? — would put
    // back a file nobody chose.
    let mut slots = std::collections::HashSet::new();
    if files
        .iter()
        .any(|(_, p)| !slots.insert((p.tx_id, p.kind.clone())))
    {
        return say_only(t.trash_slot_twice(), true);
    }

    let mut touched: Vec<i64> = Vec::new();
    let mut elsewhere = 0i64;
    let mut restored_rows = 0i64;
    let mut restored_files = 0i64;
    let mut note = |p: &trash::Pick, touched: &mut Vec<i64>| {
        if Some(p.period) == viewed {
            if !touched.contains(&p.member_id) {
                touched.push(p.member_id);
            }
        } else {
            elsewhere += 1;
        }
    };

    // Why a pick did not come back, when there is a reason worth naming.
    let mut blocked: Vec<String> = Vec::new();
    for (id, pick) in &rows {
        if let Ok(Some(other)) = trash::bill_held_by(&st.pool, *id).await {
            blocked.push(t.trash_bill_elsewhere(&other));
            failed += 1;
            continue;
        }
        match tx::restore(&st.pool, actor, *id, None).await {
            Ok(Some(_)) => {
                // The files that went with it come back with it — exactly
                // what `u` on the delete does.
                files_follow_row(&st, actor, *id, true).await;
                restored_rows += 1;
                note(pick, &mut touched);
            }
            Ok(None) => failed += 1,
            Err(e) => {
                tracing::error!("trash: restoring row {id}: {e}");
                failed += 1;
            }
        }
    }
    for (id, pick) in &files {
        let slot = crate::web::files::slot_for(&st, &auth, pick.tx_id).await;
        let done = crate::services::receipts::restore_swapping(
            &st.pool,
            &st.cfg.receipts_root,
            actor,
            *id,
            None,
            slot.as_ref().map(|(s, _, _)| s),
        )
        .await;
        match done {
            Ok(true) => {
                restored_files += 1;
                note(pick, &mut touched);
            }
            Ok(false) => failed += 1,
            Err(e) => {
                tracing::error!("trash: restoring file {id}: {e}");
                failed += 1;
            }
        }
    }

    // The trash again, without what just left it, and every pane on screen a
    // restored row or file landed in — with its totals, like any mutation.
    let mut body = match pane(&st, &auth, &period_key)
        .await
        .and_then(|p| p.render().ok())
    {
        Some(html) => html,
        None => return say_only(t.error_not_written(), true),
    };
    if let Some(period) = viewed {
        for member_id in touched {
            if let Some(html) = pane_rows(&st, &auth, member_id, period)
                .await
                .and_then(|r| r.render().ok())
            {
                body.push_str(&html);
            }
        }
    }

    let mut what: Vec<String> = Vec::new();
    if restored_rows > 0 {
        what.push(t.trash_rows(restored_rows));
    }
    if restored_files > 0 {
        what.push(t.trash_files(restored_files));
    }
    let mut said: Vec<String> = Vec::new();
    if !what.is_empty() {
        said.push(t.trash_restored(&what.join(" · ")));
    }
    if elsewhere > 0 {
        said.push(t.trash_elsewhere(elsewhere));
    }
    if failed > blocked.len() as i64 {
        said.push(t.trash_failed(failed - blocked.len() as i64));
    }
    said.extend(blocked);
    let res = (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response();
    say(res, &said.join(" · "), failed > 0)
}
