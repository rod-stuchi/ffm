//! Every change, to every open page — read off the change log.
//!
//! Every write in this app already appends to `change`: a save, a paid tick,
//! `dd` and `u`, an upload, a re-attribution, a rule edit, the materializer,
//! an amount read off a bill, the console. So the log is the one complete
//! record of what changed, and announcing from it means no handler can forget
//! to announce. (Only file changes used to reach another open page — through
//! the vault watcher — so a receipt uploaded on one computer showed paid there
//! and nowhere else.)
//!
//! Once a second the new entries are read and turned into events
//! (`services::live`): a row to re-fetch, or — when rows came, went or moved —
//! a pane to re-render. A batch is coalesced, so one `dd` of a row with three
//! files is one pane event, not four. Nothing is replayed at boot: a page
//! opened since loaded what is there.

use crate::services::live::{Live, LiveEvent};
use sqlx::SqlitePool;
use std::time::Duration;

const TICK: Duration = Duration::from_secs(1);

/// What one log entry asks of the open pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// The row itself changed: its amount, date, privacy, files, paid tick.
    Row,
    /// Rows came, went or moved within a member's pane.
    Pane,
    /// A row moved between two members' panes: every pane of the month.
    AllPanes,
    /// A rule changed: its member's pane, in every month.
    RuleEverywhere,
    /// Nothing a ledger shows (account actions).
    Nothing,
}

/// The pure half: which reach an entry has, from its entity and action.
pub fn reach(entity: &str, action: &str) -> Reach {
    match (entity, action) {
        ("transaction", "create" | "delete" | "restore" | "reorder") => Reach::Pane,
        ("transaction", "reattribute") => Reach::AllPanes,
        ("transaction", _) => Reach::Row,
        ("file", _) => Reach::Row,
        ("rule", _) => Reach::RuleEverywhere,
        _ => Reach::Nothing,
    }
}

/// Starts tailing the log from where it ends now.
pub fn spawn(pool: SqlitePool, live: Live) {
    tokio::spawn(async move {
        let mut last: i64 =
            sqlx::query_scalar!(r#"SELECT coalesce(max(id), 0) AS "n!: i64" FROM change"#)
                .fetch_one(&pool)
                .await
                .unwrap_or(0);
        let mut stop = live.stopped();
        let mut tick = tokio::time::interval(TICK);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = stop.changed() => return,
            }
            match drain(&pool, &live, last).await {
                Ok(n) => last = n,
                Err(e) => tracing::warn!("live feed: {e}"),
            }
        }
    });
}

/// Reads the entries after `after`, publishes what they reach, and returns
/// the last id read. Public for the tests, which call it rather than waiting
/// on the tick.
pub async fn drain(pool: &SqlitePool, live: &Live, after: i64) -> sqlx::Result<i64> {
    let entries = sqlx::query!(
        r#"SELECT id AS "id!: i64", entity AS "entity!: String", action AS "action!: String",
                  entity_id AS "entity_id!: i64"
           FROM change WHERE id > ?1 ORDER BY id LIMIT 500"#,
        after
    )
    .fetch_all(pool)
    .await?;
    let Some(last) = entries.last().map(|e| e.id) else {
        return Ok(after);
    };

    let mut events: Vec<LiveEvent> = Vec::new();
    let mut push = |ev: LiveEvent| {
        // A pane event already re-renders its rows: a row inside it is
        // covered, and the same event twice is one.
        if !events.contains(&ev) {
            events.push(ev);
        }
    };
    for e in &entries {
        match reach(&e.entity, &e.action) {
            Reach::Nothing => {}
            Reach::RuleEverywhere => {
                if let Some(member) = sqlx::query_scalar!(
                    r#"SELECT member_id AS "m!: i64" FROM recurring_rules WHERE id = ?1"#,
                    e.entity_id
                )
                .fetch_optional(pool)
                .await?
                {
                    push(LiveEvent::pane(member, None));
                }
            }
            reach => {
                // The row the entry is about: the transaction itself, or the
                // one a file belongs to. Its owner and month as they are now,
                // never the actor's.
                let row = if e.entity == "file" {
                    sqlx::query!(
                        r#"SELECT t.id AS "id!: i64", t.member_id AS "member!: i64",
                                  t.period AS "period!: String"
                           FROM receipts r JOIN transactions t ON t.id = r.transaction_id
                           WHERE r.id = ?1"#,
                        e.entity_id
                    )
                    .fetch_optional(pool)
                    .await?
                    .map(|r| (r.id, r.member, r.period))
                } else {
                    sqlx::query!(
                        r#"SELECT id AS "id!: i64", member_id AS "member!: i64",
                                  period AS "period!: String"
                           FROM transactions WHERE id = ?1"#,
                        e.entity_id
                    )
                    .fetch_optional(pool)
                    .await?
                    .map(|r| (r.id, r.member, r.period))
                };
                let Some((id, member, period)) = row else {
                    continue;
                };
                push(match reach {
                    Reach::Row => LiveEvent::row(id, member, period),
                    Reach::Pane => LiveEvent::pane(member, Some(period)),
                    _ => LiveEvent::pane(0, Some(period)),
                });
            }
        }
    }

    // A row whose pane is re-rendered anyway needs no fetch of its own.
    let panes: Vec<(i64, Option<String>)> = events
        .iter()
        .filter(|e| e.pane)
        .map(|e| (e.member_id, e.period.clone()))
        .collect();
    for ev in events {
        let covered = !ev.pane
            && panes
                .iter()
                .any(|(m, p)| (*m == 0 || *m == ev.member_id) && (p.is_none() || *p == ev.period));
        if !covered {
            live.publish(ev);
        }
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_each_entry_reaches() {
        assert_eq!(reach("transaction", "edit"), Reach::Row);
        assert_eq!(reach("transaction", "import"), Reach::Row);
        assert_eq!(reach("transaction", "privacy"), Reach::Row);
        assert_eq!(reach("transaction", "materialize"), Reach::Row);
        assert_eq!(reach("transaction", "create"), Reach::Pane);
        assert_eq!(reach("transaction", "delete"), Reach::Pane);
        assert_eq!(reach("transaction", "restore"), Reach::Pane);
        assert_eq!(reach("transaction", "reorder"), Reach::Pane);
        assert_eq!(reach("transaction", "reattribute"), Reach::AllPanes);
        assert_eq!(reach("file", "file_upload"), Reach::Row);
        assert_eq!(reach("file", "file_delete"), Reach::Row);
        assert_eq!(reach("rule", "rule_edit"), Reach::RuleEverywhere);
        assert_eq!(reach("member", "password_reset"), Reach::Nothing);
    }
}
