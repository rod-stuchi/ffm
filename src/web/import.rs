//! Attaching a bill to a rule, and reading what it says.
//!
//! **The only place a barcode file is ever attached is the rule's own
//! editor.** An earlier design had a panel drop parse the file and infer which
//! rule it belonged to — by issuer and period and value proximity, or by
//! matching the dropped filename against a row's title. Both were rejected as
//! too fragile: a real bill's filename, or its content, should not have to
//! line up with what is already in the ledger for the household to trust the
//! result. Here the rule is already identified, so there is no matching left
//! to get wrong.
//!
//! What the parse then writes is deliberately conservative. It **fills what is
//! empty and flags what disagrees** — it never overwrites a number a person
//! typed, because the one thing worse than failing to import is silently
//! replacing a right answer with a wrong one.

use crate::db::changes::{Actor, Change, FileAction, Value};
use crate::db::rules;
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::receipt_path::Kind as FileKind;
use crate::services::boleto::Amount;
use crate::services::import::{self, ImportError};
use crate::services::issuers::Profiles;
use crate::services::receipts;
use crate::state::AppState;
use crate::ui::fmt;
use crate::web::session::Auth;
use axum::extract::{Multipart, Path, Query, State};
use axum::response::Response;
use chrono::{Local, NaiveDate};
use serde::Deserialize;

const MAX_BYTES: usize = 20 * 1024 * 1024;

#[derive(Deserialize)]
pub struct Params {
    /// The month the ledger is showing — a barcode file belongs to a period,
    /// and the pane knows which one is on screen.
    #[serde(default)]
    period: String,
}

/// `POST /rules/{id}/barcode` — the dropzone in `E`.
pub async fn attach(
    State(st): State<AppState>,
    auth: Auth,
    Path(rule_id): Path<i64>,
    Query(p): Query<Params>,
    mut form: Multipart,
) -> Response {
    let t = auth.catalog();
    let say = |m: &str, bad: bool| crate::web::tx::say_only(m, bad);

    let Ok(Some(rule)) = rules::get(&st.pool, rule_id).await else {
        return say(t.error_not_written(), true);
    };
    if !auth.viewer.sees_private_of(rule.member_id) {
        return say(t.error_not_written(), true);
    }

    let mut filename = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    while let Ok(Some(field)) = form.next_field().await {
        if field.name() != Some("file") {
            continue;
        }
        filename = field.file_name().unwrap_or_default().to_string();
        match field.bytes().await {
            Ok(b) if b.len() <= MAX_BYTES => bytes = b.to_vec(),
            Ok(_) => return say(t.file_too_big(), true),
            Err(_) => return say(t.error_not_written(), true),
        }
    }
    if bytes.is_empty() {
        return say(t.error_not_written(), true);
    }

    // The occurrence this bill is for. A barcode file is per period, and the
    // period is the one the ledger is showing.
    let period = Period::parse(&p.period).unwrap_or_else(|| Period::of(Local::now().date_naive()));
    let period_s = period.to_string();
    let tx_id: Option<i64> = sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM transactions
           WHERE rule_id = ?1 AND period = ?2 AND deleted_at IS NULL"#,
        rule_id,
        period_s
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten();

    let Some(tx_id) = tx_id else {
        return say(t.import_no_occurrence(), true);
    };

    // pdftotext takes a path, so the upload lands in a scratch file first.
    // Not the vault: nothing goes in there until it has actually parsed.
    let scratch = std::env::temp_dir().join(format!("ffm-import-{tx_id}-{filename}"));
    if std::fs::write(&scratch, &bytes).is_err() {
        return say(t.error_not_written(), true);
    }
    let profiles = Profiles::load(&st.cfg.issuers_path()).unwrap_or_default();
    let parsed = import::import(&scratch, &profiles, Local::now().date_naive());
    let _ = std::fs::remove_file(&scratch);

    let parsed = match parsed {
        Ok(p) => p,
        // A misread is reported, never worked around: a number that failed its
        // own checksum is not a number to fall back from.
        Err(e @ (ImportError::Misread(_) | ImportError::NoBarcode | ImportError::NoText)) => {
            return say(&e.to_string(), true);
        }
        Err(e) => return say(&e.to_string(), true),
    };

    // The file itself is kept, under the deterministic name every other
    // receipt uses.
    let Some((slot, _, member_id)) = crate::web::files::slot_for(&st, &auth, tx_id).await else {
        return say(t.file_not_tracked(), true);
    };
    if receipts::store(
        &st.pool,
        &st.cfg.receipts_root,
        Actor::User(auth.viewer.member_id),
        &slot,
        FileKind::Barcode,
        &filename,
        &bytes,
    )
    .await
    .is_err()
    {
        return say(t.error_not_written(), true);
    }

    let mut notes: Vec<String> = Vec::new();
    let currency = crate::db::ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();

    // --- what the barcode said about this occurrence --------------------
    let row = sqlx::query!(
        r#"SELECT amount_cents AS "amount_cents: i64", due_date AS "due_date: String"
           FROM transactions WHERE id = ?1"#,
        tx_id
    )
    .fetch_one(&st.pool)
    .await
    .ok();

    let existing_amount = row.as_ref().and_then(|r| r.amount_cents).map(Cents);
    let existing_due = row
        .as_ref()
        .and_then(|r| r.due_date.as_deref())
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());

    let barcode_amount = parsed.import.amount;
    let barcode_due = parsed.import.due;

    // Fill what is empty; flag what disagrees; never overwrite. The policy
    // itself lives in `services::import` so it can be pinned by a test rather
    // than re-derived in each handler that imports something.
    use crate::services::import::{Fill, reconcile};

    let amount_fill = reconcile(barcode_amount, existing_amount);
    if let Fill::Differs { found, kept } = amount_fill {
        notes.push(
            t.import_amount_differs(&fmt::money(found, currency), &fmt::money(kept, currency)),
        );
    }
    let due_fill = reconcile(barcode_due, existing_due);
    if let Fill::Differs { found, kept } = due_fill {
        notes.push(t.import_date_differs(&fmt::date(found), &fmt::date(kept)));
    }

    let write_amount = amount_fill.to_write();
    let write_due = due_fill.to_write();

    let Ok(mut tx) = st.pool.begin().await else {
        return say(t.error_not_written(), true);
    };

    let raw = parsed.barcode.raw.clone();
    let kind = parsed.barcode.kind.code();
    let amount_source = parsed.import.amount_source;
    let due_source = parsed.import.due_source;
    let amount_value = write_amount.map(|c| c.get());
    let due_value = write_due.map(|d| d.format("%Y-%m-%d").to_string());
    let issuer = parsed.import.issuer.clone();

    // `barcode_raw` is the natural idempotency key, with a unique index behind
    // it: the same bill cannot end up on two rows. Hitting that index is not a
    // failure — it means this exact bill is already filed — so it gets its own
    // message rather than the generic one.
    let already: Option<String> = sqlx::query_scalar!(
        r#"SELECT period AS "p!: String" FROM transactions
           WHERE barcode_raw = ?1 AND id <> ?2 AND deleted_at IS NULL"#,
        raw,
        tx_id
    )
    .fetch_optional(&mut *tx)
    .await
    .ok()
    .flatten();

    if let Some(period) = already {
        let label = Period::parse(&period).map(fmt::period).unwrap_or(period);
        return say(&t.import_already_attached(&label), true);
    }

    let ok = sqlx::query!(
        r#"UPDATE transactions SET
               barcode_raw = ?2, barcode_kind = ?3, issuer_id = ?4,
               amount_cents = coalesce(?5, amount_cents),
               due_date = coalesce(?6, due_date),
               amount_source = coalesce(?7, amount_source),
               due_date_source = coalesce(?8, due_date_source)
           WHERE id = ?1"#,
        tx_id,
        raw,
        kind,
        issuer,
        amount_value,
        due_value,
        amount_source,
        due_source,
    )
    .execute(&mut *tx)
    .await
    .is_ok();

    if !ok {
        return say(t.error_not_written(), true);
    }

    // --- the client code, which lives on the RULE -----------------------
    // The same account every period, unlike the value and the due date, which
    // are genuinely per-period.
    if let Some(found) = parsed.import.client_code.as_deref() {
        match rule.client_code.as_deref() {
            None => {
                let _ = sqlx::query!(
                    "UPDATE recurring_rules SET client_code = ?2 WHERE id = ?1",
                    rule_id,
                    found
                )
                .execute(&mut *tx)
                .await;
            }
            // Flagged, not silently overwritten — the same "flag it, do not
            // guess" posture as the multi-page barcode window.
            Some(known) if known != found => {
                notes.push(t.import_code_differs(found, known));
            }
            _ => {}
        }
    }

    let entry = Change::file(
        Actor::User(auth.viewer.member_id),
        FileAction::Attach,
        tx_id,
    )
    .from_rule(Some(rule_id))
    .in_period(period)
    .diff("barcode", Value::Text(None), Value::text(&raw))
    .diff(
        "amount",
        Value::Money(existing_amount),
        Value::Money(write_amount.or(existing_amount)),
    )
    .diff(
        "client_code",
        Value::Text(rule.client_code.clone()),
        Value::Text(
            parsed
                .import
                .client_code
                .clone()
                .or(rule.client_code.clone()),
        ),
    );
    let _ = entry.write(&mut tx).await;
    let _ = tx.commit().await;

    // What actually happened, in one line.
    let headline = match (barcode_amount, barcode_due) {
        (Some(a), Some(d)) => t.import_filled(&fmt::money(a, currency), &fmt::date(d)),
        _ => {
            let what = parsed.import.unresolved.join(", ");
            if what.is_empty() {
                t.file_added().to_string()
            } else {
                t.import_unresolved(&what)
            }
        }
    };
    let message = if notes.is_empty() {
        headline
    } else {
        format!("{headline} · {}", notes.join(" · "))
    };

    // The row is re-read through the pane query, so what comes back was
    // decided by the database and the disk rather than by this handler.
    crate::web::tx::row_fragment(&st, &auth, tx_id, member_id, period, &message).await
}

/// Clears just the client code — a narrow correction for "the wrong file got
/// attached the first time", distinct from `u`'s full revert of the last
/// change. It touches nothing else about the rule.
pub async fn unlink(State(st): State<AppState>, auth: Auth, Path(rule_id): Path<i64>) -> Response {
    let t = auth.catalog();
    let Ok(Some(rule)) = rules::get(&st.pool, rule_id).await else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    if !auth.viewer.sees_private_of(rule.member_id) {
        return crate::web::tx::say_only(t.error_not_written(), true);
    }

    let Ok(mut tx) = st.pool.begin().await else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    let _ = sqlx::query!(
        "UPDATE recurring_rules SET client_code = NULL WHERE id = ?1",
        rule_id
    )
    .execute(&mut *tx)
    .await;
    let _ = Change::rule(
        Actor::User(auth.viewer.member_id),
        crate::db::changes::RuleAction::Edit,
        rule_id,
    )
    .diff(
        "client_code",
        Value::Text(rule.client_code),
        Value::Text(None),
    )
    .write(&mut tx)
    .await;
    let _ = tx.commit().await;

    crate::web::tx::say_only(t.import_unlinked(), false)
}

/// Amount and due date as the import sees them, for the resolver.
impl crate::services::issuers::Import {
    pub fn describe(&self) -> String {
        match (self.amount, self.due) {
            (Some(_), Some(_)) => "amount and due date".into(),
            (Some(_), None) => "amount".into(),
            (None, Some(_)) => "due date".into(),
            _ => "nothing".into(),
        }
    }
}

/// `Amount` is a sum type on purpose; the resolver already flattened it to
/// `Option<Cents>`, and this is the reminder of why that flattening is only
/// safe there — `Reference` must never become money.
#[allow(dead_code)]
fn never_coerce(a: Amount) -> Option<Cents> {
    match a {
        Amount::Fixed(c) => Some(c),
        Amount::Open | Amount::Reference(_) => None,
    }
}
