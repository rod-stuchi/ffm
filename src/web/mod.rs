//! HTTP. Routing, and the one place a template becomes a response.

pub mod ledger;
pub mod template;

#[cfg(debug_assertions)]
pub mod sandbox;

use crate::state::AppState;
use axum::Router;
use axum::routing::get;
use tower_http::services::ServeDir;

pub fn router(state: AppState) -> Router {
    let app = Router::new()
        .route("/", get(ledger::ledger))
        .nest_service("/static", ServeDir::new("static"));

    // Gated at the module AND at the mount. None of the sandbox exists in a
    // release binary, so there is no auth guard to forget.
    #[cfg(debug_assertions)]
    let app = app.merge(sandbox::router());

    app.with_state(state)
}
