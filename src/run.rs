//! Which kind of server this is: **PROD**, **dev** or **dev-lan** — and
//! whether a dev server runs as of another date (`just dev 2026-10-15`).
//!
//! It matters because they look identical. A dev server on a copy of the
//! database, or one living three weeks in the future, is exactly where
//! somebody types a real bill into the wrong ledger. So a dev build says so
//! on every screen — a badge beside the household's name that no width hides,
//! a `[dev]` prefix on the tab's title, the same badge on `/login` — while
//! PROD says it only in `:profile` and the boot log: a badge that is always
//! there stops being read, and the danger runs one way.
//!
//! - **PROD** is a release build, whatever it binds to.
//! - **dev** is a debug build on loopback, **dev-lan** one bound to an
//!   interface the rest of the house can reach.
//! - **the date** is the moved clock's today (`clock::shifted`), which only a
//!   debug build can have.

use std::sync::OnceLock;

use crate::ui::i18n::Catalog;

static LAN: OnceLock<bool> = OnceLock::new();

/// Record what the listener bound to. Called once, after the bind — before it
/// nothing has been served, so nothing has asked.
pub fn bound(ip: std::net::IpAddr) {
    let _ = LAN.set(!ip.is_loopback());
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Prod,
    Dev,
    DevLan,
}

pub fn kind() -> Kind {
    if !cfg!(debug_assertions) {
        Kind::Prod
    } else if LAN.get().copied().unwrap_or(false) {
        Kind::DevLan
    } else {
        Kind::Dev
    }
}

impl Kind {
    /// The mode's name as the operator knows it — the recipe that starts it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Prod => "PROD",
            Kind::Dev => "dev",
            Kind::DevLan => "dev-lan",
        }
    }
}

/// The moved clock's today, formatted — `None` on the real clock.
fn demo_date() -> Option<String> {
    crate::clock::shifted().then(|| crate::ui::fmt::date(crate::clock::today()))
}

/// `PROD` · `dev` · `dev-lan · 15/10/2026` — `:profile`'s first line and the
/// boot log.
pub fn line() -> String {
    match demo_date() {
        Some(d) => format!("{} · {d}", kind().name()),
        None => kind().name().to_string(),
    }
}

/// What a non-PROD server wears on every screen.
pub struct Badge {
    /// `DEV` · `DEV-LAN` — an identifier, so no catalog; the sentence is in
    /// `title`.
    pub kind: String,
    /// A demo's date in two pieces, `15/10` and `/2026`, so a phone can drop
    /// the year: there the badge shares row 1 with the household's name, and
    /// the whole date left that name reading "No…".
    pub day: Option<String>,
    pub year: String,
    pub title: String,
    /// The tab title's prefix: `[dev] `, `[dev-lan] `, `[dev 15/10] `.
    pub tab: String,
}

/// `None` for PROD, which wears nothing.
pub fn badge(t: &Catalog) -> Option<Badge> {
    let k = kind();
    if k == Kind::Prod {
        return None;
    }
    let date = demo_date();
    let (day, year) = match &date {
        Some(d) => (Some(d[..5].to_string()), d[5..].to_string()),
        None => (None, String::new()),
    };
    let title = match (&date, k) {
        (Some(d), _) => t.run_demo_title(d),
        (None, Kind::DevLan) => t.run_dev_lan_title().to_string(),
        (None, _) => t.run_dev_title().to_string(),
    };
    let tab = match &date {
        // `15/10`, not the whole date: a tab title is short, and the badge
        // itself carries the year.
        Some(d) => format!("[{} {}] ", k.name(), &d[..5]),
        None => format!("[{}] ", k.name()),
    };
    Some(Badge {
        kind: k.name().to_uppercase(),
        day,
        year,
        title,
        tab,
    })
}
