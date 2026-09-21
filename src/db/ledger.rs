//! Reading a month.
//!
//! One query per pane, plus the household and member rows that frame it. The
//! interesting part is not the SELECT — it is what the query is obliged to
//! carry, and why.
//!
//! `LedgerRow`'s contract is that **every visual channel is a pure function of
//! fields on the struct**, which is what lets the sandbox render twenty states
//! with no database at all. Three facts cannot be derived from a single row and
//! so are computed here and hung on it: the previous period's amount (the MoM
//! delta's only input), how many occurrences of a rule have happened (the
//! `7/12` badge), and — later — whether each file exists on disk.

use crate::domain::money::{Cents, Currency};
use crate::domain::period::Period;
use crate::domain::status::{RowFlags, RowState, TxKind, Visibility};
use crate::domain::viewer::Viewer;
use crate::ui::i18n::Lang;
use crate::ui::row::{FilePresence, Gutter, LedgerRow, RowCtx};
use chrono::NaiveDate;
use sqlx::SqlitePool;

/// The one household row. Every member's numbers feed the same totals, which
/// is exactly why currency lives here and not beside theme.
#[derive(Clone, Debug)]
pub struct Household {
    pub name: String,
    pub currency: Currency,
    pub default_language: Lang,
    pub show_recurring: bool,
}

#[derive(Clone, Debug)]
pub struct Member {
    pub id: i64,
    pub name: String,
    pub uuid: String,
    pub slug: String,
    pub is_admin: bool,
}

/// One member's month: the rows, and the totals they add up to.
#[derive(Debug)]
pub struct Pane {
    pub member: Member,
    pub rows: Vec<LedgerRow>,
    pub ins: Cents,
    pub outs: Cents,
    /// Marked when any component of the totals is a projection rather than a
    /// fact — leaving projections out would make a future month's balance
    /// useless, and including them unmarked would present a guess as a fact.
    pub estimated: bool,
}

impl Pane {
    pub fn balance(&self) -> Cents {
        self.ins - self.outs
    }
}

pub async fn household(pool: &SqlitePool) -> sqlx::Result<Option<Household>> {
    let row = sqlx::query!(
        r#"SELECT name, locale_currency, default_language, ledger_show_recurring
           FROM household_settings WHERE id = 1"#
    )
    .fetch_optional(pool)
    .await?;

    // The CHECK constraints make every one of these a closed set, so an
    // unparseable value would mean the schema was bypassed. Fall back rather
    // than panic: a ledger that renders in the wrong currency is recoverable,
    // a process that will not start is not.
    Ok(row.map(|r| Household {
        name: r.name,
        currency: Currency::parse(&r.locale_currency).unwrap_or_default(),
        default_language: Lang::parse(&r.default_language).unwrap_or(Lang::En),
        show_recurring: r.ledger_show_recurring != 0,
    }))
}

/// Everyone with a pane, in creation order — which is the order the tab bar and
/// the split panes read left to right.
pub async fn members(pool: &SqlitePool) -> sqlx::Result<Vec<Member>> {
    let rows = sqlx::query!(
        r#"SELECT id, name, uuid, slug, role FROM members
           WHERE deleted_at IS NULL ORDER BY id"#
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Member {
            id: r.id,
            name: r.name,
            uuid: r.uuid,
            slug: r.slug,
            is_admin: r.role == 0,
        })
        .collect())
}

/// One member's rows for one period, in stored manual order, as **this viewer**
/// is allowed to see them.
///
/// Soft-deleted rows are left out here. They are not gone — nothing is — but
/// the pane is what the household reads every day, and a deleted row comes back
/// into it through `u`, which is the mutation phase's job rather than this
/// query's.
///
/// Privacy is applied **once**, in the SELECT, and the totals are then summed
/// from the rows it returned. That is deliberate and load-bearing: filtering
/// the list in one place and the sum in another is how a hidden row announces
/// itself by making the numbers fail to add up. There is no second predicate
/// to keep in step because there is no second predicate.
pub async fn pane(
    pool: &SqlitePool,
    member: &Member,
    period: Period,
    ctx: RowCtx,
    viewer: Viewer,
) -> sqlx::Result<Pane> {
    pane_with_vault(pool, member, period, ctx, viewer, None).await
}

/// The same query, with the receipt vault available so each tracked row can be
/// asked what is actually on disk.
///
/// `root` is optional because the sandbox and most tests have no vault — and a
/// row with no files is a perfectly honest row: it reads `pending`, which is
/// what "nothing has happened yet" means.
pub async fn pane_with_vault(
    pool: &SqlitePool,
    member: &Member,
    period: Period,
    ctx: RowCtx,
    viewer: Viewer,
    root: Option<&std::path::Path>,
) -> sqlx::Result<Pane> {
    let period_s = period.to_string();
    let prev_s = period.pred().to_string();
    let sees_private = viewer.sees_private_of(member.id);

    let rows = sqlx::query!(
        r#"
        SELECT
            t.id                 AS "id!: i64",
            t.rule_id            AS "rule_id: i64",
            t.period             AS "period!: String",
            t.kind               AS "kind!: String",
            t.title              AS "title!: String",
            t.amount_cents       AS "amount_cents: i64",
            t.date               AS "date: String",
            t.due_date           AS "due_date: String",
            t.visibility         AS "visibility!: String",
            t.bucket             AS "bucket: String",
            t.bill_name          AS "bill_name: String",
            t.manual_paid        AS "manual_paid!: i64",

            r.tracked            AS "tracked?: i64",
            r.instance           AS "instance?: String",
            r.total_installments AS "total_installments?: i64",
            r.spans_period       AS "spans_period?: i64",
            r.window_size        AS "window_size?: i64",
            r.window_start       AS "window_start?: String",
            r.amount_in_filename AS "amount_in_filename?: i64",
            r.varies             AS "varies?: i64",

            -- "The same line" in the previous period: exact title, same member.
            -- Unambiguous for a recurring row (it is the same rule) and the only
            -- workable answer for a one-off, which has no rule to match on. It
            -- degrades the right way: rename the line and the delta disappears,
            -- because it is not that line any more.
            (SELECT p.amount_cents
               FROM transactions p
              WHERE p.member_id = t.member_id
                AND p.title     = t.title
                AND p.period    = ?2
                AND p.deleted_at IS NULL
              ORDER BY p.sort_key
              LIMIT 1)           AS "prev_amount_cents: i64",

            -- The barcode window that covered THIS period, not the rule's
            -- current one. A cyclic bill renews by re-entering the window, and
            -- reading the current values would silently rewrite history: the
            -- moment the 2027 window is set, January 2026 stops reading
            -- "page 1" and starts reading "page -11, outside this file".
            -- "Derived, never stored" still holds — the page is computed, it
            -- is the window that persists.
            (SELECT w.window_start FROM rule_barcode_window w
              WHERE w.rule_id = t.rule_id AND w.window_start <= t.period
              ORDER BY w.window_start DESC LIMIT 1)
                               AS "hist_window_start?: String",
            (SELECT w.window_size FROM rule_barcode_window w
              WHERE w.rule_id = t.rule_id AND w.window_start <= t.period
              ORDER BY w.window_start DESC LIMIT 1)
                               AS "hist_window_size?: i64",

            -- The `7/12` badge. Counts occurrences of this rule up to and
            -- including this period, so it reads as position in the run.
            (SELECT COUNT(*)
               FROM transactions o
              WHERE o.rule_id IS NOT NULL
                AND o.rule_id = t.rule_id
                AND o.period <= t.period
                AND o.deleted_at IS NULL) AS "installments_paid!: i64"
        FROM transactions t
        LEFT JOIN recurring_rules r ON r.id = t.rule_id
        WHERE t.member_id  = ?1
          AND t.period     = ?3
          AND t.deleted_at IS NULL
          AND (t.visibility = 'household' OR ?4)
        ORDER BY t.sort_key
        "#,
        member.id,
        prev_s,
        period_s,
        sees_private,
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    let mut ins = Cents::ZERO;
    let mut outs = Cents::ZERO;

    for (i, r) in rows.into_iter().enumerate() {
        let kind = TxKind::parse(&r.kind).unwrap_or(TxKind::Out);
        let amount = r.amount_cents.map(Cents);
        if let Some(a) = amount {
            match kind {
                TxKind::In => ins += a,
                TxKind::Out => outs += a,
            }
        }

        let recurring = r.rule_id.is_some();
        let private = r.visibility == "private";

        out.push(LedgerRow {
            id: r.id,
            rule_id: r.rule_id,
            period: Period::parse(&r.period).unwrap_or(period),
            kind,
            title: r.title,
            amount_cents: amount,
            date: r.date.as_deref().and_then(parse_date),
            due_date: r.due_date.as_deref().and_then(parse_date),
            visibility: Visibility::parse(&r.visibility).unwrap_or_default(),
            bucket: r.bucket,
            bill_name: r.bill_name,
            manual_paid: r.manual_paid != 0,

            tracked: r.tracked.unwrap_or(0) != 0,
            instance: r.instance,
            total_installments: r.total_installments.map(|n| n as u16),
            spans_period: r.spans_period.unwrap_or(0) != 0,
            // The historical window wins where there is one; the rule's own
            // columns are the fallback for a rule whose window predates this
            // table, and the editor's current values for a brand-new one.
            window_size: r.hist_window_size.or(r.window_size).map(|n| n as u16),
            window_start: r
                .hist_window_start
                .as_deref()
                .or(r.window_start.as_deref())
                .and_then(Period::parse),
            amount_in_filename: r.amount_in_filename.unwrap_or(0) != 0,
            varies: r.varies.unwrap_or(0) != 0,

            member_uuid: member.uuid.clone(),
            member_slug: member.slug.clone(),

            prev_amount_cents: r.prev_amount_cents.map(Cents),
            // Phase 8. The projection is computed on read, never stored, so
            // this stays a derived value hung on the row — it just does not
            // exist yet, and a varying income renders its own baseline in the
            // meantime rather than a wrong guess.
            estimate_cents: None,
            installments_paid: recurring.then_some(r.installments_paid as u16),
            // Filled in below, once the row exists to be asked about.
            files: FilePresence::default(),

            state: RowState::Ok,
            flags: RowFlags {
                private,
                recurring,
                ..RowFlags::default()
            },
            gutter: Gutter::Absolute(i as u32 + 1),
            ctx,
        });
    }

    // The disk decides `paid`, so it is consulted per tracked row — one exact
    // path per kind, never a traversal. An untracked row has no taxonomy to
    // file under and is skipped entirely.
    if let Some(root) = root {
        for row in out.iter_mut().filter(|r| r.tracked) {
            let (Some(bucket), Some(bill_name)) = (row.bucket.clone(), row.bill_name.clone())
            else {
                continue;
            };
            let slot = crate::services::receipts::Slot {
                transaction_id: row.id,
                vault: format!("{}-{}", member.uuid, member.slug),
                bucket,
                bill_name,
                instance: row.instance.clone(),
                period: row.period,
                amount: row.amount_cents.map(|c| (c, ctx.currency)),
                amount_in_filename: row.amount_in_filename,
            };
            if let Ok(found) = crate::services::receipts::presence(pool, root, &slot).await {
                row.files = found;
            }
        }
    }

    Ok(Pane {
        member: member.clone(),
        rows: out,
        ins,
        outs,
        estimated: false,
    })
}

/// Dates are ISO-8601 TEXT, fixed width, so this is exact rather than lenient.
/// A value that does not parse came from outside the app.
fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}
