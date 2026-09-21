//! HTTP. Routing, and the one place a template becomes a response.

pub mod auth;
pub mod csrf;
pub mod ledger;
pub mod session;
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
