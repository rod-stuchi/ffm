//! Rendering an Askama template as an axum response.
//!
//! Askama 0.14 dropped `askama_axum`, so this newtype is what every handler
//! wraps its template in. It exists as one obvious local pattern to copy,
//! because nearly every tutorial and recalled snippet still reaches for the
//! crate that no longer exists.

use askama::Template;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

pub struct Html<T>(pub T);

impl<T: Template> IntoResponse for Html<T> {
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(body) => {
                ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
            }
            // A template that fails to render is a bug, not a user error, so it
            // says so plainly rather than rendering a half-page.
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("template error: {e}"),
            )
                .into_response(),
        }
    }
}
