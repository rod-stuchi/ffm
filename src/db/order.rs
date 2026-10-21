//! `:order` — one pane's manual order, copied to other months — and the
//! place a new month's occurrence inherits from the month before.
//!
//! The matching and the key arithmetic are `domain::order`; this module reads
//! the panes, writes the keys and logs it.
//!
//! **One copy is one step for `u`**, however many months it touched. It
//! writes an anchor — a member entry for the pane copied from — and every row
//! it moved is a `reorder` entry in that row's own month, `part_of` the
//! anchor (migration 0019 says why it is shaped this way). `u` in the pane
//! and month copied from picks the anchor and [`reverse`] puts every part
//! back; redo is [`reverse`] again, on the undo's anchor.

use crate::db::changes::{Actor, Change, MemberAction, TxAction, Value};
use crate::domain::order::{self, Line};
use crate::domain::period::Period;
use crate::domain::sortkey;
use sqlx::{Sqlite, SqliteConnection, SqlitePool, Transaction};

/// The note on a copy's anchor. It is what tells the anchor apart from the
/// other member `edit` (turning a member off), and what `u` looks for — so
/// like `receipts::MOVED` it is a fixed English line, never translated.
pub const COPIED: &str = "order copied to other months";

/// The note on each row a copy moved — `order copied from 10/2026` — and on
/// the undo of one, which ends in `UNDONE`. Stored in English like every note;
/// `ui::log::note_text` words them for whoever reads them.
pub const COPIED_FROM: &str = "order copied from ";
pub const UNDONE: &str = " — undone";

/// A pane's live rows — every visibility, since only someone who may write
/// the pane copies its order, and they see all of it — in the order it shows
/// them by hand.
pub async fn lines(
    conn: &mut SqliteConnection,
    member: i64,
    period: Period,
) -> sqlx::Result<Vec<Line>> {
    let period_s = period.to_string();
    let rows = sqlx::query!(
        r#"SELECT id AS "id!: i64", rule_id AS "rule_id: i64",
                  title AS "title!: String", sort_key AS "key!: String"
           FROM transactions
           WHERE member_id = ?1 AND period = ?2 AND deleted_at IS NULL
           ORDER BY sort_key, id"#,
        member,
        period_s
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Line {
            id: r.id,
            rule_id: r.rule_id,
            title: r.title,
            key: r.key,
        })
        .collect())
}

/// The months of `year` in which `member` has live rows — what a month with
/// none yet will inherit its order from (the latest of these before it).
pub async fn months_with_rows(
    conn: &mut SqliteConnection,
    member: i64,
    year: i32,
) -> sqlx::Result<Vec<Period>> {
    let first = format!("{year:04}-01");
    let last = format!("{year:04}-12");
    let rows = sqlx::query_scalar!(
        r#"SELECT DISTINCT period AS "period!: String" FROM transactions
           WHERE member_id = ?1 AND period BETWEEN ?2 AND ?3 AND deleted_at IS NULL
           ORDER BY period"#,
        member,
        first,
        last
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.iter().filter_map(|p| Period::parse(p)).collect())
}

/// One target month, as a copy would find it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonthPlan {
    pub period: Period,
    /// How many rows the month has — 0 means nothing there yet.
    pub rows: usize,
    pub moves: Vec<order::Move>,
}

/// What copying `source`'s order onto each of `targets` would do — and how
/// many rows the source has, since an empty month has no order to copy. Read
/// only: the drawer's preview, and the first half of [`copy`]. The source
/// itself is skipped if it was named.
pub async fn plan(
    conn: &mut SqliteConnection,
    member: i64,
    source: Period,
    targets: &[Period],
) -> sqlx::Result<(usize, Vec<MonthPlan>)> {
    let from = lines(conn, member, source).await?;
    let mut out = Vec::with_capacity(targets.len());
    for &p in targets.iter().filter(|p| **p != source) {
        let to = lines(conn, member, p).await?;
        out.push(MonthPlan {
            period: p,
            rows: to.len(),
            moves: order::moves(&from, &to),
        });
    }
    Ok((from.len(), out))
}

/// What a copy did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Copied {
    /// The anchor `u` will pick — `None` when nothing needed to move, which
    /// writes nothing at all rather than an empty step.
    pub anchor: Option<i64>,
    /// The months it reordered, and how many rows moved in each.
    pub months: Vec<(Period, usize)>,
}

impl Copied {
    pub fn rows(&self) -> usize {
        self.months.iter().map(|(_, n)| n).sum()
    }
}

/// Puts `member`'s rows in each of `targets` in `source`'s manual order. One
/// SQLite transaction: every month or none.
pub async fn copy(
    pool: &SqlitePool,
    actor: Actor,
    member: i64,
    source: Period,
    targets: &[Period],
) -> sqlx::Result<Copied> {
    let mut tx = pool.begin().await?;
    let (_, plans) = plan(&mut tx, member, source, targets).await?;
    let plans: Vec<MonthPlan> = plans.into_iter().filter(|p| !p.moves.is_empty()).collect();
    if plans.is_empty() {
        return Ok(Copied::default());
    }

    let months: Vec<(Period, usize)> = plans.iter().map(|p| (p.period, p.moves.len())).collect();
    let anchor = Change::member(actor, MemberAction::OrderCopy, member)
        .in_period(source)
        .note(COPIED)
        .diff("months", Value::Text(None), Value::text(storage(&months)))
        .write(&mut tx)
        .await?;

    let note = format!("{COPIED_FROM}{}", crate::ui::fmt::period(source));
    for p in &plans {
        for m in &p.moves {
            set_key(&mut tx, m.id, &m.old_key, &m.new_key).await?;
            // Stamped with its rule like any other reorder, so it shows in
            // the rule's history too.
            Change::transaction(actor, TxAction::Reorder, m.id)
                .from_rule(m.rule_id)
                .in_period(p.period)
                .part_of(anchor)
                .note(note.clone())
                .diff(
                    "position",
                    Value::Text(Some(m.old_pos.to_string())),
                    Value::Text(Some(m.new_pos.to_string())),
                )
                .always(
                    "sort_key",
                    Value::text(m.old_key.clone()),
                    Value::text(m.new_key.clone()),
                )
                .write(&mut tx)
                .await?;
        }
    }

    tx.commit().await?;
    Ok(Copied {
        anchor: Some(anchor),
        months,
    })
}

/// `2026-01:3,2026-02:5` — the months a copy reordered and how many rows in
/// each, as the anchor's `months` field stores them: periods in storage form,
/// never the display one.
fn storage(months: &[(Period, usize)]) -> String {
    months
        .iter()
        .map(|(p, n)| format!("{p}:{n}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The months an anchor's `months` field names, read back.
pub fn months_of(stored: &str) -> Vec<(Period, usize)> {
    stored
        .split(',')
        .filter_map(|part| {
            let (p, n) = part.split_once(':')?;
            Some((Period::parse(p)?, n.parse().ok()?))
        })
        .collect()
}

pub(crate) async fn set_key(
    conn: &mut SqliteConnection,
    id: i64,
    old: &str,
    new: &str,
) -> sqlx::Result<()> {
    sqlx::query!(
        r#"UPDATE transactions SET sort_key = ?3, version = version + 1
           WHERE id = ?1 AND sort_key = ?2"#,
        id,
        old,
        new
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// undo and redo
// ---------------------------------------------------------------------------

/// What reversing a copy (or reversing that) would do, or did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reversal {
    /// The pane: its member and the month the copy was made from.
    pub owner: i64,
    pub period: Option<Period>,
    /// Per month, the rows put back.
    pub months: Vec<(Period, usize)>,
    /// Rows moved by somebody (or something) since — left where they are
    /// now rather than put back over that.
    pub left: usize,
}

impl Reversal {
    pub fn rows(&self) -> usize {
        self.months.iter().map(|(_, n)| n).sum()
    }
}

/// One part to put back: the row, its month, and the key to restore.
pub(crate) struct Back {
    pub part: i64,
    pub id: i64,
    pub rule_id: Option<i64>,
    pub period: Period,
    pub to: String,
}

/// Puts each of `back` — rows of `owner`'s `period` — on the key it had, and
/// logs each as a `reorder` that is `part_of(next)` and an undo of its part.
/// The reversal of a `:order` copy and of a selection's `J`/`K` alike.
///
/// With no `next`, the first row written is the anchor — not a part, an undo
/// of its own part — and the rest are `part_of` it: how a selection's move,
/// whose anchor is one of its rows, is reversed. Returns the anchor.
///
/// The keys are checked against the pane as it is NOW: a row added since can
/// have been given exactly the key one of these gave up — a midpoint is
/// deterministic — and equal keys leave nothing between them, so that one
/// lands just below the taken key instead. A key still held by another row
/// of this batch is not taken: that row is about to give it up.
pub(crate) async fn put_back(
    tx: &mut Transaction<'_, Sqlite>,
    actor: Actor,
    owner: i64,
    period: Period,
    back: &[&Back],
    mut next: Option<i64>,
    note: Option<&str>,
) -> sqlx::Result<Option<i64>> {
    let mut pane = lines(tx, owner, period).await?;
    let before: Vec<i64> = pane.iter().map(|l| l.id).collect();
    let mut writes: Vec<(&Back, String, String)> = Vec::new();
    // Rows of this batch not put back yet: the key one of them holds now is
    // about to be given up, so it is not taken — two rows of one step that
    // trade keys must each get theirs back exactly.
    let mut pending: Vec<i64> = back.iter().map(|b| b.id).collect();
    for &b in back {
        pending.retain(|id| *id != b.id);
        let Some(at) = pane.iter().position(|l| l.id == b.id) else {
            continue;
        };
        let from = pane[at].key.clone();
        let taken = pane
            .iter()
            .any(|l| l.id != b.id && l.key == b.to && !pending.contains(&l.id));
        let to = if taken {
            let below = pane
                .iter()
                .filter(|l| l.id != b.id && l.key < b.to)
                .map(|l| l.key.as_str())
                .max();
            sortkey::between(below, Some(&b.to))
        } else {
            b.to.clone()
        };
        pane[at].key = to.clone();
        pane.sort_by(|a, b| a.key.cmp(&b.key).then(a.id.cmp(&b.id)));
        writes.push((b, from, to));
    }
    let after: Vec<i64> = pane.iter().map(|l| l.id).collect();
    for (b, from, to) in writes {
        set_key(tx, b.id, &from, &to).await?;
        let pos = |ids: &[i64]| ids.iter().position(|i| *i == b.id).map(|i| i + 1);
        let mut entry = Change::transaction(actor, TxAction::Reorder, b.id)
            .from_rule(b.rule_id)
            .in_period(period)
            .undoing(b.part);
        if let Some(next) = next {
            entry = entry.part_of(next);
        }
        if let Some(note) = note {
            entry = entry.note(note);
        }
        if let (Some(o), Some(n)) = (pos(&before), pos(&after)) {
            entry = entry.diff(
                "position",
                Value::Text(Some(o.to_string())),
                Value::Text(Some(n.to_string())),
            );
        }
        let id = entry
            .always("sort_key", Value::text(from), Value::text(to))
            .write(tx)
            .await?;
        next.get_or_insert(id);
    }
    Ok(next)
}

/// The anchor's pane and fields, and the parts still in effect — read once,
/// for both the question and the answer.
async fn reversal_plan(
    conn: &mut SqliteConnection,
    anchor: i64,
) -> sqlx::Result<(
    Reversal,
    Vec<Back>,
    Option<(Option<String>, Option<String>)>,
)> {
    let head = sqlx::query!(
        r#"SELECT entity_id AS "owner!: i64", period AS "period: String"
           FROM change WHERE id = ?1 AND entity = 'member' AND note = ?2"#,
        anchor,
        COPIED
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(head) = head else {
        return Ok((Reversal::default(), Vec::new(), None));
    };
    let months_field = sqlx::query!(
        r#"SELECT old_value AS "old: String", new_value AS "new: String"
           FROM change_field WHERE change_id = ?1 AND field = 'months'"#,
        anchor
    )
    .fetch_optional(&mut *conn)
    .await?
    .map(|f| (f.old, f.new));

    // Each part's own sort_key field says what it set (new) and what was
    // there before (old). A part already reversed some other way has an
    // entry pointing back at it, and is not this step's to redo.
    let parts = sqlx::query!(
        r#"SELECT c.id AS "part!: i64", c.entity_id AS "id!: i64",
                  c.period AS "period: String",
                  f.old_value AS "was: String", f.new_value AS "set: String",
                  t.sort_key AS "now?: String", t.member_id AS "member?: i64",
                  t.period AS "row_period?: String", t.rule_id AS "rule_id?: i64",
                  t.deleted_at AS "deleted_at?: String"
           FROM change c
           JOIN change_field f ON f.change_id = c.id AND f.field = 'sort_key'
           LEFT JOIN transactions t ON t.id = c.entity_id
           WHERE c.part_of = ?1
             AND NOT EXISTS (SELECT 1 FROM change u WHERE u.undo_of = c.id)
           ORDER BY c.id"#,
        anchor
    )
    .fetch_all(&mut *conn)
    .await?;

    let mut rev = Reversal {
        owner: head.owner,
        period: head.period.as_deref().and_then(Period::parse),
        ..Reversal::default()
    };
    let mut back = Vec::new();
    for p in parts {
        let period = p.period.as_deref().and_then(Period::parse);
        let still = p.deleted_at.is_none()
            && p.member == Some(head.owner)
            && p.row_period.is_some()
            && p.row_period == p.period
            && p.now.is_some()
            && p.now == p.set;
        match (still, period, p.was) {
            (true, Some(period), Some(to)) => {
                match rev.months.iter_mut().find(|(m, _)| *m == period) {
                    Some((_, n)) => *n += 1,
                    None => rev.months.push((period, 1)),
                }
                back.push(Back {
                    part: p.part,
                    id: p.id,
                    rule_id: p.rule_id,
                    period,
                    to,
                });
            }
            _ => rev.left += 1,
        }
    }
    rev.months.sort();
    Ok((rev, back, months_field))
}

/// What `u` (or `U`) on this anchor would put back — for the question it
/// asks before doing it.
pub async fn preview_reverse(pool: &SqlitePool, anchor: i64) -> sqlx::Result<Reversal> {
    let mut conn = pool.acquire().await?;
    Ok(reversal_plan(&mut conn, anchor).await?.0)
}

/// Reverses a copy — or the reversal of one, which is redo. Writes a new
/// anchor pointing back at `anchor`, then puts each part's row back on the
/// key it had, logged as a `reorder` that is `part_of` the new anchor and an
/// undo of the part. A row moved since keeps where it is now: reversing it
/// would undo somebody's later move, not this copy.
///
/// The anchor is written even when every row was left, so `u` moves past
/// this step rather than sticking on it.
pub async fn reverse(
    pool: &SqlitePool,
    actor: Actor,
    anchor: i64,
) -> sqlx::Result<Option<Reversal>> {
    let mut tx = pool.begin().await?;
    let (rev, back, months_field) = reversal_plan(&mut tx, anchor).await?;
    let Some((old, new)) = months_field else {
        return Ok(None);
    };
    let Some(source) = rev.period else {
        return Ok(None);
    };

    let next = Change::member(actor, MemberAction::OrderCopy, rev.owner)
        .in_period(source)
        .note(COPIED)
        .undoing(anchor)
        .always("months", Value::Text(new), Value::Text(old))
        .write(&mut tx)
        .await?;

    // Month by month, so a key put back can be checked against the rows
    // around it as they are NOW, and each row's position read in that pane.
    let mut by_month: Vec<Period> = back.iter().map(|b| b.period).collect();
    by_month.sort();
    by_month.dedup();
    let note = format!("{COPIED_FROM}{}{UNDONE}", crate::ui::fmt::period(source));
    for period in by_month {
        let these: Vec<&Back> = back.iter().filter(|b| b.period == period).collect();
        put_back(
            &mut tx,
            actor,
            rev.owner,
            period,
            &these,
            Some(next),
            Some(&note),
        )
        .await?;
    }

    tx.commit().await?;
    Ok(Some(rev))
}

// ---------------------------------------------------------------------------
// the materializer
// ---------------------------------------------------------------------------

/// The key a new occurrence of `rule_id` should get in `member`'s `period`:
/// where its rule sat in the latest earlier month it has a live row in,
/// relative to the rows already here. `None` when there is no earlier row or
/// nothing around it to go by — the caller's own rule decides then (income
/// on top, spend last).
///
/// The same member's pane only: a row handed to somebody else is not in this
/// pane, and its neighbors there say nothing about this one.
pub async fn inherited_key(
    conn: &mut SqliteConnection,
    member: i64,
    rule_id: i64,
    period: Period,
) -> sqlx::Result<Option<String>> {
    let period_s = period.to_string();
    let previous = sqlx::query_scalar!(
        r#"SELECT period AS "period!: String" FROM transactions
           WHERE rule_id = ?1 AND member_id = ?2 AND period < ?3 AND deleted_at IS NULL
           ORDER BY period DESC LIMIT 1"#,
        rule_id,
        member,
        period_s
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(previous) = previous.as_deref().and_then(Period::parse) else {
        return Ok(None);
    };
    let source = lines(conn, member, previous).await?;
    let Some(at) = source.iter().position(|l| l.rule_id == Some(rule_id)) else {
        return Ok(None);
    };
    let target = lines(conn, member, period).await?;
    Ok(order::placement(&source, at, &target))
}
