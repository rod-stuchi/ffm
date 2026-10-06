//! `GET /live` — the open page's end of the live channel (`services::live`).
//!
//! Server-Sent Events: one-way, built into axum, and all a page needs — the
//! server says which row changed and the page fetches it as itself. A session
//! is required, so a stream belongs to somebody; what it carries is ids and a
//! month, never a rendered row.
//!
//! **And it is how a signed-out device finds out.** "Sign out other devices",
//! an admin's "Sign out everywhere", a password reset and turning a member off
//! all revoke sessions, and a page whose session is gone used to carry on
//! looking signed in until its next click bounced to `/login`. Now every
//! stream checks its own session the moment any revoke happens
//! (`Live::sessions_revoked`), and the page whose session is gone gets one
//! `signedout` event and goes to `/login` at once. A WebSocket was not needed
//! for this: the signal only ever runs server to page, which is what this
//! stream already is.

use crate::services::live::Signal;
use crate::state::AppState;
use crate::web::session::Auth;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;

/// How often a stream re-checks its session with no signal at all. A revoke
/// made from another process — the console's `ffm reset-link` — cannot reach
/// this one's channel, so it is caught here instead, within this long.
const RECHECK: Duration = Duration::from_secs(30);

pub async fn stream(
    State(st): State<AppState>,
    auth: Auth,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (out, events) = mpsc::channel::<Result<Event, Infallible>>(32);
    let mut signals = st.live.subscribe();
    let mut stop = st.live.stopped();
    let pool = st.pool.clone();
    let sid = auth.session.id.clone();
    let member = auth.viewer.member_id;

    // One task per open page. It ends when the page goes away (the send
    // fails), when its session is revoked, or on shutdown — an SSE response
    // never finishes on its own, and graceful shutdown would otherwise wait
    // on it forever.
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(RECHECK);
        tick.tick().await; // the first tick is immediate; the session was just checked
        loop {
            let check = tokio::select! {
                sig = signals.recv() => match sig {
                    Ok(Signal::Row(ev)) => {
                        let ev = Event::default().event("row").data(ev.json());
                        if out.send(Ok(ev)).await.is_err() {
                            return;
                        }
                        false
                    }
                    // Only this member's own steps: another member never
                    // learns an upload is happening.
                    Ok(Signal::Progress(p)) => {
                        if p.member_id == member {
                            let ev = Event::default().event("progress").data(p.json());
                            if out.send(Ok(ev)).await.is_err() {
                                return;
                            }
                        }
                        false
                    }
                    Ok(Signal::Sessions) => true,
                    // A page that fell behind loses the oldest events, not the
                    // stream — and whatever it missed may have been a revoke.
                    Err(broadcast::error::RecvError::Lagged(_)) => true,
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                _ = tick.tick() => true,
                _ = stop.changed() => return,
            };
            if check && !session_alive(&pool, &sid).await {
                // Not an empty `data`: an SSE event with no data is never
                // dispatched — the stream would just close, and the page would
                // read that as a dropped connection and reconnect.
                let _ = out
                    .send(Ok(Event::default().event("signedout").data("1")))
                    .await;
                return;
            }
        }
    });

    Sse::new(ReceiverStream::new(events)).keep_alive(KeepAlive::default())
}

/// Whether this stream's session still exists. A database error answers yes:
/// signing somebody out because a query failed would be worse than a stream
/// that learns about a revoke one tick later.
async fn session_alive(pool: &sqlx::SqlitePool, sid: &str) -> bool {
    sqlx::query_scalar!(
        r#"SELECT 1 AS "one!: i64" FROM sessions WHERE id = ?1"#,
        sid
    )
    .fetch_optional(pool)
    .await
    .map(|row| row.is_some())
    .unwrap_or(true)
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
