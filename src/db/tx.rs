//! Writing a transaction: add, soft delete, restore — and undo.
//!
//! Every function here opens one SQLite transaction and writes **both** the row
//! and its change-log entry inside it. That is not a convention to remember: the
//! log's writer takes `&mut Transaction`, so there is no way to do one without
//! the other.
//!
//! Nothing is ever hard-deleted. `dd` sets a mark; `u` clears it on the same
//! row — restore is in place, there is no superseding clone, so one delete plus
//! one undo leaves exactly one row in the ledger.

use crate::db::changes::{Actor, Change, FileAction, TxAction, Value};
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::sortkey;
use crate::domain::status::{TxKind, Visibility};
use chrono::{Datelike, NaiveDate};
use sqlx::SqlitePool;

fn now() -> String {
    crate::clock::stamp()
}

pub struct NewTransaction<'a> {
    pub member_id: i64,
    pub period: Period,
    pub kind: TxKind,
    pub title: &'a str,
    pub amount: Option<Cents>,
    pub date: Option<NaiveDate>,
    pub bucket: Option<&'a str>,
    pub visibility: Visibility,
}

/// Adds a one-off row and returns its id.
///
/// **Where it lands is a decision, not an accident.** A spend appends to the
/// bottom of the pane, where a household adds to a list. Income goes to the
/// top, which is where they read what came in — a starting position, not a pin:
/// `J`/`K` move it afterwards and an explicit sort ignores it entirely.
pub async fn add(pool: &SqlitePool, actor: Actor, new: NewTransaction<'_>) -> sqlx::Result<i64> {
    let period = new.period.to_string();
    let mut tx = pool.begin().await?;

    // The neighbor this row is going next to. Only that one row's key is read,
    // and none is rewritten — the whole point of a fractional index.
    let neighbor: Option<String> = if new.kind.is_income() {
        sqlx::query_scalar!(
            r#"SELECT min(sort_key) AS "k: String" FROM transactions
               WHERE member_id = ?1 AND period = ?2 AND deleted_at IS NULL"#,
            new.member_id,
            period
        )
        .fetch_one(&mut *tx)
        .await?
    } else {
        sqlx::query_scalar!(
            r#"SELECT max(sort_key) AS "k: String" FROM transactions
               WHERE member_id = ?1 AND period = ?2 AND deleted_at IS NULL"#,
            new.member_id,
            period
        )
        .fetch_one(&mut *tx)
        .await?
    };

    let sort_key = match (new.kind.is_income(), neighbor.as_deref()) {
        (true, Some(first)) => sortkey::before(first),
        (false, Some(last)) => sortkey::after(last),
        (_, None) => sortkey::between(None, None),
    };

    let at = now();
    let kind = new.kind.code();
    let visibility = new.visibility.code();
    let amount = new.amount.map(|c| c.get());
    let date = new.date.map(|d| d.format("%Y-%m-%d").to_string());
    let author = actor_id(actor);

    let id = sqlx::query_scalar!(
        r#"INSERT INTO transactions
               (member_id, rule_id, period, kind, title, amount_cents, date,
                manual_paid, visibility, bucket, sort_key, created_at, created_by)
           VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9, ?10, ?11)
           RETURNING id AS "id!: i64""#,
        new.member_id,
        period,
        kind,
        new.title,
        amount,
        date,
        visibility,
        new.bucket,
        sort_key,
        at,
        author,
    )
    .fetch_one(&mut *tx)
    .await?;

    // A create records what it created, so `gl` can answer "where did this row
    // come from" without inferring it from the row's current contents.
    Change::transaction(actor, TxAction::Create, id)
        .in_period(new.period)
        .diff("title", Value::Text(None), Value::text(new.title))
        .diff("amount", Value::Money(None), Value::Money(new.amount))
        .diff("date", Value::Date(None), Value::Date(new.date))
        .write(&mut tx)
        .await?;

    tx.commit().await?;
    Ok(id)
}

fn actor_id(actor: Actor) -> Option<i64> {
    match actor {
        Actor::User(id) => Some(id),
        Actor::Import(id) => id,
        _ => None,
    }
}

/// Marks a row deleted. Nothing leaves the database — `deleted_at` is the
/// state, and the materializer counts such a row as present so it never
/// resurrects what somebody deleted on purpose.
pub async fn soft_delete(pool: &SqlitePool, actor: Actor, id: i64) -> sqlx::Result<Option<i64>> {
    let mut tx = pool.begin().await?;

    let row = sqlx::query!(
        r#"SELECT period AS "period!: String", rule_id AS "rule_id: i64",
                  deleted_at AS "deleted_at: String"
           FROM transactions WHERE id = ?1"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else { return Ok(None) };
    // Already deleted: not an error, just nothing to do. Two people pressing
    // `dd` on the same row must not produce two entries in the log.
    if row.deleted_at.is_some() {
        return Ok(None);
    }

    let at = now();
    let by = actor_id(actor);
    sqlx::query!(
        "UPDATE transactions SET deleted_at = ?2, deleted_by = ?3, version = version + 1
         WHERE id = ?1",
        id,
        at,
        by
    )
    .execute(&mut *tx)
    .await?;

    // No change_field rows: a delete has no field diff, it is the row's
    // existence that changed.
    let mut entry = Change::transaction(actor, TxAction::Delete, id).from_rule(row.rule_id);
    if let Some(p) = Period::parse(&row.period) {
        entry = entry.in_period(p);
    }
    let change_id = entry.write(&mut tx).await?;

    tx.commit().await?;
    Ok(Some(change_id))
}

/// Clears the mark on the same row. `undo_of` points at the delete, so the log
/// reads "deleted", then "restored · undo of 18:39" — the delete is never
/// rewritten out of existence.
pub async fn restore(
    pool: &SqlitePool,
    actor: Actor,
    id: i64,
    undo_of: Option<i64>,
) -> sqlx::Result<Option<i64>> {
    let mut tx = pool.begin().await?;

    let row = sqlx::query!(
        r#"SELECT period AS "period!: String", rule_id AS "rule_id: i64",
                  deleted_at AS "deleted_at: String"
           FROM transactions WHERE id = ?1"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else { return Ok(None) };
    if row.deleted_at.is_none() {
        return Ok(None);
    }

    sqlx::query!(
        "UPDATE transactions SET deleted_at = NULL, deleted_by = NULL, version = version + 1
         WHERE id = ?1",
        id
    )
    .execute(&mut *tx)
    .await?;

    let mut entry = Change::transaction(actor, TxAction::Restore, id).from_rule(row.rule_id);
    if let Some(p) = Period::parse(&row.period) {
        entry = entry.in_period(p);
    }
    if let Some(of) = undo_of {
        entry = entry.undoing(of);
    }
    let change_id = entry.write(&mut tx).await?;

    tx.commit().await?;
    Ok(Some(change_id))
}

/// What `u` or `U` would reverse next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Undoable {
    pub change_id: i64,
    /// `transaction` or `file`. A file's soft delete is undoable too — the
    /// statusline says `u` puts it back, and it has to be true.
    pub entity: String,
    pub entity_id: i64,
    pub action: String,
    /// When the change was made, ISO UTC — a change from before today asks
    /// for a second `u` first.
    pub at: String,
    /// The row the change is about (for a file, the row it belongs to), and
    /// its title — what the statusline names.
    pub tx_id: i64,
    pub title: String,
}

/// Which way through the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// `u`: reverse my newest change still in effect.
    Undo,
    /// `U`: reverse my newest undo — which re-applies what it undid.
    Redo,
}

/// Where `u` and `U` reach: **my own** changes, to rows in one member's
/// ledger for one month — the pane and the month on screen. `None` widens
/// that axis, which only tests and the console want.
#[derive(Clone, Copy, Debug)]
pub struct Scope {
    pub me: i64,
    pub owner: Option<i64>,
    pub period: Option<Period>,
}

impl Scope {
    /// Everything this member did, anywhere — the old, unscoped `u`.
    pub fn anywhere(me: i64) -> Scope {
        Scope {
            me,
            owner: None,
            period: None,
        }
    }
}

/// A change of mine that somebody else has changed since — skipped rather
/// than reversed over their work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub who: String,
    pub title: String,
}

#[derive(Clone, Debug, Default)]
pub struct Next {
    pub pick: Option<Undoable>,
    /// Skipped on the way to `pick`, newest first — only those newer than my
    /// last undo or redo here, so each is reported once rather than on every
    /// press after it.
    pub skipped: Vec<Skipped>,
}

/// Notes on file entries that are part of ANOTHER entry: the files moved
/// because their row was deleted, and the live file a swap moved aside. They
/// follow the entry they belong to and are never a step of their own.
const SATELLITES: [&str; 2] = [
    crate::services::receipts::WITH_ROW,
    crate::services::receipts::SWAPPED,
];

/// What `u` (or `U`) would reverse next, in `scope`.
///
/// **Redo is an undo of an undo.** Every undo is its own entry, carrying
/// `undo_of` and the full diff, and reversing it re-applies what it undid
/// through the same code — so the depth along the `undo_of` chain says what
/// an entry is: 0 an original action, odd an undo, even (above 0) a redo.
/// `u` takes the newest entry at even depth, `U` the newest at odd depth —
/// and only one newer than my newest original here, because a fresh change
/// clears the redo stack, as it does in Vim.
///
/// **A change somebody else has changed since is skipped**, not reversed over
/// their work: undoing my 10:00 edit must not put my old amount back over
/// Ana's 11:00 one. Skipping rather than refusing is also what keeps `u`
/// from getting stuck on one change forever.
pub async fn next(pool: &SqlitePool, scope: Scope, dir: Direction) -> sqlx::Result<Next> {
    let period = scope.period.map(|p| p.to_string());
    let rows = sqlx::query!(
        r#"WITH RECURSIVE depth(id, d) AS (
               SELECT id, 0 FROM change WHERE member_id = ?1 AND undo_of IS NULL
               UNION ALL
               SELECT c.id, depth.d + 1 FROM change c JOIN depth ON c.undo_of = depth.id
           )
           SELECT c.id AS "id!: i64", c.entity AS "entity!: String",
                  c.entity_id AS "entity_id!: i64", c.action AS "action!: String",
                  c.at AS "at!: String", depth.d AS "depth!: i64",
                  t.id AS "tx_id!: i64", t.title AS "title!: String",
                  EXISTS (SELECT 1 FROM change u WHERE u.undo_of = c.id) AS "reversed!: bool"
           FROM change c
           JOIN depth ON depth.id = c.id
           LEFT JOIN receipts r ON c.entity = 'file' AND r.id = c.entity_id
           JOIN transactions t
             ON t.id = CASE WHEN c.entity = 'file' THEN r.transaction_id ELSE c.entity_id END
           WHERE c.member_id = ?1
             AND ((c.entity = 'transaction'
                   AND c.action IN ('create', 'delete', 'restore', 'edit', 'import',
                                    'privacy', 'reorder', 'reattribute'))
                  OR (c.entity = 'file' AND c.action IN ('file_upload', 'file_delete')))
             -- Part of another entry, and never a step of its own.
             AND NOT (c.entity = 'file' AND c.note IS NOT NULL AND c.note IN (?4, ?5))
             AND NOT EXISTS (SELECT 1 FROM change p
                             WHERE p.id = c.undo_of AND p.note IS NOT NULL AND p.note IN (?4, ?5))
             -- A bill retired from the trash because it is on another row now
             -- (`receipts::MOVED`): neither its delete nor its upload can be
             -- reversed any more, so they are not steps at all rather than
             -- ones `u` would refuse forever.
             AND NOT (c.entity = 'file'
                      AND EXISTS (SELECT 1 FROM change x
                                  WHERE x.entity = 'file' AND x.entity_id = c.entity_id
                                    AND x.action = 'file_unlink' AND x.note = ?6))
             -- The pane: rows in this member's ledger, or a row this pane
             -- handed to somebody else, which is still this pane's change.
             AND (?2 IS NULL OR t.member_id = ?2
                  OR (c.action = 'reattribute'
                      AND EXISTS (SELECT 1 FROM change_field f JOIN members m ON m.name = f.old_value
                                  WHERE f.change_id = c.id AND f.field = 'member' AND m.id = ?2)))
             -- The month on screen.
             AND (?3 IS NULL OR t.period = ?3)
           ORDER BY c.id DESC"#,
        scope.me,
        scope.owner,
        period,
        SATELLITES[0],
        SATELLITES[1],
        crate::services::receipts::MOVED,
    )
    .fetch_all(pool)
    .await?;

    let newest_fresh = rows
        .iter()
        .filter(|r| r.depth == 0)
        .map(|r| r.id)
        .max()
        .unwrap_or(0);
    let last_reversal = rows
        .iter()
        .filter(|r| r.depth > 0)
        .map(|r| r.id)
        .max()
        .unwrap_or(0);

    let mut out = Next::default();
    for r in rows {
        let wanted = match dir {
            Direction::Undo => r.depth % 2 == 0,
            Direction::Redo => r.depth % 2 == 1 && r.id > newest_fresh,
        };
        if r.reversed || !wanted {
            continue;
        }
        if let Some(who) = overtaken(pool, scope.me, r.id, &r.entity, r.entity_id, r.tx_id).await? {
            if r.id > last_reversal {
                out.skipped.push(Skipped {
                    who,
                    title: r.title.clone(),
                });
            }
            continue;
        }
        out.pick = Some(Undoable {
            change_id: r.id,
            entity: r.entity,
            entity_id: r.entity_id,
            action: r.action,
            at: r.at,
            tx_id: r.tx_id,
            title: r.title,
        });
        break;
    }
    Ok(out)
}

/// Who has changed this since `change_id`, if anybody else has — a later
/// change still in effect, by another member, to the same row (or, for a
/// file, the same slot, or the row it lives on). The materializer and the
/// system have no member and never count: they are not somebody's work.
async fn overtaken(
    pool: &SqlitePool,
    me: i64,
    change_id: i64,
    entity: &str,
    entity_id: i64,
    tx_id: i64,
) -> sqlx::Result<Option<String>> {
    if entity == "file" {
        sqlx::query_scalar!(
            r#"SELECT m.name AS "name!: String"
               FROM change o JOIN members m ON m.id = o.member_id
               LEFT JOIN receipts r ON o.entity = 'file' AND r.id = o.entity_id
               WHERE o.id > ?1 AND o.member_id <> ?2
                 AND NOT EXISTS (SELECT 1 FROM change u WHERE u.undo_of = o.id)
                 AND ((o.entity = 'file' AND o.action IN ('file_upload', 'file_delete')
                       AND r.transaction_id = ?4
                       AND r.kind = (SELECT kind FROM receipts WHERE id = ?3))
                      OR (o.entity = 'transaction' AND o.entity_id = ?4
                          AND o.action IN ('delete', 'restore', 'reattribute')))
               ORDER BY o.id DESC LIMIT 1"#,
            change_id,
            me,
            entity_id,
            tx_id,
        )
        .fetch_optional(pool)
        .await
    } else {
        sqlx::query_scalar!(
            r#"SELECT m.name AS "name!: String"
               FROM change o JOIN members m ON m.id = o.member_id
               WHERE o.id > ?1 AND o.member_id <> ?2
                 AND o.entity = 'transaction' AND o.entity_id = ?3
                 AND o.action IN ('create', 'delete', 'restore', 'edit', 'import',
                                  'privacy', 'reorder', 'reattribute')
                 AND NOT EXISTS (SELECT 1 FROM change u WHERE u.undo_of = o.id)
               ORDER BY o.id DESC LIMIT 1"#,
            change_id,
            me,
            entity_id,
        )
        .fetch_optional(pool)
        .await
    }
}

/// What `u` would undo for this member, anywhere — the unscoped form, kept
/// for the tests that walk one member's whole history.
pub async fn last_undoable(pool: &SqlitePool, member_id: i64) -> sqlx::Result<Option<Undoable>> {
    Ok(next(pool, Scope::anywhere(member_id), Direction::Undo)
        .await?
        .pick)
}

/// Applies the reverse of `undoable`, writing the reversal as its own entry.
/// Reverses a **transaction** change. A file change is undone by
/// `services::receipts`, which is the only thing that may touch the vault —
/// this module never moves a file.
pub async fn undo(pool: &SqlitePool, actor: Actor, u: &Undoable) -> sqlx::Result<bool> {
    if u.entity != "transaction" {
        return Ok(false);
    }
    let done = match u.action.as_str() {
        // Undoing an add is a soft delete like any other — the row stays, it is
        // simply marked, and the log says who reversed what.
        "create" | "restore" => {
            let mut tx = pool.begin().await?;
            let at = now();
            let by = actor_id(actor);
            let n = sqlx::query!(
                r#"UPDATE transactions SET deleted_at = ?2, deleted_by = ?3,
                                          version = version + 1
                   WHERE id = ?1 AND deleted_at IS NULL"#,
                u.entity_id,
                at,
                by
            )
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if n == 0 {
                return Ok(false);
            }
            Change::transaction(actor, TxAction::Delete, u.entity_id)
                .undoing(u.change_id)
                .write(&mut tx)
                .await?;
            tx.commit().await?;
            true
        }
        "delete" => restore(pool, actor, u.entity_id, Some(u.change_id))
            .await?
            .is_some(),
        // The rest all reverse by reading the change's own `old_value` back
        // out of `change_field` and applying it through the same public
        // mutation that wrote it. Re-deriving the reversal from the row's
        // current state instead would be a second implementation of every
        // verb, free to disagree with the first.
        "privacy" => toggle_privacy(pool, actor, u.entity_id, Some(u.change_id))
            .await?
            .is_some(),
        "reorder" => {
            let f = fields_of(pool, u.change_id).await?;
            let Some((old_pos, new_pos)) = f
                .iter()
                .find(|(name, _, _)| name == "position")
                .and_then(|(_, o, n)| Some((o.as_deref()?, n.as_deref()?)))
            else {
                return Ok(false);
            };
            let (Ok(old_pos), Ok(new_pos)) = (old_pos.parse::<i64>(), new_pos.parse::<i64>())
            else {
                return Ok(false);
            };
            reorder(
                pool,
                actor,
                u.entity_id,
                old_pos - new_pos,
                Some(u.change_id),
            )
            .await?
        }
        "reattribute" => {
            let f = fields_of(pool, u.change_id).await?;
            // The diff stores member *names* — that is what the log pane
            // reads — so undoing one means resolving a name back to an id.
            let Some(from_name) = f
                .iter()
                .find(|(name, _, _)| name == "member")
                .and_then(|(_, o, _)| o.clone())
            else {
                return Ok(false);
            };
            let to: Option<i64> = sqlx::query_scalar!(
                r#"SELECT id AS "id!: i64" FROM members WHERE name = ?1"#,
                from_name
            )
            .fetch_optional(pool)
            .await?;
            let Some(to) = to else { return Ok(false) };
            let done = reattribute(pool, actor, u.entity_id, to, Some(u.change_id)).await?;
            if done
                && let Some(Some(key)) = f
                    .iter()
                    .find(|(name, _, _)| name == "sort_key")
                    .map(|(_, o, _)| o.clone())
            {
                sqlx::query!(
                    "UPDATE transactions SET sort_key = ?2, version = version + 1 WHERE id = ?1",
                    u.entity_id,
                    key
                )
                .execute(pool)
                .await?;
            }
            done
        }
        // An amount read from a bill is an edit made by the bill: undoing it
        // puts back the recorded field, and nothing else.
        "edit" | "import" => undo_edit(pool, actor, u).await?,
        _ => false,
    };
    Ok(done)
}

/// The fields a change touched, as `(field, old_value, new_value)`.
async fn fields_of(
    pool: &SqlitePool,
    change_id: i64,
) -> sqlx::Result<Vec<(String, Option<String>, Option<String>)>> {
    let rows = sqlx::query!(
        r#"SELECT field AS "field!: String", old_value AS "old_value: String",
                  new_value AS "new_value: String"
           FROM change_field WHERE change_id = ?1"#,
        change_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.field, r.old_value, r.new_value))
        .collect())
}

/// Undoing an edit puts each recorded field back to its `old_value` and leaves
/// every other field of the row exactly as it is now — an edit that touched
/// only the amount must not also roll back a title somebody changed since.
async fn undo_edit(pool: &SqlitePool, actor: Actor, u: &Undoable) -> sqlx::Result<bool> {
    let cur = sqlx::query!(
        r#"SELECT title AS "title!: String", amount_cents AS "amount_cents: i64",
                  date AS "date: String", bucket AS "bucket: String",
                  visibility AS "visibility!: String", rule_id AS "rule_id: i64"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        u.entity_id
    )
    .fetch_optional(pool)
    .await?;
    let Some(cur) = cur else { return Ok(false) };

    let f = fields_of(pool, u.change_id).await?;
    let at = |name: &str| {
        f.iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, o, _)| o.clone())
    };

    let title = match at("title") {
        Some(Some(v)) => v,
        Some(None) => String::new(),
        None => cur.title.clone(),
    };
    let amount = match at("amount") {
        Some(v) => v.and_then(|s| s.parse::<i64>().ok()).map(Cents),
        None => cur.amount_cents.map(Cents),
    };
    let bucket = match at("bucket") {
        Some(v) => v,
        None => cur.bucket.clone(),
    };
    // The Day field only exists on a row with no rule behind it, and `edit`
    // ignores it for one that has — so reading a day out of the old date is
    // only ever meaningful here.
    let date = match at("date") {
        Some(v) => v,
        None => cur.date.clone(),
    };
    let day = date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .map(|d| d.day());
    let visibility = Visibility::parse(&cur.visibility).unwrap_or(Visibility::Household);
    // Only an edit that moved the tick carries a `paid` field; anything else
    // leaves the tick as it is now.
    let paid = at("paid").map(|v| v.as_deref() == Some("1"));
    // A due date a rule's new due day moved — put back as it was stamped.
    let due_date = at("due_date").map(|v| {
        v.as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    });

    edit(
        pool,
        actor,
        u.entity_id,
        EditFields {
            title: &title,
            amount,
            day: if cur.rule_id.is_none() { day } else { None },
            bucket: bucket.as_deref(),
            visibility,
            paid,
            due_date,
        },
        Some(u.change_id),
        // An undo writes regardless of the version. It is reversing a change
        // that is already in the log, not competing with one — and refusing
        // it because the materializer touched the row would make `u`
        // unreliable for the one thing it exists to do.
        None,
    )
    .await
    .map(|s| s == Saved::Ok)
}

/// One row, as the pane renders it — for the fragment a mutation returns.
pub async fn one(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<(i64, Period, bool)>> {
    let row = sqlx::query!(
        r#"SELECT member_id AS "member_id!: i64", period AS "period!: String",
                  deleted_at AS "deleted_at: String"
           FROM transactions WHERE id = ?1"#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(
        row.and_then(|r| {
            Period::parse(&r.period).map(|p| (r.member_id, p, r.deleted_at.is_some()))
        }),
    )
}

/// Moves a row `count` places down (`+`) or up (`-`) within its own pane.
///
/// **One write.** The row's new key is the midpoint of the two it lands
/// between; nothing else is touched. That is the whole reason the sort key is
/// fractional rather than an integer position — renumbering N rows per move is
/// write amplification, and two people doing it at once is a conflict SQLite
/// cannot help with.
///
/// Returns `false` when the row is already at the end it is being moved
/// toward, which is a no-op rather than an error: `J` on the last row should
/// do nothing quietly.
pub async fn reorder(
    pool: &SqlitePool,
    actor: Actor,
    id: i64,
    delta: i64,
    undo_of: Option<i64>,
) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;

    let row = sqlx::query!(
        r#"SELECT member_id AS "member_id!: i64", period AS "period!: String",
                  sort_key AS "sort_key!: String", rule_id AS "rule_id: i64"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else { return Ok(false) };

    // Manual order is what is being moved, so the neighbors are read in
    // manual order regardless of how the pane happens to be displayed. A
    // derived view refuses the gesture before it ever reaches here.
    let keys: Vec<String> = sqlx::query_scalar!(
        r#"SELECT sort_key AS "k!: String" FROM transactions
           WHERE member_id = ?1 AND period = ?2 AND deleted_at IS NULL
           ORDER BY sort_key"#,
        row.member_id,
        row.period,
    )
    .fetch_all(&mut *tx)
    .await?;

    let Some(from) = keys.iter().position(|k| *k == row.sort_key) else {
        return Ok(false);
    };
    // A count bigger than the distance available CLAMPS, the way `99j` lands
    // on the last line rather than refusing to move at all. Refusing was the
    // first behavior and it was silent: `99J` on the first row did nothing,
    // with an empty statusline, which reads as a broken key rather than as a
    // row already where it was asked to go.
    let to = (from as i64 + delta).clamp(0, keys.len() as i64 - 1);
    // Still a quiet no-op when there is genuinely nowhere to go — `K` on the
    // top row, `J` on the bottom one, or a count of zero. Clamping turns
    // "past the end" into "at the end", and at the end is where it already is.
    if delta == 0 || to == from as i64 {
        return Ok(false);
    }
    let to = to as usize;

    // Landing between the two rows that will end up on either side. Moving
    // DOWN means the target's neighbor is the one after it; moving up, the
    // one before.
    let (before, after) = if delta > 0 {
        (
            Some(keys[to].as_str()),
            keys.get(to + 1).map(|s| s.as_str()),
        )
    } else {
        (
            to.checked_sub(1)
                .and_then(|i| keys.get(i))
                .map(|s| s.as_str()),
            Some(keys[to].as_str()),
        )
    };
    let new_key = sortkey::between(before, after);

    sqlx::query!(
        "UPDATE transactions SET sort_key = ?2, version = version + 1 WHERE id = ?1",
        id,
        new_key
    )
    .execute(&mut *tx)
    .await?;

    let mut entry = Change::transaction(actor, TxAction::Reorder, id).from_rule(row.rule_id);
    if let Some(p) = Period::parse(&row.period) {
        entry = entry.in_period(p);
    }
    if let Some(of) = undo_of {
        entry = entry.undoing(of);
    }
    entry
        .diff(
            "position",
            Value::Text(Some((from + 1).to_string())),
            Value::Text(Some((to + 1).to_string())),
        )
        .write(&mut tx)
        .await?;

    tx.commit().await?;
    Ok(true)
}

/// What `e` can change about one occurrence.
///
/// **No `due_date`.** Once a row is recurring its due day lives on the rule
/// and nowhere else, so there are never two controls for one fact with no rule
/// for which wins at the next materialization. A non-recurring row has no rule
/// to hold one, which is exactly why `day` is here at all.
/// What a save did, which is three answers rather than a boolean.
///
/// `Stale` is the whole reason this type exists: "nothing was written" and
/// "somebody else got there first" need different copy, because only one of
/// them is worth reopening the row to look at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
    Ok,
    /// The row moved under the form. Nothing was written.
    Stale,
    /// No live row with that id.
    Gone,
}

pub struct EditFields<'a> {
    pub title: &'a str,
    pub amount: Option<Cents>,
    /// Day of the month, for a row with no rule behind it.
    pub day: Option<u32>,
    pub bucket: Option<&'a str>,
    pub visibility: Visibility,
    /// The manual paid tick, for an untracked recurring row only
    /// (`manual_paid_applies`). `None` leaves it as it is — what every caller
    /// but the edit form and undo passes.
    pub paid: Option<bool>,
    /// The stamped due date — `Some(None)` clears it, `None` leaves it as it
    /// is. Only undo passes it, reversing a rule's new due day reaching this
    /// month's row (`rules::edit_reporting`); the form never edits it on a
    /// recurring row.
    pub due_date: Option<Option<NaiveDate>>,
}

/// Saves an edit, recording only the fields that actually moved.
/// Saves an edit, recording only the fields that actually moved.
///
/// `expect_version` is the optimistic-concurrency check, and the **only**
/// place in this app that has one. Pass the version the form was rendered
/// with and the UPDATE matches on it, so a row somebody else has saved since
/// refuses rather than silently overwriting their work. Pass `None` — as undo
/// does — to write regardless: an undo is reversing a change that is already
/// in the log, not competing with one.
pub async fn edit(
    pool: &SqlitePool,
    actor: Actor,
    id: i64,
    fields: EditFields<'_>,
    undo_of: Option<i64>,
    expect_version: Option<i64>,
) -> sqlx::Result<Saved> {
    let mut tx = pool.begin().await?;

    let old = sqlx::query!(
        r#"SELECT title AS "title!: String", amount_cents AS "amount_cents: i64",
                  date AS "date: String", due_date AS "due_date: String",
                  bucket AS "bucket: String", visibility AS "visibility!: String",
                  period AS "period!: String", rule_id AS "rule_id: i64",
                  version AS "version!: i64", manual_paid AS "manual_paid!: i64"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?;

    // A deleted row and a missing one answer the same way on purpose: from
    // the form's side they are one fact, "this is not there any more", and
    // `u` is the next move for either.
    let Some(old) = old else {
        return Ok(Saved::Gone);
    };
    let Some(period) = Period::parse(&old.period) else {
        return Ok(Saved::Gone);
    };

    // The day is stamped onto the occurrence's own date. A recurring row never
    // reaches here with one, so this cannot fight the materializer.
    let new_date = match (old.rule_id, fields.day) {
        (None, Some(day)) => period
            .due_date(day)
            .map(|d| d.format("%Y-%m-%d").to_string()),
        _ => old.date.clone(),
    };

    let amount = fields.amount.map(|c| c.get());
    let visibility = fields.visibility.code();
    // A changed amount is marked as a person's, so a bill attached later
    // flags it rather than replacing it — see `services::import::reconcile`.
    // Clearing it makes it a placeholder again. An undo only puts back what
    // was there, so it leaves the mark alone.
    let is_undo = undo_of.is_some();
    let set_due = fields.due_date.is_some();
    let due_stamp = fields
        .due_date
        .flatten()
        .map(|d| d.format("%Y-%m-%d").to_string());
    let old_due = old
        .due_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());

    // The check itself. `version` is compared in the WHERE rather than
    // against the row read above, so the test and the write are one
    // statement — reading, comparing in Rust and then writing would leave a
    // window between them, which is the bug this is here to prevent.
    let expected = expect_version.unwrap_or(old.version);
    let wrote = sqlx::query!(
        r#"UPDATE transactions SET title = ?2, amount_cents = ?3, date = ?4,
                                   bucket = ?5, visibility = ?6,
                                   amount_source = CASE
                                       WHEN ?3 IS amount_cents OR ?8 THEN amount_source
                                       WHEN ?3 IS NULL THEN NULL
                                       ELSE 'user' END,
                                   manual_paid = coalesce(?9, manual_paid),
                                   due_date = CASE WHEN ?10 THEN ?11 ELSE due_date END,
                                   version = version + 1
           WHERE id = ?1 AND version = ?7"#,
        id,
        fields.title,
        amount,
        new_date,
        fields.bucket,
        visibility,
        expected,
        is_undo,
        fields.paid,
        set_due,
        due_stamp,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if wrote == 0 {
        // Nothing written, and nothing logged: the change entry and the row
        // it describes commit together or not at all, which is the rule the
        // whole change log rests on.
        return Ok(Saved::Stale);
    }

    let old_date = old
        .date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    let new_date_parsed = new_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());

    let mut entry = Change::transaction(actor, TxAction::Edit, id)
        .from_rule(old.rule_id)
        .in_period(period);
    if let Some(of) = undo_of {
        entry = entry.undoing(of);
    }
    let entry = entry
        .diff("title", Value::text(&old.title), Value::text(fields.title))
        .diff(
            "amount",
            Value::Money(old.amount_cents.map(Cents)),
            Value::Money(fields.amount),
        )
        .diff("date", Value::Date(old_date), Value::Date(new_date_parsed))
        .diff(
            "bucket",
            Value::Text(old.bucket.clone()),
            Value::Text(fields.bucket.map(str::to_string)),
        )
        .diff(
            "paid",
            Value::Bool(old.manual_paid != 0),
            Value::Bool(fields.paid.unwrap_or(old.manual_paid != 0)),
        )
        .diff(
            "due_date",
            Value::Date(old_due),
            Value::Date(fields.due_date.unwrap_or(old_due)),
        );

    // Privacy is its own verb, not a field of an edit: handing a hidden row to
    // the household is a different thing from changing its amount, and the log
    // should read that way.
    let privacy_changed = old.visibility != visibility;

    if !entry.is_empty() {
        entry.write(&mut tx).await?;
    }
    if privacy_changed {
        let mut p = Change::transaction(actor, TxAction::Privacy, id)
            .from_rule(old.rule_id)
            .in_period(period);
        if let Some(of) = undo_of {
            p = p.undoing(of);
        }
        p.diff(
            "visibility",
            Value::text(&old.visibility),
            Value::text(visibility),
        )
        .write(&mut tx)
        .await?;
    }

    tx.commit().await?;
    Ok(Saved::Ok)
}

/// A row whose bill was removed gives it up to another row.
///
/// **A row holds a bill only while it holds the file.** Its barcode stays on
/// it after the bill goes to the trash, and used to refuse every other row the
/// same bill. `web::import::attach_to` calls this when the bill is attached
/// elsewhere, inside the attach's own transaction:
///
/// - the barcode is cleared, so the new row can take `idx_tx_barcode`, and so
///   are the sources the schema ties to one (the values themselves stay);
/// - the newest trashed copy of the bill is **retired** — a `file_unlink`
///   noted `receipts::MOVED`, which `U`, `u` and every restore then leave
///   alone. A bill is on one row only, so that copy can never come back. The
///   file stays in `.delete/`: nothing is destroyed.
pub async fn release_bill(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: Actor,
    holder: i64,
    barcode: &str,
) -> sqlx::Result<()> {
    let row = sqlx::query!(
        r#"SELECT period AS "period!: String", rule_id AS "rule_id: i64"
           FROM transactions WHERE id = ?1"#,
        holder
    )
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query!(
        r#"UPDATE transactions SET
               barcode_raw = NULL, barcode_kind = NULL, issuer_id = NULL,
               amount_source = CASE WHEN amount_source = 'barcode'
                                    THEN NULL ELSE amount_source END,
               due_date_source = CASE WHEN due_date_source IN ('fator', 'free_field')
                                      THEN NULL ELSE due_date_source END,
               version = version + 1
           WHERE id = ?1"#,
        holder
    )
    .execute(&mut **tx)
    .await?;

    let copy: Option<i64> = sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM receipts
           WHERE transaction_id = ?1 AND kind = 'barcode' AND deleted_at IS NOT NULL
           ORDER BY deleted_at DESC, id DESC LIMIT 1"#,
        holder
    )
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(copy) = copy {
        let mut entry = Change::file(actor, FileAction::Unlink, copy)
            .from_rule(row.rule_id)
            .note(crate::services::receipts::MOVED)
            .diff("barcode", Value::text(barcode), Value::Text(None));
        if let Some(p) = Period::parse(&row.period) {
            entry = entry.in_period(p);
        }
        entry.write(tx).await?;
    }
    Ok(())
}

/// Whether the manual paid tick applies to this row: live, generated by a
/// rule, and that rule tracks nothing on disk. A tracked row is proven paid by
/// its receipt and never by a tick; a one-off records a spend when it is made.
/// A deleted rule still counts — an orphaned untracked bill can still be paid.
pub async fn manual_paid_applies(pool: &SqlitePool, id: i64) -> sqlx::Result<Applies> {
    let row = sqlx::query!(
        r#"SELECT t.rule_id AS "rule_id: i64", r.tracked AS "tracked: i64"
           FROM transactions t LEFT JOIN recurring_rules r ON r.id = t.rule_id
           WHERE t.id = ?1 AND t.deleted_at IS NULL"#,
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        None => Applies::Gone,
        Some(r) if r.rule_id.is_none() => Applies::OneOff,
        Some(r) if r.tracked.unwrap_or(0) != 0 => Applies::Tracked,
        Some(_) => Applies::Yes,
    })
}

/// The answer to `manual_paid_applies`, with the reason when it does not.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Applies {
    Yes,
    Tracked,
    OneOff,
    Gone,
}

/// `m` — toggle the manual paid tick on an untracked recurring row. The caller
/// has already asked `manual_paid_applies`.
///
/// Logged as an `edit` of the one `paid` field rather than a verb of its own:
/// the schema's action list has no `paid`, and an edit with one field reads
/// inline in the log — "edited · paid by hand · no → yes" — and is undone by
/// `undo_edit` like any other. Returns the new state.
pub async fn toggle_paid(
    pool: &SqlitePool,
    actor: Actor,
    id: i64,
    undo_of: Option<i64>,
) -> sqlx::Result<Option<bool>> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query!(
        r#"SELECT manual_paid AS "manual_paid!: i64", period AS "period!: String",
                  rule_id AS "rule_id: i64"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let was = row.manual_paid != 0;
    let now = !was;

    sqlx::query!(
        "UPDATE transactions SET manual_paid = ?2, version = version + 1 WHERE id = ?1",
        id,
        now
    )
    .execute(&mut *tx)
    .await?;

    let mut entry = Change::transaction(actor, TxAction::Edit, id).from_rule(row.rule_id);
    if let Some(p) = Period::parse(&row.period) {
        entry = entry.in_period(p);
    }
    if let Some(of) = undo_of {
        entry = entry.undoing(of);
    }
    entry
        .diff("paid", Value::Bool(was), Value::Bool(now))
        .write(&mut tx)
        .await?;

    tx.commit().await?;
    Ok(Some(now))
}

/// `p` — toggle whether this row is hidden from the other members.
///
/// Its own verb in the log, for the same reason it is its own key: who can see
/// a row is a different kind of fact from what it says.
pub async fn toggle_privacy(
    pool: &SqlitePool,
    actor: Actor,
    id: i64,
    undo_of: Option<i64>,
) -> sqlx::Result<Option<bool>> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query!(
        r#"SELECT visibility AS "visibility!: String", period AS "period!: String",
                  rule_id AS "rule_id: i64"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else { return Ok(None) };
    let now_private = row.visibility == "household";
    let next = if now_private { "private" } else { "household" };

    sqlx::query!(
        "UPDATE transactions SET visibility = ?2, version = version + 1 WHERE id = ?1",
        id,
        next
    )
    .execute(&mut *tx)
    .await?;

    let mut entry = Change::transaction(actor, TxAction::Privacy, id).from_rule(row.rule_id);
    if let Some(p) = Period::parse(&row.period) {
        entry = entry.in_period(p);
    }
    if let Some(of) = undo_of {
        entry = entry.undoing(of);
    }
    entry
        .diff(
            "visibility",
            Value::text(&row.visibility),
            Value::text(next),
        )
        .write(&mut tx)
        .await?;

    tx.commit().await?;
    Ok(Some(now_private))
}

/// `Ctrl-H` / `Ctrl-L` — move the row to another member's pane.
///
/// **A fresh sort key in the destination**, appended: the key is global, so
/// carrying the old one over would drop the row into an arbitrary position in
/// somebody else's ordering. Appending is the one placement that means
/// anything when you did not choose it.
pub async fn reattribute(
    pool: &SqlitePool,
    actor: Actor,
    id: i64,
    to_member: i64,
    undo_of: Option<i64>,
) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;

    let row = sqlx::query!(
        r#"SELECT member_id AS "member_id!: i64", period AS "period!: String",
                  rule_id AS "rule_id: i64", sort_key AS "sort_key!: String"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else { return Ok(false) };
    if row.member_id == to_member {
        return Ok(false);
    }

    let last: Option<String> = sqlx::query_scalar!(
        r#"SELECT max(sort_key) AS "k: String" FROM transactions
           WHERE member_id = ?1 AND period = ?2 AND deleted_at IS NULL"#,
        to_member,
        row.period,
    )
    .fetch_one(&mut *tx)
    .await?;

    let sort_key = match last.as_deref() {
        Some(k) => sortkey::after(k),
        None => sortkey::between(None, None),
    };

    let (from_name, to_name) = (
        member_name(&mut tx, row.member_id).await?,
        member_name(&mut tx, to_member).await?,
    );

    sqlx::query!(
        "UPDATE transactions SET member_id = ?2, sort_key = ?3, version = version + 1
         WHERE id = ?1",
        id,
        to_member,
        sort_key,
    )
    .execute(&mut *tx)
    .await?;

    let mut entry = Change::transaction(actor, TxAction::Reattribute, id).from_rule(row.rule_id);
    if let Some(p) = Period::parse(&row.period) {
        entry = entry.in_period(p);
    }
    if let Some(of) = undo_of {
        entry = entry.undoing(of);
    }
    entry
        .diff("member", Value::text(&from_name), Value::text(&to_name))
        // Appending in the destination is the right placement for a move you
        // chose; it is the wrong one for undoing it, and the old key is gone
        // the moment this UPDATE lands. So it is recorded here — the only
        // way `u` can put the row back exactly where it was.
        .always(
            "sort_key",
            Value::text(&row.sort_key),
            Value::text(&sort_key),
        )
        .write(&mut tx)
        .await?;

    tx.commit().await?;
    Ok(true)
}

async fn member_name(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: i64,
) -> sqlx::Result<String> {
    sqlx::query_scalar!(
        r#"SELECT name AS "n!: String" FROM members WHERE id = ?1"#,
        id
    )
    .fetch_optional(&mut **tx)
    .await
    .map(|n| n.unwrap_or_default())
}
