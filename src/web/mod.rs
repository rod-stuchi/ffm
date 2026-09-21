//! HTTP. Routing, and the one place a template becomes a response.

pub mod template;

#[cfg(debug_assertions)]
pub mod sandbox;

use axum::Router;
use tower_http::services::ServeDir;

pub fn router() -> Router {
    let app = Router::new().nest_service("/static", ServeDir::new("static"));

    // Gated at the module AND at the mount. None of the sandbox exists in a
    // release binary, so there is no auth guard to forget.
    #[cfg(debug_assertions)]
    let app = app.merge(sandbox::router());

    app
}
