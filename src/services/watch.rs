//! Watching the receipt vault for files changed by hand.
//!
//! The vault is meant to be browsed and filed by hand, and `paid` is already
//! read from the disk on every render — so the ledger is never wrong after a
//! hand edit. What a hand edit leaves undone is the **record** (a `receipts`
//! row, a change-log entry) and the **open pages**, which only see it on their
//! next load. This closes both: every change under the vault is reconciled
//! (`receipts::reconcile_row`) and pushed (`live`).
//!
//! - **The whole vault, recursively.** FSEvents and inotify both make that
//!   cheap at household scale, so there is no "watch only the month on
//!   screen" to keep in step with every open page.
//! - **Debounced, by path.** A Finder copy is a burst of create-then-modify; a
//!   path is acted on once it has been quiet for [`QUIET`]. That pause is also
//!   what lets the app's own writes commit their row before anybody looks, so
//!   reconciling one finds nothing to log — which is why no list of the app's
//!   own writes is kept.
//! - **A path names a member and a month, not a row.** Its first segment is
//!   the member's folder, the year is its folder, and the month is the file
//!   name's first two digits. Parsing the bucket and bill name back out of a
//!   lossy slug is avoided altogether: that member's tracked rows in that
//!   month are a handful, and all of them are reconciled.
//! - **The trash is never watched for meaning.** `.delete/` is where the app
//!   puts files; anything there, and dotfiles and half-downloaded files, are
//!   ignored.
//!
//! A watcher that cannot start is logged and leaves the app exactly as it was
//! before this existed.

use crate::domain::period::Period;
use crate::services::live::{Live, LiveEvent};
use crate::services::receipts;
use notify::{RecursiveMode, Watcher};
use sqlx::SqlitePool;
use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a path must be left alone before it is acted on.
pub const QUIET: Duration = Duration::from_millis(1500);

/// Where a changed path belongs: a member's folder, and the month when the
/// path says one. A whole folder going (a year, a bill) says no month, and
/// then every tracked row of that member is reconciled.
pub fn locate(root: &Path, path: &Path) -> Option<(String, Option<Period>)> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<&str> = rel
        .components()
        .map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let (slug, rest) = parts.split_first()?;
    // `.delete/` is the app's own trash; a dotfile is the system's.
    if parts.iter().any(|p| p.starts_with('.')) {
        return None;
    }
    if let Some(name) = parts.last()
        && (name.starts_with("~$") || name.ends_with(".part") || name.ends_with(".crdownload"))
    {
        return None;
    }
    if rest.is_empty() {
        return None;
    }
    let period = (rest.len() >= 2)
        .then(|| {
            let year = rest[rest.len() - 2];
            let name = rest[rest.len() - 1];
            let month = name.get(..2)?;
            (year.len() == 4 && month.bytes().all(|b| b.is_ascii_digit()))
                .then(|| Period::parse(&format!("{year}-{month}")))
                .flatten()
        })
        .flatten();
    Some((slug.to_string(), period))
}

/// Reconciles one member's rows — of one month, or all of them — and pushes
/// each to the open pages. Within a month every row is pushed, so another
/// member's page catches up with an upload made through the app too; across a
/// whole member only the rows that actually changed are, rather than a page's
/// worth of re-fetches for nothing.
async fn settle(pool: &SqlitePool, root: &Path, live: &Live, slug: &str, period: Option<Period>) {
    let rows = match receipts::tracked_in(pool, slug, period).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("watch · cannot read rows for {slug}: {e}");
            return;
        }
    };
    for (id, member_id, p) in rows {
        let done = match receipts::reconcile_row(pool, root, id).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("watch · cannot reconcile row {id}: {e}");
                continue;
            }
        };
        if done.changed() {
            tracing::info!(
                "files · row {id} · {} found on disk, {} removed by hand",
                done.found,
                done.removed
            );
        }
        if period.is_some() || done.changed() {
            live.publish(LiveEvent {
                id,
                member_id,
                period: p,
                disk: done.changed(),
            });
        }
    }
}

/// Starts watching `root`. Never fatal: logged, and the app runs as before.
pub fn spawn(pool: SqlitePool, root: PathBuf, live: Live) {
    // Events arrive as absolute, resolved paths (`/private/var/…` on macOS),
    // so the root they are measured against has to be resolved too.
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            for p in ev.paths {
                let _ = tx.send(p);
            }
        }
    });
    let mut watcher = match watcher {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("watch · cannot start: {e} — files changed by hand show on reload only");
            return;
        }
    };
    if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
        tracing::warn!(
            "watch · cannot watch {}: {e} — files changed by hand show on reload only",
            root.display()
        );
        return;
    }
    tracing::info!("watch · {}", root.display());

    tokio::spawn(async move {
        // Owned by the task, so it lives exactly as long as the loop does.
        let _watcher = watcher;
        let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
        let mut tick = tokio::time::interval(Duration::from_millis(300));
        loop {
            tokio::select! {
                got = rx.recv() => match got {
                    Some(p) => { pending.insert(p, Instant::now()); }
                    None => return,
                },
                _ = tick.tick() => {
                    let ready: Vec<PathBuf> = pending
                        .iter()
                        .filter(|(_, at)| at.elapsed() >= QUIET)
                        .map(|(p, _)| p.clone())
                        .collect();
                    let mut groups: BTreeSet<(String, Option<Period>)> = BTreeSet::new();
                    for p in ready {
                        pending.remove(&p);
                        if let Some(g) = locate(&root, &p) {
                            groups.insert(g);
                        }
                    }
                    for (slug, period) in groups {
                        settle(&pool, &root, &live, &slug, period).await;
                    }
                }
            }
        }
    });
}

/// Catches up on what changed while the process was down: every live tracked
/// row, reconciled once. Its own task, started after the listener is bound —
/// the server answers from the first moment, and the log fills in behind it.
pub fn catch_up(pool: SqlitePool, root: PathBuf, live: Live) {
    tokio::spawn(async move {
        let rows = match receipts::all_tracked(&pool).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("files · cannot catch up: {e}");
                return;
            }
        };
        let (mut found, mut removed) = (0, 0);
        for (id, member_id, period) in rows {
            match receipts::reconcile_row(&pool, &root, id).await {
                Ok(d) => {
                    found += d.found;
                    removed += d.removed;
                    if d.changed() {
                        live.publish(LiveEvent {
                            id,
                            member_id,
                            period,
                            disk: true,
                        });
                    }
                }
                Err(e) => tracing::warn!("files · cannot reconcile row {id}: {e}"),
            }
        }
        tracing::info!("files · {found} found on disk, {removed} removed by hand while away");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(rel: &str) -> Option<(String, Option<String>)> {
        locate(Path::new("/v"), &Path::new("/v").join(rel))
            .map(|(s, p)| (s, p.map(|p| p.to_string())))
    }

    #[test]
    fn a_path_names_a_member_and_a_month() {
        assert_eq!(
            at("udo/consumo/cpfl/2026/09.pdf"),
            Some(("udo".into(), Some("2026-09".into())))
        );
        assert_eq!(
            at("udo/consumo/cpfl/casa_2/2026/10_barcode.pdf"),
            Some(("udo".into(), Some("2026-10".into())))
        );
        assert_eq!(
            at("udo/consumo/cpfl/2026/09_brl122.94.pdf"),
            Some(("udo".into(), Some("2026-09".into())))
        );
        // A folder going says no month: the whole member is reconciled.
        assert_eq!(at("udo/consumo/cpfl/2026"), Some(("udo".into(), None)));
    }

    #[test]
    fn the_trash_dotfiles_and_half_downloads_are_not_changes() {
        assert_eq!(at("udo/.delete/consumo/cpfl/2026/09.r4.pdf"), None);
        assert_eq!(at("udo/consumo/cpfl/2026/.DS_Store"), None);
        assert_eq!(at(".ffm-write-probe"), None);
        assert_eq!(at("udo/consumo/cpfl/2026/09.pdf.crdownload"), None);
        assert_eq!(at("udo/consumo/cpfl/2026/~$09.pdf"), None);
        // Outside the vault altogether.
        assert_eq!(
            locate(Path::new("/v"), Path::new("/elsewhere/09.pdf")),
            None
        );
    }
}
