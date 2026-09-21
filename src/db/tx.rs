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

use crate::db::changes::{Actor, Change, TxAction, Value};
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::sortkey;
use crate::domain::status::{TxKind, Visibility};
use chrono::{NaiveDate, Utc};
use sqlx::SqlitePool;

fn now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
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

    // The neighbour this row is going next to. Only that one row's key is read,
    // and none is rewritten — the whole point of a fractional index.
    let neighbour: Option<String> = if new.kind.is_income() {
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

    let sort_key = match (new.kind.is_income(), neighbour.as_deref()) {
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
        "UPDATE transactions SET deleted_at = ?2, deleted_by = ?3 WHERE id = ?1",
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
        "UPDATE transactions SET deleted_at = NULL, deleted_by = NULL WHERE id = ?1",
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

/// What `u` would undo: this member's most recent action that has not already
/// been undone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Undoable {
    pub change_id: i64,
    pub entity_id: i64,
    pub action: String,
}

/// Scoped to the asking member on purpose. `u` means "undo what *I* just did" —
/// reaching into somebody else's last action would be a different, much more
/// surprising gesture, and this is a shared ledger.
pub async fn last_undoable(pool: &SqlitePool, member_id: i64) -> sqlx::Result<Option<Undoable>> {
    let row = sqlx::query!(
        r#"SELECT c.id AS "id!: i64", c.entity_id AS "entity_id!: i64",
                  c.action AS "action!: String"
           FROM change c
           WHERE c.member_id = ?1
             AND c.entity = 'transaction'
             AND c.action IN ('create', 'delete', 'restore')
             AND c.undo_of IS NULL
             -- Already undone: its reversal points back at it. Without this,
             -- `u` twice in a row would toggle the same change forever instead
             -- of walking back through history.
             AND NOT EXISTS (SELECT 1 FROM change u WHERE u.undo_of = c.id)
           ORDER BY c.id DESC
           LIMIT 1"#,
        member_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| Undoable {
        change_id: r.id,
        entity_id: r.entity_id,
        action: r.action,
    }))
}

/// Applies the reverse of `undoable`, writing the reversal as its own entry.
pub async fn undo(pool: &SqlitePool, actor: Actor, u: &Undoable) -> sqlx::Result<bool> {
    let done = match u.action.as_str() {
        // Undoing an add is a soft delete like any other — the row stays, it is
        // simply marked, and the log says who reversed what.
        "create" | "restore" => {
            let mut tx = pool.begin().await?;
            let at = now();
            let by = actor_id(actor);
            let n = sqlx::query!(
                r#"UPDATE transactions SET deleted_at = ?2, deleted_by = ?3
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
        _ => false,
    };
    Ok(done)
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
