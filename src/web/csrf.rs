//! CSRF, sized for this deployment.
//!
//! No token scheme: not proportionate for a closed household on a LAN, and a
//! token is a second piece of state to get wrong. What actually does the work
//! is `SameSite=Lax` on the session cookie — a cross-site POST carries no
//! cookie at all, so a forged request arrives unauthenticated. This layer is
//! the belt to that pair of braces.
//!
//! **It applies to authenticated state-changing routes only.** `/login`,
//! `/setup`, `/invite/:token` and `/lang` are plain form POSTs from a page that
//! may have been opened cold; a blanket middleware rejects all four and makes
//! the app impossible to enter. This is written down because the obvious "fix"
//! is to apply it everywhere.
//!
//! The check accepts two proofs, not one:
//!
//! * `HX-Request` — HTMX sets it on every request it makes, and a cross-origin
//!   form cannot add a custom header without a preflight this server never
//!   answers.
//! * `Sec-Fetch-Site: same-origin` (or `none`, meaning the user typed it) —
//!   which browsers attach to plain form posts, and which a page on another
//!   origin cannot forge.
//!
//! Requiring only the first would mean every control has to be HTMX before it
//! can be safe, which is backwards: a plain `<form>` is the more robust thing.

use axum::extract::Request;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

pub fn is_same_origin(headers: &HeaderMap) -> bool {
    if headers.contains_key("hx-request") {
        return true;
    }
    match headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        Some("same-origin") | Some("same") | Some("none") => true,
        Some(_) => false,
        // No Sec-Fetch-Site at all: an older browser, or curl. Allowed, because
        // refusing would break the app for a browser that has no way to comply,
        // and SameSite=Lax is still standing behind this.
        None => true,
    }
}

pub async fn guard(req: Request, next: Next) -> Result<Response, StatusCode> {
    let changes_state = !matches!(
        req.method(),
        &Method::GET | &Method::HEAD | &Method::OPTIONS
    );
    if changes_state && !is_same_origin(req.headers()) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn htmx_and_same_origin_forms_both_pass() {
        assert!(is_same_origin(&headers(&[("hx-request", "true")])));
        assert!(is_same_origin(&headers(&[(
            "sec-fetch-site",
            "same-origin"
        )])));
        // Typed into the address bar, or followed from a bookmark.
        assert!(is_same_origin(&headers(&[("sec-fetch-site", "none")])));
    }

    #[test]
    fn a_post_from_another_origin_does_not() {
        assert!(!is_same_origin(&headers(&[(
            "sec-fetch-site",
            "cross-site"
        )])));
        assert!(!is_same_origin(&headers(&[(
            "sec-fetch-site",
            "same-site"
        )])));
    }

    #[test]
    fn a_browser_that_says_nothing_is_allowed_through() {
        // SameSite=Lax is still standing behind this, and refusing would break
        // the app for a client with no way to comply.
        assert!(is_same_origin(&HeaderMap::new()));
    }
}
