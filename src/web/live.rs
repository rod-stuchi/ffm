//! `GET /live` — the open page's end of the live channel (`services::live`).
//!
//! Server-Sent Events: one-way, built into axum, and all a page needs — the
//! server says which row changed and the page fetches it as itself. A session
//! is required, so a stream belongs to somebody; what it carries is ids and a
//! month, never a rendered row.

use crate::services::live::LiveEvent;
use crate::state::AppState;
use crate::web::session::Auth;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use std::convert::Infallible;
use tokio_stream::wrappers::{BroadcastStream, WatchStream};
use tokio_stream::{Stream, StreamExt};

pub async fn stream(
    State(st): State<AppState>,
    _auth: Auth,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rows = BroadcastStream::new(st.live.subscribe())
        // A page that fell behind loses the oldest events, not the stream;
        // the next one still arrives.
        .filter_map(|ev| ev.ok())
        .map(Some::<LiveEvent>);
    // The switch `main` trips on shutdown, as one `None` that ends the stream.
    let stop = WatchStream::new(st.live.stopped())
        .filter(|stopped| *stopped)
        .map(|_| None);
    let events = rows.merge(stop).take_while(|ev| ev.is_some()).map(|ev| {
        let ev = ev.expect("take_while keeps only Some");
        Ok(Event::default().event("row").data(ev.json()))
    });
    Sse::new(events).keep_alive(KeepAlive::default())
}

/// Marks a successful mutation with `ffm:dot`, so the page re-fetches the
/// month picker's trigger (`GET /months/dot`). Layered on exactly the routes
/// that can move a month in or out of "overdue" — files, row deletes and
/// restores, undo/redo, privacy, re-attribution, rule writes, members turned
/// off and on — listed once in `web::router`, so no handler has to remember.
///
/// Only a non-GET, and only a success: reading a row moves nothing. The flag
/// is merged into whatever `HX-Trigger` the handler already set (the
/// statusline's `ffm:say` rides there), never written over it.
pub async fn mark_dot(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mutates = req.method() != axum::http::Method::GET;
    let mut res = next.run(req).await;
    if !mutates || !res.status().is_success() {
        return res;
    }
    let mut events = res
        .headers()
        .get("HX-Trigger")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(s).ok())
        .unwrap_or_default();
    events.insert("ffm:dot".into(), serde_json::Value::Bool(true));
    let json = crate::web::tx::ascii_json(&serde_json::Value::Object(events).to_string());
    if let Ok(v) = axum::http::HeaderValue::from_str(&json) {
        res.headers_mut().insert("HX-Trigger", v);
    }
    res
}
