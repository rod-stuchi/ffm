//! The live channel: a row changed, and every open page showing it should
//! know.
//!
//! **Ids, never HTML.** A row is rendered per viewer — their language, their
//! sort, and above all their privacy — so one rendered fragment would be
//! wrong for everybody else. An event names the row and its month; each page
//! that has the row on screen asks for it again (`GET /tx/{id}`) as itself.
//! The HTML still comes from a fragment: this is a signal, not a JSON API.

use std::cell::RefCell;
use std::future::Future;
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
    /// The change-log entries it announces (`change.id`), so the page whose
    /// own answer already drew them can let it pass (`Made`). Empty for a
    /// file changed by hand in the vault, which no page made.
    pub changes: Vec<i64>,
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
            changes: Vec::new(),
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
            changes: Vec::new(),
        }
    }

    /// The same request of a page, whatever changes it carries.
    pub fn same(&self, other: &LiveEvent) -> bool {
        (self.id, self.member_id, &self.period, self.disk, self.pane)
            == (
                other.id,
                other.member_id,
                &other.period,
                other.disk,
                other.pane,
            )
    }

    /// Whether a page acting on this pane event re-renders the row `other`
    /// names: its member's, or every member's (0), in its month or in every
    /// month (`None`).
    pub fn covers(&self, other: &LiveEvent) -> bool {
        self.pane
            && !other.pane
            && (self.member_id == 0 || self.member_id == other.member_id)
            && (self.period.is_none() || self.period == other.period)
    }

    pub fn json(&self) -> String {
        serde_json::json!({
            "id": self.id,
            "member": self.member_id,
            "period": self.period,
            "disk": self.disk,
            "pane": self.pane,
            "changes": self.changes,
        })
        .to_string()
    }
}

// --- the page that made a change -----------------------------------------

/// What one write made and what its answer drew, gathered while it runs
/// (`web::live::claim`): the entries `Change::write` appended, and the rows
/// and panes rendered after them.
///
/// **The page that made a change has been answered with it already.** The
/// feed announces every change to every page, its maker's included, so every
/// `J` drew its pane twice — the answer, then `/panes` a second later — and a
/// row walked by `J` a press at a time could show a step behind, when a pane
/// rendered before the next move landed after it. An answer now names the
/// changes it drew in full (`X-FFM-Changes`), and the page lets an event made
/// only of those pass. **Only those**: `dd` answers with the struck-through
/// row while its pane lost a line, so the delete is not claimed and its
/// event still comes back to the page that made it.
#[derive(Debug, Default)]
pub struct Made {
    pub changes: Vec<i64>,
    /// What was drawn, with how many changes had been made by then: a
    /// drawing shows only the changes before it.
    pub drawn: Vec<(Drawn, usize)>,
}

/// A row or a pane an answer rendered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drawn {
    Row(i64),
    /// A member's pane, in one month (`YYYY-MM`).
    Pane(i64, String),
}

impl Drawn {
    /// Whether this drawing is all a page would re-fetch for `ev`. A pane
    /// for every month (a rule's) is this pane: an answer is drawn for the
    /// month its page shows.
    pub fn shows(&self, ev: &LiveEvent) -> bool {
        match self {
            Drawn::Row(id) => !ev.pane && ev.id == *id,
            Drawn::Pane(member, period) => {
                ev.member_id == *member && ev.period.as_ref().is_none_or(|p| p == period)
            }
        }
    }
}

tokio::task_local! {
    static MADE: RefCell<Made>;
}

/// Runs one request with its record open, and hands back what it made and
/// drew.
pub async fn recording<F: Future>(f: F) -> (F::Output, Made) {
    MADE.scope(RefCell::new(Made::default()), async move {
        let out = f.await;
        (out, MADE.with(|m| m.take()))
    })
    .await
}

/// An entry was appended. Outside a request — the materializer's tick, the
/// vault watcher, the console — there is no record and nothing to do; work a
/// handler spawns has none either, so its changes are nobody's own.
pub fn made(change_id: i64) {
    let _ = MADE.try_with(|m| m.borrow_mut().changes.push(change_id));
}

/// A row or pane was rendered for the answer.
pub fn drew(d: Drawn) {
    let _ = MADE.try_with(|m| {
        let mut m = m.borrow_mut();
        let n = m.changes.len();
        m.drawn.push((d, n));
    });
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
