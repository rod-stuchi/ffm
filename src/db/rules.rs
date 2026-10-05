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
use crate::domain::months::Months;
use crate::domain::period::Period;
use crate::domain::status::{TxKind, Visibility};
use chrono::NaiveDate;
use sqlx::{Sqlite, SqlitePool, Transaction};

fn now() -> String {
    crate::clock::stamp()
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
    /// is how an ordinary open-ended bill gets canceled, with nothing about
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
    /// One uploaded document covering several periods. Off by default: most
    /// bills genuinely do get a new file each month.
    pub spans_period: bool,
    pub window_size: Option<i64>,
    /// Which period is page 1 of the current file.
    pub window_start: Option<Period>,
    pub varies: bool,
    pub estimate_basis: String,
    pub estimate_window: i64,
    pub amount_in_filename: bool,
    /// Which months of the year the rule actually runs. `None` = every month,
    /// which is why the column is nullable rather than holding all twelve:
    /// "seasonal" and "ordinary" stay distinguishable in the value itself.
    pub months: Option<Months>,
    pub visibility: Visibility,
    /// What a locked PDF of this rule opens with — see migration 0012. Only a
    /// tracked rule keeps one; the form drops it otherwise.
    pub pdf_password: Option<String>,
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
    /// The subscriber's own account number, read from a bill's **text layer**
    /// — FEBRABAN never encodes it, because the barcode's job is value and due
    /// date, not who is paying. It lives on the rule rather than the
    /// occurrence: the same account every period. Populated only by attaching
    /// a file; there is no form field for it.
    pub client_code: Option<String>,
    /// The rest of what the rule's bill is bound to — see migration 0007.
    /// `barcode_company` is the fallback binding when no profile could read a
    /// client code; `issuer_id` and `barcode_kind` are only ever displayed.
    pub issuer_id: Option<String>,
    pub barcode_company: Option<String>,
    pub barcode_kind: Option<String>,
    pub spans_period: bool,
    pub window_size: Option<i64>,
    pub window_start: Option<Period>,
    pub varies: bool,
    pub estimate_basis: String,
    pub estimate_window: i64,
    pub amount_in_filename: bool,
    /// `None` = runs every month.
    pub months: Option<Months>,
    pub visibility: Visibility,
    /// Plain text on purpose — it has to open files. Never logged, and only
    /// rendered to somebody who may write the rule.
    pub pdf_password: Option<String>,
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
    let spans = i64::from(form.spans_period);
    let window_start = form.window_start.map(|p| p.to_string());
    let months = form.months.as_ref().map(|m| m.to_storage());

    let id = sqlx::query_scalar!(
        r#"INSERT INTO recurring_rules
               (member_id, title, kind, amount_cents, cadence_months, due_day,
                start_date, end_date, total_installments, carry_forward_amount,
                tracked, bucket, bill_name, instance, no_barcode, varies,
                estimate_basis, estimate_window, amount_in_filename,
                visibility, created_at, spans_period, window_size, window_start,
                months, pdf_password)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                   ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)
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
        spans,
        form.window_size,
        window_start,
        months,
        form.pdf_password,
    )
    .fetch_one(&mut *tx)
    .await?;

    record_window(&mut tx, id, form, &at).await?;

    Change::rule(actor, RuleAction::Create, id)
        .diff("title", Value::Text(None), Value::text(&form.title))
        .diff("amount", Value::Money(None), Value::Money(form.amount))
        .diff(
            "due_day",
            Value::Day(None),
            Value::Day(form.due_day.map(|d| d as u32)),
        )
        .diff(
            "pdf_password",
            Value::Bool(false),
            Value::Bool(form.pdf_password.is_some()),
        )
        .write(&mut tx)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Edits a rule, recording only the fields that actually moved.
pub async fn edit(pool: &SqlitePool, actor: Actor, id: i64, form: &RuleForm) -> sqlx::Result<bool> {
    Ok(edit_reporting(pool, actor, id, form).await?.is_some())
}

/// What a rule save did beyond the rule itself.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Edited {
    /// This month's row followed a new due day — the date it moved to.
    pub moved_due: Option<NaiveDate>,
}

/// [`edit`], saying what else moved. `None` when there is no such rule.
///
/// **A new `due_day` reaches future materializations, and this month's row
/// while it is unconfirmed.** It used to reach only the future, so the row a
/// person was looking at kept the old day after they changed the rule — and a
/// rule ending this month never showed the new day anywhere. Earlier months,
/// and this month once paid or received, keep the date they were stamped with:
/// a confirmed row's due date is what it was paid against.
pub async fn edit_reporting(
    pool: &SqlitePool,
    actor: Actor,
    id: i64,
    form: &RuleForm,
) -> sqlx::Result<Option<Edited>> {
    let Some(old) = get(pool, id).await? else {
        return Ok(None);
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
    let spans = i64::from(form.spans_period);
    let window_start = form.window_start.map(|p| p.to_string());
    let months = form.months.as_ref().map(|m| m.to_storage());

    sqlx::query!(
        r#"UPDATE recurring_rules SET
               title = ?2, kind = ?3, amount_cents = ?4, cadence_months = ?5,
               due_day = ?6, start_date = ?7, end_date = ?8,
               total_installments = ?9, carry_forward_amount = ?10,
               tracked = ?11, bucket = ?12, bill_name = ?13, instance = ?14,
               no_barcode = ?15, varies = ?16, estimate_basis = ?17,
               estimate_window = ?18, amount_in_filename = ?19, visibility = ?20,
               spans_period = ?21, window_size = ?22, window_start = ?23,
               months = ?24, pdf_password = ?25
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
        spans,
        form.window_size,
        window_start,
        months,
        form.pdf_password,
    )
    .execute(&mut *tx)
    .await?;

    record_window(&mut tx, id, form, &now()).await?;

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
    let entry = password_field(entry, &old.pdf_password, &form.pdf_password);

    // A save that changed nothing writes nothing: a log full of empty edits is
    // a log nobody reads.
    if !entry.is_empty() {
        entry.write(&mut tx).await?;
    }

    let mut edited = Edited::default();
    if old.due_day != form.due_day {
        edited.moved_due = follow_due_day(&mut tx, actor, id, form.due_day).await?;
    }

    tx.commit().await?;
    Ok(Some(edited))
}

/// The PDF password as the log sees it: whether one is set, never what it is.
///
/// A password replaced by another is still on → on, which `diff` would drop as
/// unchanged — so that one case is written with `always`, and the log still
/// says the password moved without saying to what.
fn password_field(entry: Change, old: &Option<String>, new: &Option<String>) -> Change {
    let (was, is) = (Value::Bool(old.is_some()), Value::Bool(new.is_some()));
    if old.is_some() && new.is_some() && old != new {
        entry.always("pdf_password", was, is)
    } else {
        entry.diff("pdf_password", was, is)
    }
}

/// Restamps this month's occurrence of a rule with a new due day, unless it is
/// confirmed: a live receipt (or payslip) for a tracked rule, the manual tick
/// for an untracked one. Logged on the row as an `edit` of `due_date`, so `u`
/// puts the old date back. Returns the new date when the row moved.
async fn follow_due_day(
    tx: &mut Transaction<'_, Sqlite>,
    actor: Actor,
    rule_id: i64,
    due_day: Option<i64>,
) -> sqlx::Result<Option<NaiveDate>> {
    let period = Period::of(crate::clock::today());
    let key = period.to_string();
    let row = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.due_date AS "due_date: String"
           FROM transactions t JOIN recurring_rules r ON r.id = t.rule_id
           WHERE t.rule_id = ?1 AND t.period = ?2 AND t.deleted_at IS NULL
             AND NOT (CASE WHEN r.tracked = 1
                           THEN EXISTS (SELECT 1 FROM receipts x
                                        WHERE x.transaction_id = t.id
                                          AND x.kind = 'receipt'
                                          AND x.deleted_at IS NULL)
                           ELSE t.manual_paid = 1 END)"#,
        rule_id,
        key
    )
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else { return Ok(None) };

    let old = row
        .due_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    let new = due_day
        .and_then(|d| u32::try_from(d).ok())
        .and_then(|d| period.due_date(d));
    if old == new {
        return Ok(None);
    }
    let stamp = new.map(iso);
    sqlx::query!(
        "UPDATE transactions SET due_date = ?2, version = version + 1 WHERE id = ?1",
        row.id,
        stamp
    )
    .execute(&mut **tx)
    .await?;
    Change::transaction(actor, crate::db::changes::TxAction::Edit, row.id)
        .from_rule(Some(rule_id))
        .in_period(period)
        .diff("due_date", Value::Date(old), Value::Date(new))
        .write(tx)
        .await?;
    Ok(new)
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

/// Records the rule's current barcode window as a row, retiring whatever was
/// live before it.
///
/// This is what closes the KNOWN HOLE: the page an occurrence shows is derived
/// from the window that covered **its own** period, so renewing a cyclic
/// bill's window no longer rewrites last year's rows into "page −11, outside
/// this file". Re-entering the two fields *is* the renewal — there is no
/// separate control, and none is needed.
async fn record_window(
    tx: &mut Transaction<'_, Sqlite>,
    rule_id: i64,
    form: &RuleForm,
    at: &str,
) -> sqlx::Result<()> {
    let (Some(size), Some(start)) = (form.window_size, form.window_start) else {
        return Ok(());
    };
    if !form.spans_period {
        return Ok(());
    }
    let start = start.to_string();

    // Unchanged is not a renewal: an ordinary save must not retire the live
    // window and insert an identical one, or every edit would add a row.
    let live: Option<(String, i64)> = sqlx::query_as(
        "SELECT window_start, window_size FROM rule_barcode_window
          WHERE rule_id = ?1 AND retired_at IS NULL",
    )
    .bind(rule_id)
    .fetch_optional(&mut **tx)
    .await?;
    if live
        .as_ref()
        .is_some_and(|(s, n)| *s == start && *n == size)
    {
        return Ok(());
    }

    sqlx::query!(
        "UPDATE rule_barcode_window SET retired_at = ?2
          WHERE rule_id = ?1 AND retired_at IS NULL",
        rule_id,
        at
    )
    .execute(&mut **tx)
    .await?;

    sqlx::query!(
        r#"INSERT INTO rule_barcode_window
               (rule_id, window_start, window_size, uploaded_at)
           VALUES (?1, ?2, ?3, ?4)"#,
        rule_id,
        start,
        size,
        at,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
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
                  client_code AS "client_code: String",
                  issuer_id AS "issuer_id: String",
                  barcode_company AS "barcode_company: String",
                  barcode_kind AS "barcode_kind: String",
                  spans_period AS "spans_period!: i64",
                  window_size AS "window_size: i64",
                  window_start AS "window_start: String",
                  estimate_basis AS "estimate_basis!: String",
                  estimate_window AS "estimate_window!: i64",
                  amount_in_filename AS "amount_in_filename!: i64",
                  months AS "months: String",
                  visibility AS "visibility!: String",
                  pdf_password AS "pdf_password: String"
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
            client_code: r.client_code,
            issuer_id: r.issuer_id,
            barcode_company: r.barcode_company,
            barcode_kind: r.barcode_kind,
            spans_period: r.spans_period != 0,
            window_size: r.window_size,
            window_start: r.window_start.as_deref().and_then(Period::parse),
            varies: r.varies != 0,
            estimate_basis: r.estimate_basis,
            estimate_window: r.estimate_window,
            amount_in_filename: r.amount_in_filename != 0,
            months: r.months.as_deref().and_then(Months::from_storage),
            visibility: Visibility::parse(&r.visibility)?,
            pdf_password: r.pdf_password,
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
