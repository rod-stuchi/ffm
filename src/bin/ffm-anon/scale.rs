//! A bigger household than the real one: `--years` adds history back in
//! time, `--scale` adds members. Both run after the shuffle, so every copy is
//! made of already-fake data, and both copy only what is live — a cloned row
//! has no change log behind it, and a deleted row with no log would be a row
//! the trash and `u` know nothing about.

use crate::barcode::Target;
use crate::ids;
use crate::money::Scope;
use crate::names::Pool;
use crate::shuffle::{Shuffler, reference_of};
use crate::sqlrow::{V, clone_by, clone_row};
use chrono::{Months, NaiveDate};
use ffm::domain::slug::slugify;
use ffm::services::boleto;
use sqlx::SqliteConnection;
use std::collections::HashMap;

struct Row {
    id: i64,
    rule: Option<i64>,
    period: String,
    title: String,
    amount: Option<i64>,
    barcode: Option<String>,
    date: Option<String>,
    due: Option<String>,
}

async fn live_rows(conn: &mut SqliteConnection, member: Option<i64>) -> anyhow::Result<Vec<Row>> {
    type R = (
        i64,
        Option<i64>,
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<R> = sqlx::query_as(
        "SELECT id, rule_id, period, title, amount_cents, barcode_raw, date, due_date
         FROM transactions WHERE deleted_at IS NULL AND (?1 IS NULL OR member_id = ?1) ORDER BY id",
    )
    .bind(member)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Row {
            id: r.0,
            rule: r.1,
            period: r.2,
            title: r.3,
            amount: r.4,
            barcode: r.5,
            date: r.6,
            due: r.7,
        })
        .collect())
}

fn scope(r: &Row) -> Scope {
    match r.rule {
        Some(id) => Scope::Rule(id),
        None => Scope::Title(r.title.trim().to_lowercase()),
    }
}

fn back(date: &str, months: u32) -> Option<String> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some(
        d.checked_sub_months(Months::new(months))?
            .format("%Y-%m-%d")
            .to_string(),
    )
}

fn period_back(period: &str, months: u32) -> Option<String> {
    let d = NaiveDate::parse_from_str(&format!("{period}-01"), "%Y-%m-%d").ok()?;
    Some(
        d.checked_sub_months(Months::new(months))?
            .format("%Y-%m")
            .to_string(),
    )
}

/// A row's code, rebuilt for a copy: the copy's amount, the copy's due date.
/// An account's own slip is the same every month and stays as it is.
fn copy_barcode(
    s: &mut Shuffler,
    raw: &str,
    reference: NaiveDate,
    cents: Option<i64>,
    due: Option<NaiveDate>,
) -> anyhow::Result<String> {
    if boleto::names_an_account(boleto::Kind::Cobranca, raw) {
        return Ok(raw.to_string());
    }
    let parsed = boleto::parse(raw, reference).map_err(|e| anyhow::anyhow!("{e}: {raw}"))?;
    let embedded = match parsed.amount {
        boleto::Amount::Fixed(_) => cents,
        _ => None,
    };
    s.fresh_barcode(
        raw,
        reference,
        &Target {
            cents: embedded,
            due,
            company: None,
        },
    )
}

/// The receipts of `from`, copied onto `to`, with the path moved by `path`.
async fn copy_receipts(
    conn: &mut SqliteConnection,
    s: &Shuffler,
    from: &Row,
    to: i64,
    uploader: Option<i64>,
    amount: Option<i64>,
    path: impl Fn(&str) -> String,
) -> anyhow::Result<HashMap<i64, i64>> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, path FROM receipts WHERE transaction_id = ?1 AND deleted_at IS NULL ORDER BY id")
            .bind(from.id)
            .fetch_all(&mut *conn)
            .await?;
    let mut ids = HashMap::new();
    for (id, p) in rows {
        let mut new_path = path(&p);
        // A receipt's amount suffix follows the copy's amount.
        if let (Some(old), Some(new)) = (from.amount, amount)
            && let Some((dir, name)) = new_path.rsplit_once('/')
        {
            let name = crate::paths::filename(name, s.currency, |c| {
                if c == old.abs() { new.abs() } else { c }
            });
            new_path = format!("{dir}/{name}");
        }
        let mut set = vec![
            ("transaction_id", V::I(to)),
            ("path", V::T(new_path)),
            ("deleted_from_path", V::N),
        ];
        if let Some(u) = uploader {
            set.push(("uploaded_by", V::I(u)));
        }
        ids.insert(id, clone_row(conn, "receipts", id, set).await?);
    }
    Ok(ids)
}

/// The live bill codes of a rule in a period, onto `rule` in `period`.
async fn copy_codes(
    conn: &mut SqliteConnection,
    s: &mut Shuffler,
    from_rule: i64,
    from_period: &str,
    rule: i64,
    period: &str,
    months_back: u32,
    salt: u32,
    receipt_ids: &HashMap<i64, i64>,
) -> anyhow::Result<()> {
    let codes: Vec<(i64, String, Option<i64>, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT id, barcode_raw, amount_cents, due_date, receipt_id FROM rule_bill_code
         WHERE rule_id = ?1 AND period = ?2 AND retired_at IS NULL",
    )
    .bind(from_rule)
    .bind(from_period)
    .fetch_all(&mut *conn)
    .await?;
    let taken: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM rule_bill_code WHERE rule_id = ?1 AND period = ?2 AND retired_at IS NULL)",
    )
    .bind(rule)
    .bind(period)
    .fetch_one(&mut *conn)
    .await?;
    if taken {
        return Ok(());
    }
    for (id, raw, amount, due, receipt) in codes {
        let amount = amount.map(|c| s.money.map(salt, &Scope::Rule(from_rule), c));
        let due = due.and_then(|d| {
            if months_back > 0 {
                back(&d, months_back)
            } else {
                Some(d)
            }
        });
        let moved = due
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
        let raw = copy_barcode(
            s,
            &raw,
            reference_of(None, from_period),
            amount,
            moved.filter(|_| months_back > 0),
        )?;
        let mut set = vec![
            ("rule_id", V::I(rule)),
            ("period", V::T(period.to_string())),
            ("barcode_raw", V::T(raw)),
            ("amount_cents", amount.into()),
            ("due_date", due.into()),
        ];
        if let Some(r) = receipt.and_then(|r| receipt_ids.get(&r)) {
            set.push(("receipt_id", V::I(*r)));
        }
        clone_row(conn, "rule_bill_code", id, set).await?;
    }
    Ok(())
}

/// `--years K`: every live row copied 12, 24 … 12·K months back. A loan or a
/// canceled bill — a rule with an end — ran once, and is left alone.
pub async fn years(conn: &mut SqliteConnection, s: &mut Shuffler, k: u32) -> anyhow::Result<()> {
    if k == 0 {
        return Ok(());
    }
    let finite: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM recurring_rules WHERE total_installments IS NOT NULL OR end_date IS NOT NULL OR deleted_at IS NOT NULL",
    )
    .fetch_all(&mut *conn)
    .await?;
    let rows = live_rows(conn, None).await?;
    for year in 1..=k {
        let months = 12 * year;
        let mut receipt_ids = HashMap::new();
        let mut done = Vec::new();
        for r in rows
            .iter()
            .filter(|r| r.rule.is_none_or(|id| !finite.contains(&id)))
        {
            let Some(period) = period_back(&r.period, months) else {
                continue;
            };
            if let Some(rule) = r.rule {
                let taken: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM transactions WHERE rule_id = ?1 AND period = ?2)",
                )
                .bind(rule)
                .bind(&period)
                .fetch_one(&mut *conn)
                .await?;
                if taken {
                    continue;
                }
            }
            let amount = r.amount.map(|c| s.money.map(1000 + year, &scope(r), c));
            let due = r.due.as_deref().and_then(|d| back(d, months));
            let barcode = match &r.barcode {
                Some(raw) => {
                    let due_date = due
                        .as_deref()
                        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
                    Some(copy_barcode(
                        s,
                        raw,
                        reference_of(r.due.as_deref(), &r.period),
                        amount,
                        due_date,
                    )?)
                }
                None => None,
            };
            let id = clone_row(
                conn,
                "transactions",
                r.id,
                vec![
                    ("period", V::T(period.clone())),
                    (
                        "date",
                        r.date.as_deref().and_then(|d| back(d, months)).into(),
                    ),
                    ("due_date", due.into()),
                    ("amount_cents", amount.into()),
                    ("barcode_raw", barcode.into()),
                    ("version", V::I(1)),
                ],
            )
            .await?;
            let shift = |p: &str| shift_year(p, &r.period, &period);
            receipt_ids.extend(copy_receipts(conn, s, r, id, None, amount, shift).await?);
            done.push((r, period));
        }
        for (r, period) in done {
            if let Some(rule) = r.rule {
                copy_codes(
                    conn,
                    s,
                    rule,
                    &r.period,
                    rule,
                    &period,
                    months,
                    1000 + year,
                    &receipt_ids,
                )
                .await?;
            }
        }
    }
    sqlx::query(&format!(
        "UPDATE recurring_rules SET start_date = date(start_date, '-{k} years')
         WHERE total_installments IS NULL AND end_date IS NULL AND deleted_at IS NULL"
    ))
    .execute(&mut *conn)
    .await?;
    println!("· {k} more year(s) of history");
    Ok(())
}

/// The year folder of `path` moved from `from`'s year to `to`'s.
fn shift_year(path: &str, from: &str, to: &str) -> String {
    let (fy, ty) = (&from[..4], &to[..4]);
    path.split('/')
        .map(|seg| if seg == fy { ty } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}

/// `--scale N`: N−1 more members, each a copy of every real one, with a name,
/// amounts, barcodes and vault folder of their own.
pub async fn members(
    conn: &mut SqliteConnection,
    s: &mut Shuffler,
    n: u32,
    password: &str,
) -> anyhow::Result<()> {
    if n <= 1 {
        return Ok(());
    }
    let hash = ffm::db::auth::hash_password(password)?;
    let sources: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM members WHERE deleted_at IS NULL ORDER BY id")
            .fetch_all(&mut *conn)
            .await?;
    let mut made = 0;
    for copy in 1..n {
        for source in &sources {
            let name = ffm::db::auth::normal_name(
                &s.names
                    .part(&format!("clone {copy} of {source}"), Pool::Person),
            );
            let slug = slugify(&name);
            let member = sqlx::query(
                "INSERT INTO members (uuid, name, slug, password_hash, role, created_at) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
            )
            .bind(ids::uuid(&mut s.rng))
            .bind(&name)
            .bind(&slug)
            .bind(&hash)
            .bind(ffm::clock::stamp())
            .execute(&mut *conn)
            .await?
            .last_insert_rowid();
            let has_settings: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM member_settings WHERE member_id = ?1)",
            )
            .bind(source)
            .fetch_one(&mut *conn)
            .await?;
            if has_settings {
                clone_by(
                    conn,
                    "member_settings",
                    "member_id",
                    *source,
                    vec![("member_id", V::I(member))],
                )
                .await?;
            }

            let rules: Vec<i64> = sqlx::query_scalar(
                "SELECT id FROM recurring_rules WHERE member_id = ?1 AND deleted_at IS NULL",
            )
            .bind(source)
            .fetch_all(&mut *conn)
            .await?;
            let mut rule_ids = HashMap::new();
            for id in rules {
                let amount: Option<i64> =
                    sqlx::query_scalar("SELECT amount_cents FROM recurring_rules WHERE id = ?1")
                        .bind(id)
                        .fetch_one(&mut *conn)
                        .await?;
                let amount = amount.map(|c| s.money.map(copy, &Scope::Rule(id), c));
                let new = clone_row(
                    conn,
                    "recurring_rules",
                    id,
                    vec![("member_id", V::I(member)), ("amount_cents", amount.into())],
                )
                .await?;
                rule_ids.insert(id, new);
                let recipe: Option<i64> = sqlx::query_scalar(
                    "SELECT id FROM rule_recipe WHERE rule_id = ?1 AND retired_at IS NULL",
                )
                .bind(id)
                .fetch_optional(&mut *conn)
                .await?;
                if let Some(r) = recipe {
                    clone_row(
                        conn,
                        "rule_recipe",
                        r,
                        vec![("rule_id", V::I(new)), ("created_by", V::I(member))],
                    )
                    .await?;
                }
            }

            let rows = live_rows(conn, Some(*source)).await?;
            let mut receipt_ids = HashMap::new();
            let mut done = Vec::new();
            for r in rows
                .iter()
                .filter(|r| r.rule.is_none_or(|id| rule_ids.contains_key(&id)))
            {
                let amount = r.amount.map(|c| s.money.map(copy, &scope(r), c));
                let barcode = match &r.barcode {
                    Some(raw) => Some(copy_barcode(
                        s,
                        raw,
                        reference_of(r.due.as_deref(), &r.period),
                        amount,
                        None,
                    )?),
                    None => None,
                };
                let id = clone_row(
                    conn,
                    "transactions",
                    r.id,
                    vec![
                        ("member_id", V::I(member)),
                        ("rule_id", r.rule.map(|id| rule_ids[&id]).into()),
                        ("amount_cents", amount.into()),
                        ("barcode_raw", barcode.into()),
                        ("created_by", V::I(member)),
                        ("version", V::I(1)),
                    ],
                )
                .await?;
                let retarget = |p: &str| match p.split_once('/') {
                    Some((_, rest)) => format!("{slug}/{rest}"),
                    None => p.to_string(),
                };
                receipt_ids
                    .extend(copy_receipts(conn, s, r, id, Some(member), amount, retarget).await?);
                done.push(r);
            }
            for r in done {
                if let Some(rule) = r.rule {
                    copy_codes(
                        conn,
                        s,
                        rule,
                        &r.period,
                        rule_ids[&rule],
                        &r.period,
                        0,
                        copy,
                        &receipt_ids,
                    )
                    .await?;
                }
            }
            made += 1;
        }
    }
    println!("· {made} more member(s)");
    Ok(())
}
