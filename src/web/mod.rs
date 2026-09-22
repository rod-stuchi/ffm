//! HTTP. Routing, and the one place a template becomes a response.

pub mod auth;
pub mod csrf;
pub mod files;
pub mod import;
pub mod ledger;
pub mod log;
pub mod rules;
pub mod session;
pub mod settings;
pub mod template;
pub mod tx;

#[cfg(debug_assertions)]
pub mod sandbox;

use crate::state::AppState;
use axum::Router;
use axum::routing::{get, post};
use tower_http::services::ServeDir;

pub fn router(state: AppState) -> Router {
    // The three ways in, plus the two preferences that belong to a device
    // rather than to a member. All plain form POSTs, and all deliberately
    // OUTSIDE the same-origin guard: they are submitted from a page that may
    // have been opened cold, and a blanket check rejects every one of them and
    // makes the app impossible to enter. `SameSite=Lax` is what protects them,
    // which is sound here — a forged login POST gains an attacker nothing,
    // `/lang` has no security value, and setup and invite-accept are
    // single-use by construction.
    let open = Router::new()
        .route("/login", get(auth::login_page).post(auth::login))
        .route("/setup", get(auth::setup_page).post(auth::setup))
        .route("/invite/{token}", get(auth::invite_page).post(auth::accept))
        .route("/lang", post(auth::set_lang))
        .route("/theme", post(auth::set_theme));

    // Authenticated, state-changing: these carry the same-origin check.
    let guarded = Router::new()
        .route("/logout", post(auth::logout))
        .route("/preview", post(auth::toggle_preview))
        // Every mutation in the app goes through one response shape: a row
        // fragment, or an error row carrying Try Again. See web::tx.
        .route("/tx", post(tx::add))
        .route("/tx/{id}/delete", post(tx::delete))
        .route("/undo", post(tx::undo))
        .route("/tx/{id}/move", post(tx::move_row))
        // `e` — the row opens into its own form, in place.
        .route("/tx/{id}", get(tx::one_row).post(tx::save_edit))
        .route("/tx/{id}/edit", get(tx::edit_form))
        .route("/tx/{id}/privacy", post(tx::privacy))
        .route("/tx/{id}/member", post(tx::reattribute))
        // Files. Three distinct actions rather than one upload plus a
        // question: each kind has its own fixed name on disk and its own
        // effect on state.
        .route("/tx/{id}/file/{kind}", get(files::open).post(files::upload))
        .route("/tx/{id}/file/{kind}/delete", post(files::delete))
        // `gl` — read-only, and the same column the rule editor uses.
        .route("/tx/{id}/log", get(log::log))
        // `:grep` — every month, which is what makes it a different verb.
        .route("/grep", get(log::grep))
        // The rule pane. A / I open it blank, E on a recurring row opens it
        // filled in; saving reconciles immediately so the periods the rule is
        // responsible for arrive in the same response.
        .route("/rules/new", get(rules::new_rule))
        .route("/rules", post(rules::create))
        .route("/rules/{id}/edit", get(rules::edit_rule))
        .route("/rules/{id}", post(rules::save))
        .route("/rules/{id}/delete", post(rules::delete))
        // The ONE place a barcode file is ever attached: the rule's own
        // editor, where the rule is already identified.
        .route("/rules/{id}/barcode", post(import::attach))
        .route("/rules/{id}/unlink", post(import::unlink))
        // The settings surfaces. Vsplit panes with no Save button: every
        // control writes on `change`, so there is never a pending second copy
        // of the truth.
        .route("/profile", get(settings::profile))
        .route("/household", get(settings::household))
        .route("/members", get(settings::members_view))
        .route("/settings/member", post(settings::set_member))
        .route("/settings/household", post(settings::set_household))
        .route("/members/invite", post(settings::invite))
        .route("/members/{id}/reset", post(settings::reset))
        .route("/members/{id}/signout", post(settings::sign_out))
        .route("/members/invites/{id}/revoke", post(settings::revoke))
        .layer(axum::middleware::from_fn(csrf::guard));

    let app = Router::new()
        .route("/", get(ledger::ledger))
        .merge(open)
        .merge(guarded)
        .nest_service("/static", ServeDir::new("static"));

    // Gated at the module AND at the mount. None of the sandbox exists in a
    // release binary, so there is no auth guard to forget.
    #[cfg(debug_assertions)]
    let app = app.merge(sandbox::router());

    app.with_state(state)
}
