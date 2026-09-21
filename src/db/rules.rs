//! Recurring rules — defined once, materialized into the ledger each period.
//!
//! A rule is **created from scratch**, never by promoting an existing row.
//! `A`/`I` open a blank editor with no anchor transaction at all, and saving
//! inserts a genuinely new row rather than mutating one that was already there.
//! The reason is the barcode binding: a rule can be bound to a bill file, and
//! that has to happen at creation or not at all — there is no still-plain
//! transaction lying around afterwards to attach it to.

use crate::db::changes::{Actor, Change, RuleAction, Value};
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::status::{TxKind, Visibility};
use chrono::{NaiveDate, Utc};
use sqlx::SqlitePool;

fn now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn iso(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Everything the rule form collects. Not `Rule` itself, because a form is
/// input and a row is a fact — `id`, `created_at` and `deleted_at` belong to
/// the latter only.
#[derive(Clone, Debug)]
pub struct RuleForm {
    pub member_id: i64,
    pub kind: TxKind,
    pub title: String,
    /// Seeds the very first occurrence. After that, carry-forward or the
    /// household's own edits decide.
    pub amount: Option<Cents>,
    /// Months between occurrences. An integer, not a word: there is no English
    /// word for the four-month case, and "bimonthly" means both every two
    /// months and twice a month.
    pub cadence_months: i64,
    /// Day of the month. The rule owns this; the materializer stamps a real
    /// date from it onto each occurrence, clamped to that month's length.
    pub due_day: Option<i64>,
    pub start_date: NaiveDate,
    /// `None` = ongoing. Plain and independently settable on every rule — this
    /// is how an ordinary open-ended bill gets cancelled, with nothing about
    /// installments involved.
    pub end_date: Option<NaiveDate>,
    /// A finite, non-cyclic run. Set only for something like a car loan.
    pub total_installments: Option<i64>,
    pub carry_forward_amount: bool,
    pub tracked: bool,
    pub bucket: Option<String>,
    pub bill_name: Option<String>,
    pub instance: Option<String>,
    pub no_barcode: bool,
    pub varies: bool,
    pub estimate_basis: String,
    pub estimate_window: i64,
    pub amount_in_filename: bool,
    pub visibility: Visibility,
}

/// A rule as stored, with the fields the pane and the materializer read.
#[derive(Clone, Debug)]
pub struct Rule {
    pub id: i64,
    pub member_id: i64,
    pub kind: TxKind,
    pub title: String,
    pub amount: Option<Cents>,
    pub cadence_months: i64,
    pub due_day: Option<i64>,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub total_installments: Option<i64>,
    pub carry_forward_amount: bool,
    pub tracked: bool,
    pub bucket: Option<String>,
    pub bill_name: Option<String>,
    pub instance: Option<String>,
    pub no_barcode: bool,
    pub varies: bool,
    pub estimate_basis: String,
    pub estimate_window: i64,
    pub amount_in_filename: bool,
    pub visibility: Visibility,
}

/// `end_date` and `total_installments` are two ways of saying the same thing
/// for a finite rule, and the form keeps them in sync. This is the direction
/// that has to agree with the materializer, so it lives here rather than in a
/// template's JavaScript.
///
/// The last installment lands `(n - 1)` cadences after the start: a
/// twelve-month loan starting in March ends in February, not the March after.
pub fn end_from_installments(
    start: NaiveDate,
    cadence_months: i64,
    installments: i64,
) -> Option<NaiveDate> {
    let steps = u32::try_from((installments - 1).max(0) * cadence_months).ok()?;
    Period::of(start)
        .succ_n(steps)
        .due_date(chrono::Datelike::day(&start))
}

/// The inverse, for when somebody types an end date instead.
pub fn installments_from_end(start: NaiveDate, cadence_months: i64, end: NaiveDate) -> Option<i64> {
    if end < start || cadence_months <= 0 {
        return None;
    }
    let months = Period::of(start).months_until(Period::of(end)) as i64;
    Some(months / cadence_months + 1)
}

pub async fn create(pool: &SqlitePool, actor: Actor, form: &RuleForm) -> sqlx::Result<i64> {
    let mut tx = pool.begin().await?;
    let at = now();
    let kind = form.kind.code();
    let visibility = form.visibility.code();
    let amount = form.amount.map(|c| c.get());
    let start = iso(form.start_date);
    let end = form.end_date.map(iso);
    let carry = i64::from(form.carry_forward_amount);
    let tracked = i64::from(form.tracked);
    let no_barcode = i64::from(form.no_barcode);
    let varies = i64::from(form.varies);
    let in_filename = i64::from(form.amount_in_filename);

    let id = sqlx::query_scalar!(
        r#"INSERT INTO recurring_rules
               (member_id, title, kind, amount_cents, cadence_months, due_day,
                start_date, end_date, total_installments, carry_forward_amount,
                tracked, bucket, bill_name, instance, no_barcode, varies,
                estimate_basis, estimate_window, amount_in_filename,
                visibility, created_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                   ?15, ?16, ?17, ?18, ?19, ?20, ?21)
           RETURNING id AS "id!: i64""#,
        form.member_id,
        form.title,
        kind,
        amount,
        form.cadence_months,
        form.due_day,
        start,
        end,
        form.total_installments,
        carry,
        tracked,
        form.bucket,
        form.bill_name,
        form.instance,
        no_barcode,
        varies,
        form.estimate_basis,
        form.estimate_window,
        in_filename,
        visibility,
        at,
    )
    .fetch_one(&mut *tx)
    .await?;

    Change::rule(actor, RuleAction::Create, id)
        .diff("title", Value::Text(None), Value::text(&form.title))
        .diff("amount", Value::Money(None), Value::Money(form.amount))
        .diff(
            "due_day",
            Value::Day(None),
            Value::Day(form.due_day.map(|d| d as u32)),
        )
        .write(&mut tx)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Edits a rule, recording only the fields that actually moved.
///
/// **Editing `due_day` changes future materializations only.** Past
/// occurrences keep the date they were stamped with, exactly as the rule
/// form's own notebox promises — the occurrence's stamped date is what the
/// ring, the overdue check and the unresolved badge all read.
pub async fn edit(pool: &SqlitePool, actor: Actor, id: i64, form: &RuleForm) -> sqlx::Result<bool> {
    let Some(old) = get(pool, id).await? else {
        return Ok(false);
    };

    let mut tx = pool.begin().await?;
    let kind = form.kind.code();
    let visibility = form.visibility.code();
    let amount = form.amount.map(|c| c.get());
    let start = iso(form.start_date);
    let end = form.end_date.map(iso);
    let carry = i64::from(form.carry_forward_amount);
    let tracked = i64::from(form.tracked);
    let no_barcode = i64::from(form.no_barcode);
    let varies = i64::from(form.varies);
    let in_filename = i64::from(form.amount_in_filename);

    sqlx::query!(
        r#"UPDATE recurring_rules SET
               title = ?2, kind = ?3, amount_cents = ?4, cadence_months = ?5,
               due_day = ?6, start_date = ?7, end_date = ?8,
               total_installments = ?9, carry_forward_amount = ?10,
               tracked = ?11, bucket = ?12, bill_name = ?13, instance = ?14,
               no_barcode = ?15, varies = ?16, estimate_basis = ?17,
               estimate_window = ?18, amount_in_filename = ?19, visibility = ?20
           WHERE id = ?1"#,
        id,
        form.title,
        kind,
        amount,
        form.cadence_months,
        form.due_day,
        start,
        end,
        form.total_installments,
        carry,
        tracked,
        form.bucket,
        form.bill_name,
        form.instance,
        no_barcode,
        varies,
        form.estimate_basis,
        form.estimate_window,
        in_filename,
        visibility,
    )
    .execute(&mut *tx)
    .await?;

    let entry = Change::rule(actor, RuleAction::Edit, id)
        .diff("title", Value::text(&old.title), Value::text(&form.title))
        .diff(
            "amount",
            Value::Money(old.amount),
            Value::Money(form.amount),
        )
        .diff(
            "due_day",
            Value::Day(old.due_day.map(|d| d as u32)),
            Value::Day(form.due_day.map(|d| d as u32)),
        )
        .diff(
            "bucket",
            Value::Text(old.bucket.clone()),
            Value::Text(form.bucket.clone()),
        )
        .diff(
            "bill_name",
            Value::Text(old.bill_name.clone()),
            Value::Text(form.bill_name.clone()),
        )
        .diff(
            "cadence",
            Value::Text(Some(old.cadence_months.to_string())),
            Value::Text(Some(form.cadence_months.to_string())),
        )
        .diff(
            "start_date",
            Value::Date(Some(old.start_date)),
            Value::Date(Some(form.start_date)),
        )
        .diff(
            "end_date",
            Value::Date(old.end_date),
            Value::Date(form.end_date),
        )
        .diff(
            "tracked",
            Value::Bool(old.tracked),
            Value::Bool(form.tracked),
        );

    // A save that changed nothing writes nothing: a log full of empty edits is
    // a log nobody reads.
    if !entry.is_empty() {
        entry.write(&mut tx).await?;
    }

    tx.commit().await?;
    Ok(true)
}

/// Marks a rule deleted. Its occurrences are left exactly as they are — they
/// are what happened, and deleting the rule that produced them would rewrite
/// history. What it stops is *future* materialization.
pub async fn soft_delete(pool: &SqlitePool, actor: Actor, id: i64) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    let at = now();
    let n = sqlx::query!(
        "UPDATE recurring_rules SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
        id,
        at
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if n == 0 {
        return Ok(false);
    }

    Change::rule(actor, RuleAction::Delete, id)
        .write(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn get(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<Rule>> {
    let row = sqlx::query!(
        r#"SELECT id AS "id!: i64", member_id AS "member_id!: i64",
                  kind AS "kind!: String", title AS "title!: String",
                  amount_cents AS "amount_cents: i64",
                  cadence_months AS "cadence_months!: i64",
                  due_day AS "due_day: i64",
                  start_date AS "start_date!: String",
                  end_date AS "end_date: String",
                  total_installments AS "total_installments: i64",
                  carry_forward_amount AS "carry_forward_amount!: i64",
                  tracked AS "tracked!: i64",
                  bucket AS "bucket: String", bill_name AS "bill_name: String",
                  instance AS "instance: String",
                  no_barcode AS "no_barcode!: i64", varies AS "varies!: i64",
                  estimate_basis AS "estimate_basis!: String",
                  estimate_window AS "estimate_window!: i64",
                  amount_in_filename AS "amount_in_filename!: i64",
                  visibility AS "visibility!: String"
           FROM recurring_rules WHERE id = ?1 AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(|r| {
        Some(Rule {
            id: r.id,
            member_id: r.member_id,
            kind: TxKind::parse(&r.kind)?,
            title: r.title,
            amount: r.amount_cents.map(Cents),
            cadence_months: r.cadence_months,
            due_day: r.due_day,
            start_date: NaiveDate::parse_from_str(&r.start_date, "%Y-%m-%d").ok()?,
            end_date: r
                .end_date
                .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
            total_installments: r.total_installments,
            carry_forward_amount: r.carry_forward_amount != 0,
            tracked: r.tracked != 0,
            bucket: r.bucket,
            bill_name: r.bill_name,
            instance: r.instance,
            no_barcode: r.no_barcode != 0,
            varies: r.varies != 0,
            estimate_basis: r.estimate_basis,
            estimate_window: r.estimate_window,
            amount_in_filename: r.amount_in_filename != 0,
            visibility: Visibility::parse(&r.visibility)?,
        })
    }))
}

/// The per-member tag cloud behind the Bucket combobox: the member's own
/// previously-used values, not a category table and not shared across
/// accounts. A kid's school-cafeteria labels have no business appearing in a
/// parent's utility dropdown.
pub async fn buckets(pool: &SqlitePool, member_id: i64) -> sqlx::Result<Vec<String>> {
    let rows = sqlx::query_scalar!(
        r#"SELECT DISTINCT bucket AS "bucket!: String" FROM (
               SELECT bucket FROM recurring_rules
                WHERE member_id = ?1 AND bucket IS NOT NULL AND deleted_at IS NULL
               UNION
               SELECT bucket FROM transactions
                WHERE member_id = ?1 AND bucket IS NOT NULL AND deleted_at IS NULL
           ) ORDER BY bucket"#,
        member_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn a_twelve_month_loan_ends_eleven_months_later_not_twelve() {
        // The last installment lands on the twelfth occurrence, which is
        // eleven cadences after the first. Off by one here and every car loan
        // in the household runs a month long.
        assert_eq!(
            end_from_installments(d(2026, 3, 5), 1, 12),
            Some(d(2027, 2, 5))
        );
        assert_eq!(
            installments_from_end(d(2026, 3, 5), 1, d(2027, 2, 5)),
            Some(12)
        );
    }

    #[test]
    fn the_two_fields_round_trip_through_each_other() {
        for (cadence, n) in [(1, 6), (2, 6), (3, 4), (6, 2), (12, 3)] {
            let start = d(2026, 1, 10);
            let end = end_from_installments(start, cadence, n).unwrap();
            assert_eq!(
                installments_from_end(start, cadence, end),
                Some(n),
                "cadence {cadence}, {n} installments"
            );
        }
    }

    #[test]
    fn a_due_day_past_the_end_of_a_month_is_clamped() {
        // A loan due on the 31st, ending in February.
        assert_eq!(
            end_from_installments(d(2026, 1, 31), 1, 2),
            Some(d(2026, 2, 28))
        );
    }

    #[test]
    fn an_end_before_the_start_is_not_an_installment_count() {
        assert_eq!(installments_from_end(d(2026, 3, 5), 1, d(2026, 1, 5)), None);
    }
}
