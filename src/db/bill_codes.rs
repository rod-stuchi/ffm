//! Each month's code in a bill that covers several months (migration 0017).
//!
//! Read once, when the file is attached (`services::installments`), and kept
//! one per month: the code, what it says, the page it is printed on, and the
//! shared file it came from. Every covered month shows that one file opened
//! on its own page, and carries its own amount and due date — stamped now for
//! the months that exist, and by the materializer for the ones that appear
//! later.

use crate::db::changes::{Actor, Change, TxAction, Value};
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::services::boleto::Amount;
use crate::services::installments::FoundCode;
use chrono::NaiveDate;
use sqlx::{Sqlite, SqlitePool, Transaction};

/// One month's code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BillCode {
    pub rule_id: i64,
    pub receipt_id: i64,
    pub period: Period,
    pub barcode_raw: String,
    pub kind: String,
    pub amount: Option<Cents>,
    pub due: Option<NaiveDate>,
    pub page: u32,
}

/// Retires the rule's live codes and keeps these, for the file `receipt_id`.
pub async fn replace(
    tx: &mut Transaction<'_, Sqlite>,
    rule_id: i64,
    receipt_id: i64,
    plan: &[(Period, FoundCode)],
    at: &str,
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE rule_bill_code SET retired_at = ?2 WHERE rule_id = ?1 AND retired_at IS NULL",
        rule_id,
        at
    )
    .execute(&mut **tx)
    .await?;
    for (period, code) in plan {
        let period_s = period.to_string();
        let kind = code.barcode.kind.code();
        let amount = match code.barcode.amount {
            Amount::Fixed(c) => Some(c.get()),
            _ => None,
        };
        let due = code
            .barcode
            .due
            .date()
            .map(|d| d.format("%Y-%m-%d").to_string());
        let page = code.page as i64;
        sqlx::query!(
            r#"INSERT INTO rule_bill_code
                   (rule_id, receipt_id, period, barcode_raw, kind, amount_cents,
                    due_date, page, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
            rule_id,
            receipt_id,
            period_s,
            code.barcode.raw,
            kind,
            amount,
            due,
            page,
            at,
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// The live code for one rule in one month, whatever the file's state.
pub async fn for_period(
    pool: &SqlitePool,
    rule_id: i64,
    period: Period,
) -> sqlx::Result<Option<BillCode>> {
    let p = period.to_string();
    let row = sqlx::query!(
        r#"SELECT receipt_id AS "receipt_id!: i64", barcode_raw AS "barcode_raw!: String",
                  kind AS "kind!: String", amount_cents AS "amount_cents: i64",
                  due_date AS "due_date: String", page AS "page!: i64"
           FROM rule_bill_code
           WHERE rule_id = ?1 AND period = ?2 AND retired_at IS NULL"#,
        rule_id,
        p
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| BillCode {
        rule_id,
        receipt_id: r.receipt_id,
        period,
        barcode_raw: r.barcode_raw,
        kind: r.kind,
        amount: r.amount_cents.map(Cents),
        due: r
            .due_date
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        page: r.page.max(1) as u32,
    }))
}

/// The shared file a month's code points at, when that file is still live:
/// its stored path and extension.
pub async fn shared_file(
    pool: &SqlitePool,
    code: &BillCode,
) -> sqlx::Result<Option<(String, String)>> {
    let row = sqlx::query!(
        r#"SELECT path AS "path!: String", ext AS "ext!: String"
           FROM receipts WHERE id = ?1 AND deleted_at IS NULL"#,
        code.receipt_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.path, r.ext)))
}

/// Each month's amount in a rule's live codes — what the row's Progress line
/// counts for a month that has no row yet (`db::ledger`). One query per rule.
pub async fn amounts(
    pool: &SqlitePool,
    rule_id: i64,
) -> sqlx::Result<std::collections::HashMap<Period, Cents>> {
    let rows = sqlx::query!(
        r#"SELECT period AS "period!: String", amount_cents AS "amount_cents!: i64"
           FROM rule_bill_code
           WHERE rule_id = ?1 AND retired_at IS NULL AND amount_cents IS NOT NULL"#,
        rule_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| Some((Period::parse(&r.period)?, Cents(r.amount_cents))))
        .collect())
}

/// What a rule's live codes cover, for the rule pane's read-back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub count: i64,
    pub first: Period,
    pub last: Period,
    /// The installment's value, when every month's is the same.
    pub amount: Option<Cents>,
}

pub async fn summary(pool: &SqlitePool, rule_id: i64) -> sqlx::Result<Option<Summary>> {
    let row = sqlx::query!(
        r#"SELECT count(*) AS "n!: i64", min(period) AS "first: String",
                  max(period) AS "last: String",
                  CASE WHEN count(DISTINCT amount_cents) = 1 THEN max(amount_cents) END
                      AS "amount: i64"
           FROM rule_bill_code WHERE rule_id = ?1 AND retired_at IS NULL"#,
        rule_id
    )
    .fetch_one(pool)
    .await?;
    let (Some(first), Some(last)) = (
        row.first.as_deref().and_then(Period::parse),
        row.last.as_deref().and_then(Period::parse),
    ) else {
        return Ok(None);
    };
    Ok((row.n > 0).then_some(Summary {
        count: row.n,
        first,
        last,
        amount: row.amount.map(Cents),
    }))
}

/// Puts a month's code on its row: the amount, the due date and the code.
///
/// The same policy a single bill follows (`web::import::attach_to`): the
/// amount follows the bill whoever set the row's figure, a due date a person
/// typed with `e` is kept, and the change is one `import` entry carrying both
/// fields, so `u` puts them back. The code itself is left off a row when
/// another live row already holds it — the unique index would refuse it, and
/// the amount and date are what the row needs; they are then sourced `text`.
/// Returns whether anything changed.
pub async fn stamp(
    pool: &SqlitePool,
    actor: Actor,
    tx_id: i64,
    code: &BillCode,
) -> sqlx::Result<bool> {
    let row = sqlx::query!(
        r#"SELECT amount_cents AS "amount_cents: i64", due_date AS "due_date: String",
                  due_date_source AS "due_date_source: String",
                  barcode_raw AS "barcode_raw: String", rule_id AS "rule_id: i64",
                  period AS "period!: String"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        tx_id
    )
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(false) };

    let old_amount = row.amount_cents.map(Cents);
    let old_due = row
        .due_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    let typed_due = row.due_date_source.as_deref() == Some("user");
    let new_amount = code.amount.or(old_amount);
    let new_due = if typed_due {
        old_due
    } else {
        code.due.or(old_due)
    };

    let taken: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM transactions
                          WHERE barcode_raw = ?1 AND id <> ?2 AND deleted_at IS NULL)
             AS "taken!: bool""#,
        code.barcode_raw,
        tx_id
    )
    .fetch_one(pool)
    .await?;
    let barcode = (!taken).then(|| code.barcode_raw.clone());
    let same_code = barcode.is_none() || row.barcode_raw == barcode;
    if new_amount == old_amount && new_due == old_due && same_code {
        return Ok(false);
    }

    let mut tx = pool.begin().await?;
    let amount_v = new_amount.map(|c| c.get());
    let due_v = new_due.map(|d| d.format("%Y-%m-%d").to_string());
    // Read off the barcode only while the row carries one: the schema refuses
    // a `barcode` source on a row with no code. When the code is held by
    // another live row (the same file on an older rule), the figures still
    // came off this bill — printed in its text, where the code is too.
    let has_code = barcode.is_some() || row.barcode_raw.is_some();
    let due_source = (!typed_due && code.due.is_some()).then_some(match code.kind.as_str() {
        _ if !has_code => "text",
        "cobranca" => "fator",
        _ => "free_field",
    });
    let amount_source = code
        .amount
        .is_some()
        .then_some(if has_code { "barcode" } else { "text" });
    sqlx::query!(
        r#"UPDATE transactions SET
               amount_cents = ?2, due_date = ?3,
               barcode_raw = coalesce(?4, barcode_raw),
               barcode_kind = CASE WHEN ?4 IS NULL THEN barcode_kind ELSE ?5 END,
               amount_source = coalesce(?6, amount_source),
               due_date_source = coalesce(?7, due_date_source),
               version = version + 1
           WHERE id = ?1"#,
        tx_id,
        amount_v,
        due_v,
        barcode,
        code.kind,
        amount_source,
        due_source,
    )
    .execute(&mut *tx)
    .await?;
    let entry = Change::transaction(actor, TxAction::Import, tx_id)
        .from_rule(row.rule_id)
        .in_period(Period::parse(&row.period).unwrap_or(code.period))
        .diff("amount", Value::Money(old_amount), Value::Money(new_amount))
        .diff("due_date", Value::Date(old_due), Value::Date(new_due));
    if !entry.is_empty() {
        entry.write(&mut tx).await?;
    }
    tx.commit().await?;
    Ok(true)
}
