//! The live channel: a row changed, and every open page showing it should
//! know.
//!
//! **Ids, never HTML.** A row is rendered per viewer — their language, their
//! sort, and above all their privacy — so one rendered fragment would be
//! wrong for everybody else. An event names the row and its month; each page
//! that has the row on screen asks for it again (`GET /tx/{id}`) as itself.
//! The HTML still comes from a fragment: this is a signal, not a JSON API.

use tokio::sync::{broadcast, watch};

/// One row to re-fetch. `disk` is set when the change came from the vault by
/// hand rather than through the app, which the statusline says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveEvent {
    pub id: i64,
    pub member_id: i64,
    /// `YYYY-MM` — a page only cares about rows in the month it shows.
    pub period: String,
    pub disk: bool,
}

impl LiveEvent {
    pub fn json(&self) -> String {
        serde_json::json!({
            "id": self.id,
            "member": self.member_id,
            "period": self.period,
            "disk": self.disk,
        })
        .to_string()
    }
}

/// The sender half every handler and task shares, plus the switch that ends
/// every open stream on shutdown — an SSE response never finishes on its own,
/// and graceful shutdown would otherwise wait on it forever.
#[derive(Clone)]
pub struct Live {
    tx: broadcast::Sender<LiveEvent>,
    stop: watch::Receiver<bool>,
}

impl Live {
    /// The channel, and the switch `main` trips when the process is told to
    /// stop.
    pub fn new() -> (Live, watch::Sender<bool>) {
        let (tx, _) = broadcast::channel(256);
        let (stop_tx, stop) = watch::channel(false);
        (Live { tx, stop }, stop_tx)
    }

    /// Nobody listening is not an error: no page is open.
    pub fn publish(&self, ev: LiveEvent) {
        let _ = self.tx.send(ev);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LiveEvent> {
        self.tx.subscribe()
    }

    pub fn stopped(&self) -> watch::Receiver<bool> {
        self.stop.clone()
    }
}
