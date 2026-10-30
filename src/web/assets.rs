//! `/static` URLs that change when their file does.
//!
//! A page names its stylesheet and scripts as `/static/css/build.css?v=…`,
//! the `v` a hash of the file's own bytes — so the browser can keep them for a
//! year (`immutable`) and a month change sends only the HTML, and a deploy
//! that changed one file is a new URL for that file alone. A version number
//! would have moved every URL on every release, and stood still across a
//! debug build's edits.
//!
//! Only a URL whose `v` is the file's hash **now** is kept for good. One a page
//! loaded before a deploy still names the old hash, and gets the new file with
//! no promise attached — rather than the new bytes cached under the old name.
//! Fonts and icons carry no `v` (the stylesheet names the fonts, and it cannot
//! be templated) and keep the browser's ordinary caching, as before.

use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// What each file under `static/` hashed to, by its path there.
fn hashes() -> &'static Mutex<HashMap<String, String>> {
    static HASHES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    HASHES.get_or_init(Default::default)
}

/// `/static/{path}?v={hash}` — or the bare URL when the file cannot be read,
/// which is then served the way it always was.
///
/// A release reads each file once: its `static/` changes only on a deploy,
/// which restarts it. A debug build reads it on every call, so a stylesheet
/// rebuilt by `just css` is a new URL on the next load even without the
/// `dev-reload` feature's `no-cache`.
pub fn url(path: &str) -> String {
    let mut known = hashes().lock().unwrap_or_else(|e| e.into_inner());
    if cfg!(debug_assertions) || !known.contains_key(path) {
        match std::fs::read(std::path::Path::new("static").join(path)) {
            Ok(bytes) => {
                known.insert(
                    path.to_string(),
                    blake3::hash(&bytes).to_hex()[..12].to_string(),
                );
            }
            Err(_) => {
                known.remove(path);
            }
        }
    }
    match known.get(path) {
        Some(v) => format!("/static/{path}?v={v}"),
        None => format!("/static/{path}"),
    }
}

/// `/static`'s cache rule: a URL [`url`] made, with the hash still current, is
/// kept for a year. Nothing here reads a file — a path no page has named has
/// no hash to match.
pub async fn cache(req: Request, next: Next) -> Response {
    let path = req.uri().path().trim_start_matches('/').to_string();
    let v = req
        .uri()
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("v=")))
        .map(str::to_string);
    let mut res = next.run(req).await;
    let current = v.is_some_and(|v| {
        hashes()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&path)
            .is_some_and(|h| *h == v)
    });
    if current && res.status().is_success() {
        res.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    res
}
