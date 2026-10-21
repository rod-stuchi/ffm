//! `:charts` — the rows a range of months is drawn from.
//!
//! **One query for the whole range, not a pane read per month.** A pane read
//! asks the vault about every tracked row (its files, its progress), which is
//! what a ledger screen needs and a chart does not: the Years tab reads every
//! month the household has, and a pane loop there would be hundreds of
//! `stat`s and progress queries to draw a dozen bars.
//!
//! **What it must still agree with is the pane header**, row for row: the
//! same privacy predicate the pane applies (another member's private row only
//! for its owner and an admin, and not under preview), the same months a
//! turned-off member is hidden from (`members_in`), and the same `~`
//! projection for a varying income that has not landed yet — one function,
//! `ledger::projection`, so the two cannot come apart. A test holds a month's
//! sums against `pane_with_vault`'s.

use crate::db::ledger;
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::status::TxKind;
use crate::domain::viewer::Viewer;
use sqlx::SqlitePool;

/// One live row, as much of it as a chart reads.
#[derive(Clone, Debug)]
pub struct ChartRow {
    pub id: i64,
    pub period: Period,
    pub member_id: i64,
    pub kind: TxKind,
    /// What the row is worth: the stored amount, or the projection while a
    /// varying income is unconfirmed (`LedgerRow::effective_cents`). `None`
    /// is a row still waiting for its amount — counted nowhere.
    pub cents: Option<Cents>,
    pub estimated: bool,
    pub title: String,
    pub bucket: Option<String>,
    pub rule_id: Option<i64>,
}

impl ChartRow {
    pub fn is_income(&self) -> bool {
        self.kind.is_income()
    }
}

/// Every live row in `from..=to` this viewer may see, oldest month first.
///
/// `root` is the vault, which only a varying income's confirmation reads —
/// the same answer its mark gives.
pub async fn rows(
    pool: &SqlitePool,
    root: Option<&std::path::Path>,
    viewer: Viewer,
    from: Period,
    to: Period,
) -> sqlx::Result<Vec<ChartRow>> {
    let (from_s, to_s) = (from.to_string(), to.to_string());
    let viewer_id = viewer.member_id;
    let sees_all = viewer.is_admin();

    let found = sqlx::query!(
        r#"SELECT
               t.id              AS "id!: i64",
               t.period          AS "period!: String",
               t.member_id       AS "member_id!: i64",
               t.kind            AS "kind!: String",
               t.amount_cents    AS "amount_cents: i64",
               t.title           AS "title!: String",
               t.bucket          AS "bucket: String",
               t.rule_id         AS "rule_id: i64",
               t.manual_paid     AS "manual_paid!: i64",
               r.tracked         AS "tracked?: i64",
               r.varies          AS "varies?: i64",
               r.estimate_basis  AS "estimate_basis?: String",
               r.estimate_window AS "estimate_window?: i64",
               r.amount_cents    AS "rule_amount_cents?: i64"
           FROM transactions t
           LEFT JOIN recurring_rules r ON r.id = t.rule_id
           WHERE t.deleted_at IS NULL
             AND t.period >= ?1 AND t.period <= ?2
             -- The pane's own rule: a private row is its owner's and the
             -- admin's (`Viewer::sees_private_of`), never under preview.
             AND (t.visibility = 'household' OR t.member_id = ?3 OR ?4)
             -- A turned-off member's months are hidden from every total, as
             -- `members_in` hides their pane.
             AND NOT EXISTS (SELECT 1 FROM member_inactive i
                             WHERE i.member_id = t.member_id AND i.from_period <= t.period
                               AND (i.until_period IS NULL OR t.period < i.until_period))
             AND EXISTS (SELECT 1 FROM members m
                         WHERE m.id = t.member_id AND m.deleted_at IS NULL)
           ORDER BY t.period, t.member_id, t.sort_key"#,
        from_s,
        to_s,
        viewer_id,
        sees_all,
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(found.len());
    for r in found {
        let Some(period) = Period::parse(&r.period) else {
            continue;
        };
        let kind = TxKind::parse(&r.kind).unwrap_or(TxKind::Out);
        let mut cents = r.amount_cents.map(Cents);
        let mut estimated = false;

        // A varying income shows its projection until the money lands — and
        // only then is the disk asked, so a year of plain rows costs nothing.
        if kind.is_income()
            && r.varies.unwrap_or(0) != 0
            && let Some(rule_id) = r.rule_id
            && !ledger::is_confirmed(
                pool,
                root,
                r.tracked.unwrap_or(0) != 0,
                r.id,
                r.manual_paid != 0,
            )
            .await
            && let Some(p) = ledger::projection(
                pool,
                root,
                rule_id,
                period,
                r.estimate_basis.as_deref(),
                r.estimate_window,
                r.rule_amount_cents,
            )
            .await?
        {
            cents = Some(p);
            estimated = true;
        }

        out.push(ChartRow {
            id: r.id,
            period,
            member_id: r.member_id,
            kind,
            cents,
            estimated,
            title: r.title,
            bucket: r.bucket,
            rule_id: r.rule_id,
        });
    }
    Ok(out)
}

/// The first month anything was recorded in, as this viewer sees it — where
/// the Years tab starts. `None` for an empty ledger.
pub async fn first_period(pool: &SqlitePool, viewer: Viewer) -> sqlx::Result<Option<Period>> {
    let viewer_id = viewer.member_id;
    let sees_all = viewer.is_admin();
    let p = sqlx::query_scalar!(
        r#"SELECT min(t.period) AS "period: String"
           FROM transactions t
           WHERE t.deleted_at IS NULL
             AND (t.visibility = 'household' OR t.member_id = ?1 OR ?2)"#,
        viewer_id,
        sees_all,
    )
    .fetch_one(pool)
    .await?;
    Ok(p.as_deref().and_then(Period::parse))
}
