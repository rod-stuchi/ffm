//! `:prune` — byte-identical files in the vault, and removing the copies the
//! trash holds of them. Admin only; see `services::prune` for why this is the
//! one place a file leaves the disk, and what keeps it safe.

use crate::db::ledger;
use crate::domain::money::Currency;
use crate::services::prune::{self, Group, Place, PruneError, Record, Refusal};
use crate::state::AppState;
use crate::ui::fmt;
use crate::ui::i18n::Catalog;
use crate::web::session::Auth;
use crate::web::template::Html;
use crate::web::tx::{say, say_only};
use askama::Template;
use axum::extract::{Form, State};
use axum::response::{IntoResponse, Response};

pub struct CopyView {
    /// Relative to the vault — what the checkbox submits, and what is shown.
    pub rel: String,
    pub in_trash: bool,
    /// `live · Moto CG 09/2026 · Bill`, `trash · deleted 18/09/2026` …
    pub what: String,
    /// The copy `X` and `gx` leave unchecked: the first one, in a group with
    /// no live copy, so checking a whole group or the whole list is always a
    /// batch the server accepts. Still checkable by hand — the server's
    /// last-copy refusal stands behind that.
    pub keep: bool,
}

pub struct GroupView {
    /// `2980b480 · 2 copies · 412 KB`.
    pub head: String,
    pub copies: Vec<CopyView>,
}

#[derive(Template)]
#[template(path = "partials/prune_pane.html")]
pub struct PrunePane {
    pub t: Catalog,
    pub groups: Vec<GroupView>,
    pub count: String,
    pub empty: bool,
}

/// `GET /prune`.
pub async fn view(State(st): State<AppState>, auth: Auth) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return say_only(t.settings_admin_only(), true);
    }
    match pane(&st, &auth).await {
        Some(p) => Html(p).into_response(),
        None => say_only(t.error_not_written(), true),
    }
}

async fn pane(st: &AppState, auth: &Auth) -> Option<PrunePane> {
    let groups = match prune::scan(&st.pool, &st.cfg.receipts_root).await {
        Ok(g) => g,
        Err(e) => {
            tracing::error!("prune: scanning the vault: {e}");
            return None;
        }
    };
    let currency = currency(st).await;
    Some(build(auth.catalog(), &groups, currency))
}

async fn currency(st: &AppState) -> Currency {
    ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default()
}

/// The pane from what the scan found — pure, so it can be rendered from
/// fixtures as well as from a vault.
pub fn build(t: Catalog, groups: &[Group], currency: Currency) -> PrunePane {
    // What could go without losing a byte: every trashed copy, except that a
    // group made only of trashed copies has to keep one.
    let freeable: u64 = groups
        .iter()
        .map(|g| {
            let trashed = g.copies.iter().filter(|c| c.in_trash()).count();
            g.size * trashed.min(g.copies.len() - 1) as u64
        })
        .sum();
    let views = groups
        .iter()
        .map(|g| GroupView {
            head: t.prune_group(
                &g.hash[..8.min(g.hash.len())],
                g.copies.len() as i64,
                &fmt::bytes(g.size, currency),
            ),
            copies: {
                let all_trash = g.copies.iter().all(|c| c.in_trash());
                g.copies
                    .iter()
                    .enumerate()
                    .map(|(i, c)| CopyView {
                        rel: c.rel.clone(),
                        in_trash: c.in_trash(),
                        what: what(&t, &c.place),
                        keep: all_trash && i == 0,
                    })
                    .collect()
            },
        })
        .collect();
    PrunePane {
        count: t.prune_count(groups.len() as i64, &fmt::bytes(freeable, currency)),
        empty: groups.is_empty(),
        groups: views,
        t,
    }
}

fn what(t: &Catalog, place: &Place) -> String {
    let row = |r: &Record| {
        let period = r.period.map(fmt::period).unwrap_or_default();
        let kind = crate::web::trash::file_kind(t, &r.kind, r.income);
        format!("{} {} · {}", r.title, period, kind)
    };
    match place {
        Place::Live(Some(r)) => t.prune_live(&row(r)),
        Place::Live(None) => t.prune_live_unknown().to_string(),
        Place::Trash(Some(r)) => {
            let day = r
                .deleted_at
                .as_deref()
                .and_then(crate::ui::log::local_date)
                .map(fmt::date)
                .unwrap_or_default();
            t.prune_trash(&row(r), &day)
        }
        Place::Trash(None) => t.prune_trash_unknown().to_string(),
    }
}

/// `POST /prune` — every checked copy, or none of them.
pub async fn run(
    State(st): State<AppState>,
    auth: Auth,
    // Repeated keys — `pick=a&pick=b` — which a struct cannot take.
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return say_only(t.settings_admin_only(), true);
    }
    let picks: Vec<String> = pairs
        .into_iter()
        .filter(|(k, _)| k == "pick")
        .map(|(_, v)| v)
        .collect();
    if picks.is_empty() {
        return say_only(t.prune_nothing_picked(), false);
    }

    let done = prune::prune(
        &st.pool,
        &st.cfg.receipts_root,
        auth.viewer.member_id,
        &picks,
    )
    .await;
    let currency = currency(&st).await;
    let (said, bad) = match done {
        Ok(p) => (
            t.prune_done(p.files as i64, &fmt::bytes(p.bytes, currency)),
            false,
        ),
        Err(PruneError::Refused(r)) => {
            let msg = match r {
                Refusal::NotInTrash(p) => t.prune_not_in_trash(&p),
                Refusal::NotADuplicate(p) => t.prune_not_a_duplicate(&p),
                Refusal::LastCopy(p) => t.prune_last_copy(&p),
            };
            return say_only(&msg, true);
        }
        Err(PruneError::Io(e)) => {
            tracing::error!("prune: {e}");
            (t.prune_failed().to_string(), true)
        }
    };

    // The list again, from a fresh scan: what was removed is gone from it,
    // and a group left with one copy is not a group any more.
    match pane(&st, &auth).await.and_then(|p| p.render().ok()) {
        Some(html) => say(
            (
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                html,
            )
                .into_response(),
            &said,
            bad,
        ),
        None => say_only(&said, bad),
    }
}
