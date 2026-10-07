//! Turning rules into rows.
//!
//! An in-process task, ticking hourly and **also run once at startup**. No
//! external cron and no systemd unit: one process per household is the whole
//! deployment. Startup matters as much as the tick — a process that was down
//! across a month boundary has to catch up the moment it is back, not up to an
//! hour later.
//!
//! **Not lazily on view.** Materializing when somebody opens a month would make
//! "does this row exist" depend on "did anyone happen to look", which silently
//! breaks every query that reads across months: the unresolved badge would
//! report all-clear for a month nobody opened, and MoM deltas and `:grep` would
//! have nothing to find.
//!
//! **Reconcile per rule; keep no watermark.** For each live rule, find the
//! periods between its start and `min(current_month, end_date)` that have no
//! occurrence, and insert those. The absence of rows *is* the state, so there
//! is nothing to keep in sync. A `last_checked` control value would look
//! simpler and quietly get it wrong: create a rule today whose start is six
//! months back, and a watermark says "already caught up". Reconciliation
//! handles that, plus downtime of any length, edited start dates and
//! reactivated rules, for free.

use crate::db::changes::{Actor, Change, TxAction};
use crate::domain::money::Cents;
use crate::domain::months::Months;
use crate::domain::period::Period;
use crate::domain::sortkey;
use chrono::{Datelike, NaiveDate};
use sqlx::SqlitePool;
use std::time::Duration;

/// How often the tick runs. An hour is far more often than a household's rules
/// change; what it really bounds is how long after midnight on the 1st the new
/// month's rows appear.
const TICK: Duration = Duration::from_secs(3600);

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub rules: usize,
    pub created: usize,
}

/// Runs one reconciliation pass over every live rule.
///
/// Safe to run concurrently with itself: `UNIQUE(rule_id, period)` makes a
/// double insert impossible, so the startup run and an hourly tick can race
/// harmlessly. That is also what removes double-insertion from the open
/// concurrency question entirely.
pub async fn reconcile(pool: &SqlitePool, today: NaiveDate) -> sqlx::Result<Report> {
    let rules = sqlx::query!(
        r#"SELECT id AS "id!: i64", member_id AS "member_id!: i64",
                  kind AS "kind!: String", title AS "title!: String",
                  amount_cents AS "amount_cents: i64",
                  cadence_months AS "cadence_months!: i64",
                  due_day AS "due_day: i64",
                  start_date AS "start_date!: String",
                  end_date AS "end_date: String",
                  total_installments AS "total_installments: i64",
                  carry_forward_amount AS "carry_forward_amount!: i64",
                  bucket AS "bucket: String", bill_name AS "bill_name: String",
                  months AS "months: String",
                  visibility AS "visibility!: String"
           FROM recurring_rules WHERE deleted_at IS NULL"#
    )
    .fetch_all(pool)
    .await?;

    // The stretches each member was inactive in. A month inside one has no
    // pane for that member and counts in no total, so a bill materialized
    // there would be a row nobody sees — and turning them back on must not
    // backfill the months they were away, which is exactly what reconciling
    // from `start_date` would otherwise do.
    let inactive = sqlx::query!(
        r#"SELECT member_id AS "member_id!: i64", from_period AS "from_period!: String",
                  until_period AS "until_period: String"
           FROM member_inactive"#
    )
    .fetch_all(pool)
    .await?;
    let away = |member: i64, p: &str| {
        inactive.iter().any(|i| {
            i.member_id == member
                && i.from_period.as_str() <= p
                && i.until_period.as_deref().is_none_or(|u| p < u)
        })
    };

    let current = Period::of(today);
    let mut report = Report {
        rules: rules.len(),
        created: 0,
    };

    for r in rules {
        let Ok(start) = NaiveDate::parse_from_str(&r.start_date, "%Y-%m-%d") else {
            continue;
        };
        let end = r
            .end_date
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());

        // Never past the current month. Future months stay projections
        // rendered on the fly and are never persisted.
        let last = match end {
            Some(e) if Period::of(e) < current => Period::of(e),
            _ => current,
        };

        // The seasonal allow-list, and the ONLY place it is read. Everything
        // downstream reads occurrences, and an occurrence that was never
        // materialized is simply not there — which is what keeps a rule that
        // skips November a small change rather than a cross-cutting one.
        //
        // A value that no longer parses degrades to "every month" rather than
        // silently materializing nothing: a rule that quietly stops producing
        // rows is the worst failure this loop has.
        let months = r.months.as_deref().and_then(Months::from_storage);

        let cadence = r.cadence_months.max(1) as u32;
        let mut period = Period::of(start);
        let mut n = 0i64;

        while period <= last {
            // A finite run stops at its installment count, whatever the dates
            // say — the two are kept in sync by the form, and this is the
            // safety net if they ever disagree.
            if let Some(total) = r.total_installments
                && n >= total
            {
                break;
            }

            // A skipped month is not an occurrence that failed to appear —
            // it is a month this rule does not run in. It still advances the
            // cadence and the installment count, because it is a period the
            // rule passed through.
            if let Some(m) = &months
                && !m.contains(period.month())
            {
                // It does NOT advance the installment count. `n` counts
                // payments, and a month the rule does not run in is not one.
                // (A rule with both a months list and a fixed installment
                // count is an odd shape — the form's end-date sync still
                // computes in whole months and does not know about skips —
                // but if somebody builds one, "12 installments" has to mean
                // twelve actual rows.)
                period = period.succ_n(cadence);
                continue;
            }

            let period_s = period.to_string();

            // A month the member was inactive in: skipped like a month the
            // rule does not run in, and for the same reason it is not a
            // payment, so it does not count toward the installments either.
            if away(r.member_id, &period_s) {
                period = period.succ_n(cadence);
                continue;
            }

            // Soft-deleted occurrences count as PRESENT. This is where two
            // decided rules collide: deletes are soft, so a `dd`'d occurrence
            // is still a row. Looking only for live rows would resurrect every
            // deleted occurrence on the next tick and fight the household once
            // an hour.
            let exists: i64 = sqlx::query_scalar!(
                r#"SELECT count(*) AS "n!: i64" FROM transactions
                   WHERE rule_id = ?1 AND period = ?2"#,
                r.id,
                period_s
            )
            .fetch_one(pool)
            .await?;

            if exists == 0 {
                let amount = seed_amount(
                    pool,
                    r.id,
                    r.carry_forward_amount != 0,
                    r.amount_cents.map(Cents),
                )
                .await?;

                let created = insert_occurrence(
                    pool,
                    Occurrence {
                        rule_id: r.id,
                        member_id: r.member_id,
                        period,
                        kind: &r.kind,
                        title: &r.title,
                        amount,
                        due_day: r.due_day,
                        bucket: r.bucket.as_deref(),
                        bill_name: r.bill_name.as_deref(),
                        visibility: &r.visibility,
                    },
                )
                .await?;
                if created {
                    report.created += 1;
                }
            }

            period = period.succ_n(cadence);
            n += 1;
        }
    }

    Ok(report)
}

/// What a new occurrence starts with.
///
/// Carry-forward is opt-in per rule and seeds from the **previous period's
/// actual value**, which is what makes a bill that drifts slowly read right.
/// With it off the row starts with no amount at all — the `R$ __,__`
/// placeholder — forcing an explicit edit, because for a credit-card statement
/// auto-filling last month's number would be actively wrong rather than merely
/// unhelpful.
///
/// The very first occurrence has nothing to carry forward from, so it always
/// falls back to the rule's own amount.
async fn seed_amount(
    pool: &SqlitePool,
    rule_id: i64,
    carry_forward: bool,
    rule_amount: Option<Cents>,
) -> sqlx::Result<Option<Cents>> {
    if !carry_forward {
        return Ok(None);
    }
    let previous: Option<i64> = sqlx::query_scalar!(
        r#"SELECT amount_cents AS "a: i64" FROM transactions
           WHERE rule_id = ?1 AND amount_cents IS NOT NULL AND deleted_at IS NULL
           ORDER BY period DESC LIMIT 1"#,
        rule_id
    )
    .fetch_optional(pool)
    .await?
    .flatten();

    Ok(previous.map(Cents).or(rule_amount))
}

struct Occurrence<'a> {
    rule_id: i64,
    member_id: i64,
    period: Period,
    kind: &'a str,
    title: &'a str,
    amount: Option<Cents>,
    due_day: Option<i64>,
    bucket: Option<&'a str>,
    bill_name: Option<&'a str>,
    visibility: &'a str,
}

/// Inserts one occurrence and logs it, in one transaction.
///
/// Returns `false` when the unique index refused it — which is a *race won by
/// somebody else*, not an error: the row exists, which is all the caller
/// wanted.
async fn insert_occurrence(pool: &SqlitePool, o: Occurrence<'_>) -> sqlx::Result<bool> {
    let period_s = o.period.to_string();
    let mut tx = pool.begin().await?;

    // Income lands at the top of its pane, a spend at the bottom — the same
    // rule a hand-entered row follows, because a materialized row is an
    // ordinary transaction that happens to have a rule behind it.
    let neighbor: Option<String> = if o.kind == "in" {
        sqlx::query_scalar!(
            r#"SELECT min(sort_key) AS "k: String" FROM transactions
               WHERE member_id = ?1 AND period = ?2 AND deleted_at IS NULL"#,
            o.member_id,
            period_s
        )
        .fetch_one(&mut *tx)
        .await?
    } else {
        sqlx::query_scalar!(
            r#"SELECT max(sort_key) AS "k: String" FROM transactions
               WHERE member_id = ?1 AND period = ?2 AND deleted_at IS NULL"#,
            o.member_id,
            period_s
        )
        .fetch_one(&mut *tx)
        .await?
    };
    let sort_key = match (o.kind == "in", neighbor.as_deref()) {
        (true, Some(first)) => sortkey::before(first),
        (false, Some(last)) => sortkey::after(last),
        (_, None) => sortkey::between(None, None),
    };

    // The rule owns `due_day`; materializing stamps a real date from it,
    // clamped to this month's length — so a rule due on the 31st means the
    // 28th in February rather than failing. Downstream, the stamped date is
    // what the ring, the overdue check and the unresolved badge read.
    let due_date = o
        .due_day
        .and_then(|d| u32::try_from(d).ok())
        .and_then(|d| o.period.due_date(d))
        .map(|d| d.format("%Y-%m-%d").to_string());

    let at = crate::clock::stamp();
    let amount = o.amount.map(|c| c.get());

    let inserted = sqlx::query!(
        r#"INSERT OR IGNORE INTO transactions
               (member_id, rule_id, period, kind, title, amount_cents, date,
                due_date, manual_paid, visibility, bucket, bill_name, sort_key,
                created_at, created_by)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, 0, ?8, ?9, ?10, ?11, ?12, NULL)"#,
        o.member_id,
        o.rule_id,
        period_s,
        o.kind,
        o.title,
        amount,
        due_date,
        o.visibility,
        o.bucket,
        o.bill_name,
        sort_key,
        at,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if inserted == 0 {
        // Somebody else's tick got there first. Nothing to log.
        return Ok(false);
    }

    let id: i64 = sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM transactions
           WHERE rule_id = ?1 AND period = ?2"#,
        o.rule_id,
        period_s
    )
    .fetch_one(&mut *tx)
    .await?;

    // `source = materializer`, and no actor: nobody typed this. The log says
    // so rather than attributing it to whoever happened to be signed in.
    Change::transaction(Actor::Materializer, TxAction::Materialize, id)
        .from_rule(Some(o.rule_id))
        .in_period(o.period)
        .write(&mut tx)
        .await?;

    tx.commit().await?;

    // A month a covering bill already has a code for — a loan's carnê, or a
    // cycle attached before its months came round — starts with that code's
    // amount and due date, the way every month did when the file was attached.
    // The bill's import, not the materializer's guess: logged as one.
    if let Some(code) = crate::db::bill_codes::for_period(pool, o.rule_id, o.period).await? {
        crate::db::bill_codes::stamp(pool, Actor::Import(None), id, &code).await?;
    }
    Ok(true)
}

/// Starts the background task: one pass now, then one an hour.
pub fn spawn(pool: SqlitePool) {
    tokio::spawn(async move {
        loop {
            let today = crate::clock::today();
            match reconcile(&pool, today).await {
                Ok(r) if r.created > 0 => {
                    tracing::info!(
                        "materialized {} occurrence(s) from {} rule(s)",
                        r.created,
                        r.rules
                    );
                }
                Ok(_) => tracing::debug!("materializer: nothing to do"),
                // A failed pass is not fatal: the next one reconciles from the
                // rows themselves, so there is no state to have corrupted.
                Err(e) => tracing::error!("materializer: {e}"),
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// The day of the month a rule's date falls on, for the form's sync of
/// installments and end date. Public because the rule editor needs the same
/// answer the materializer would give.
pub fn due_day_of(date: NaiveDate) -> u32 {
    date.day()
}
