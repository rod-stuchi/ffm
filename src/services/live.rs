//! The live channel: a row changed, and every open page showing it should
//! know.
//!
//! **Ids, never HTML.** A row is rendered per viewer — their language, their
//! sort, and above all their privacy — so one rendered fragment would be
//! wrong for everybody else. An event names the row and its month; each page
//! that has the row on screen asks for it again (`GET /tx/{id}`) as itself.
//! The HTML still comes from a fragment: this is a signal, not a JSON API.

use tokio::sync::{broadcast, watch};

/// Something on a ledger changed. Either **one row** to re-fetch, or — when
/// rows came, went or moved (`pane`) — **a whole pane** to re-render.
/// `disk` is set when the change came from the vault by hand rather than
/// through the app, which the statusline says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveEvent {
    /// The row; 0 for a pane event.
    pub id: i64,
    /// The row's (or pane's) member; 0 for "every pane" — a re-attribution,
    /// which moves a row between two of them.
    pub member_id: i64,
    /// `YYYY-MM` — a page only cares about the month it shows. `None` for a
    /// change that reaches every month (a rule's).
    pub period: Option<String>,
    pub disk: bool,
    pub pane: bool,
}

impl LiveEvent {
    /// One row re-fetched.
    pub fn row(id: i64, member_id: i64, period: impl Into<String>) -> LiveEvent {
        LiveEvent {
            id,
            member_id,
            period: Some(period.into()),
            disk: false,
            pane: false,
        }
    }

    /// One member's pane re-rendered — `period: None` for every month,
    /// `member_id: 0` for every member.
    pub fn pane(member_id: i64, period: Option<String>) -> LiveEvent {
        LiveEvent {
            id: 0,
            member_id,
            period,
            disk: false,
            pane: true,
        }
    }

    pub fn json(&self) -> String {
        serde_json::json!({
            "id": self.id,
            "member": self.member_id,
            "period": self.period,
            "disk": self.disk,
            "pane": self.pane,
        })
        .to_string()
    }
}

/// What goes down the channel. A row to re-fetch, or word that some sessions
/// were just revoked.
///
/// **`Sessions` names nobody.** A session id is the cookie itself, so it never
/// travels — not even to the server's own stream tasks as a list to compare.
/// Each open stream asks the database whether *its own* session is still
/// there, and only the page whose session is gone hears anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signal {
    Row(LiveEvent),
    Sessions,
    Progress(Progress),
}

/// A step of a long request — "Shrinking", "Asking codex to read the bill" —
/// for the page that made it (`web::fidget`). Words, never data: no amount,
/// name or file content travels here. Only the member's own streams forward
/// it, and only the page that sent `job` shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub member_id: i64,
    /// The page's own random id for this request (`X-FFM-Job`).
    pub job: String,
    /// Already in the member's language.
    pub text: String,
}

impl Progress {
    pub fn json(&self) -> String {
        serde_json::json!({ "job": self.job, "text": self.text }).to_string()
    }
}

/// The sender half every handler and task shares, plus the switch that ends
/// every open stream on shutdown — an SSE response never finishes on its own,
/// and graceful shutdown would otherwise wait on it forever.
#[derive(Clone)]
pub struct Live {
    tx: broadcast::Sender<Signal>,
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
        let _ = self.tx.send(Signal::Row(ev));
    }

    /// Some sessions were revoked — signed out of other devices, everywhere,
    /// by a reset, or a member turned off. Every open page whose session is
    /// gone is told so at once and sent to `/login` (`web::live::stream`).
    pub fn sessions_revoked(&self) {
        let _ = self.tx.send(Signal::Sessions);
    }

    /// A step of a long request, for the page that is waiting on it. Nobody
    /// listening is not an error: the page may have no stream open.
    pub fn progress(&self, p: Progress) {
        let _ = self.tx.send(Signal::Progress(p));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Signal> {
        self.tx.subscribe()
    }

    pub fn stopped(&self) -> watch::Receiver<bool> {
        self.stop.clone()
    }
}
