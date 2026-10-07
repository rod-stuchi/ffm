//! `GET /_dev/reload` — the browser's end of `just dev`'s reload, and only
//! that: a dev build with the `dev-reload` feature, never a release.
//!
//! It replaced `tower-livereload`, whose page script reloaded on the first
//! message after **any** reconnect, and whose stream sent nothing after its
//! first message. Behind the HTTPS proxy that fronts `just dev-lan`, an idle
//! stream is cut after 60 seconds — so the page reloaded about once a minute
//! on a server that had never restarted, and a half-typed form went with it.
//!
//! Two things fix it, and both are needed:
//!
//! - **The stream names the process** (`BOOT`), and the page reloads only
//!   when the name changes. A reconnect to the same server is just a
//!   reconnect; a restarted server is a new name.
//! - **Keep-alives**, so a proxy has no idle stream to cut in the first place.

use crate::state::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use std::convert::Infallible;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// This process, by when it started. Any value that differs between two runs
/// would do; it is never compared for order.
static BOOT: LazyLock<String> = LazyLock::new(|| {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_default()
});

/// No session: the pre-auth pages and the sandbox reload too.
pub async fn stream(State(st): State<AppState>) -> impl axum::response::IntoResponse {
    let (out, events) = mpsc::channel::<Result<Event, Infallible>>(1);
    let mut stop = st.live.stopped();
    let boot = Event::default()
        .event("boot")
        .data(BOOT.as_str())
        .retry(Duration::from_secs(1));
    // One event, then the sender is held until shutdown or until the page goes
    // away — an SSE response never finishes on its own, and graceful shutdown
    // would otherwise wait on it, which is every cargo-watch restart.
    tokio::spawn(async move {
        if out.send(Ok(boot)).await.is_err() {
            return;
        }
        tokio::select! {
            _ = stop.wait_for(|stopped| *stopped) => {}
            _ = out.closed() => {}
        }
    });
    // `X-Accel-Buffering: no` — an nginx in front buffers proxied responses
    // by default, which would hold every event back until its buffer filled.
    (
        [("x-accel-buffering", "no")],
        Sse::new(ReceiverStream::new(events)).keep_alive(KeepAlive::default()),
    )
}
