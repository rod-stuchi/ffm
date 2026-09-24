//! Uploading, opening and deleting a tracked occurrence's files.
//!
//! Scoped to **recurring-rule occurrences only**: a one-off has no taxonomy to
//! file itself under, so it gets no attachment feature at all.
//!
//! Three distinct actions rather than one upload plus a question — each kind
//! has its own fixed name on disk and its own effect on state, and asking
//! afterwards would be asking about something already decided.

use crate::db::changes::Actor;
use crate::db::ledger;
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::receipt_path::Kind;
use crate::services::receipts::{self, Slot};
use crate::state::AppState;
use crate::web::session::Auth;
use axum::extract::{Multipart, Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

/// At most 20 MB. A scanned bill is a few hundred kilobytes; this is the
/// bound that keeps a mistake from filling the vault's disk, not a target.
const MAX_BYTES: usize = 20 * 1024 * 1024;

/// Gathers everything the path depends on for one occurrence, and refuses
/// anything this viewer has no business touching.
pub(crate) async fn slot_for(
    st: &AppState,
    auth: &Auth,
    tx_id: i64,
) -> Option<(Slot, Period, i64)> {
    let row = sqlx::query!(
        r#"SELECT t.member_id AS "member_id!: i64", t.period AS "period!: String",
                  t.amount_cents AS "amount_cents: i64",
                  t.visibility AS "visibility!: String",
                  t.rule_id AS "rule_id: i64",
                  r.tracked AS "tracked?: i64",
                  r.bucket AS "bucket?: String", r.bill_name AS "bill_name?: String",
                  r.instance AS "instance?: String",
                  r.amount_in_filename AS "amount_in_filename?: i64",
                  m.slug AS "slug!: String"
           FROM transactions t
           JOIN members m ON m.id = t.member_id
           LEFT JOIN recurring_rules r ON r.id = t.rule_id
           WHERE t.id = ?1 AND t.deleted_at IS NULL"#,
        tx_id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()??;

    // A private row you cannot see is a file you cannot touch — otherwise its
    // absence from your ledger would be contradicted by its disappearance
    // from somebody else's.
    if row.visibility == "private" && !auth.viewer.sees_private_of(row.member_id) {
        return None;
    }
    // Untracked, or a one-off: nothing to file under.
    if row.tracked.unwrap_or(0) == 0 {
        return None;
    }

    let period = Period::parse(&row.period)?;
    let house = ledger::household(&st.pool).await.ok().flatten();
    let currency = house.map(|h| h.currency).unwrap_or_default();

    Some((
        Slot {
            transaction_id: tx_id,
            vault: crate::domain::receipt_path::vault(&row.slug),
            bucket: row.bucket?,
            bill_name: row.bill_name?,
            instance: row.instance,
            period,
            amount: row.amount_cents.map(|c| (Cents(c), currency)),
            amount_in_filename: row.amount_in_filename.unwrap_or(0) != 0,
        },
        period,
        row.member_id,
    ))
}

/// `POST /tx/{id}/file/{kind}` — add barcode / add scheduled / add receipt.
pub async fn upload(
    State(st): State<AppState>,
    auth: Auth,
    Path((tx_id, kind)): Path<(i64, String)>,
    mut form: Multipart,
) -> Response {
    let t = auth.catalog();
    let Some(kind) = Kind::parse(&kind) else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    let Some((slot, period, member_id)) = slot_for(&st, &auth, tx_id).await else {
        return crate::web::tx::say_only(t.file_not_tracked(), true);
    };

    let mut filename = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    while let Ok(Some(field)) = form.next_field().await {
        if field.name() != Some("file") {
            continue;
        }
        filename = field.file_name().unwrap_or_default().to_string();
        match field.bytes().await {
            Ok(b) if b.len() <= MAX_BYTES => bytes = b.to_vec(),
            Ok(_) => return crate::web::tx::say_only(t.file_too_big(), true),
            Err(_) => return crate::web::tx::say_only(t.error_not_written(), true),
        }
    }
    if bytes.is_empty() || filename.is_empty() {
        return crate::web::tx::say_only(t.error_not_written(), true);
    }

    // A bill is not just a file: it is checked against what the rule is bound
    // to, through the same attach the rule pane's dropzone uses — see
    // `import::attach_on_row`. Scheduled and receipt stay plain files.
    if kind == Kind::Barcode {
        let upload = crate::web::import::Upload { filename, bytes };
        return crate::web::import::attach_on_row(&st, &auth, tx_id, upload).await;
    }

    match receipts::store(
        &st.pool,
        &st.cfg.receipts_root,
        Actor::User(auth.viewer.member_id),
        &slot,
        kind,
        &filename,
        &bytes,
    )
    .await
    {
        // The row is re-read through the pane query, so the status mark it
        // comes back with was decided by the disk — not by the fact that an
        // upload just succeeded.
        Ok(_) => {
            crate::web::tx::row_fragment(&st, &auth, tx_id, member_id, period, t.file_added()).await
        }
        Err(e) => crate::web::tx::say_only(&e.to_string(), true),
    }
}

/// `POST /tx/{id}/file/{kind}/delete` — a soft delete: the file moves into a
/// sibling `.delete/`, never off disk.
pub async fn delete(
    State(st): State<AppState>,
    auth: Auth,
    Path((tx_id, kind)): Path<(i64, String)>,
) -> Response {
    let t = auth.catalog();
    let Some(kind) = Kind::parse(&kind) else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    let Some((slot, period, member_id)) = slot_for(&st, &auth, tx_id).await else {
        return crate::web::tx::say_only(t.file_not_tracked(), true);
    };

    match receipts::soft_delete(
        &st.pool,
        &st.cfg.receipts_root,
        Actor::User(auth.viewer.member_id),
        &slot,
        kind,
        "manual",
        None,
    )
    .await
    {
        Ok(true) => {
            crate::web::tx::row_fragment(&st, &auth, tx_id, member_id, period, t.file_removed())
                .await
        }
        Ok(false) => crate::web::tx::say_only(t.file_none_receipt(), true),
        Err(e) => crate::web::tx::say_only(&e.to_string(), true),
    }
}

/// `GET /tx/{id}/file/{kind}` — the file itself, in a new browser tab.
///
/// All four allowed extensions render natively, which is exactly why no custom
/// renderer is built: this hands the browser the bytes and gets out of the way.
pub async fn open(
    State(st): State<AppState>,
    auth: Auth,
    Path((tx_id, kind)): Path<(i64, String)>,
) -> Response {
    let Some(kind) = Kind::parse(&kind) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((slot, _, _)) = slot_for(&st, &auth, tx_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let Ok(present) = receipts::presence(&st.pool, &st.cfg.receipts_root, &slot).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let ext = match kind {
        Kind::Barcode => present.bill,
        Kind::Scheduled => present.scheduled,
        Kind::Receipt => present.receipt,
    };
    let Some(ext) = ext else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // The path is computed from the same builder the store writes with, so
    // there is no user-supplied path anywhere in this handler and nothing to
    // traverse out of.
    let rel = crate::domain::receipt_path::file(
        &crate::domain::receipt_path::Taxonomy {
            vault: &slot.vault,
            bucket: &slot.bucket,
            bill_name: &slot.bill_name,
            instance: slot.instance.as_deref(),
        },
        slot.period,
        kind,
        &ext,
        slot.amount_in_filename.then_some(slot.amount).flatten(),
    );

    match tokio::fs::read(st.cfg.receipts_root.join(&rel)).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, content_type(&ext)),
                // `inline`, so the browser renders it rather than downloading
                // it — the whole point of keeping the extension set closed.
                (header::CONTENT_DISPOSITION, "inline"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn content_type(ext: &str) -> &'static str {
    match ext {
        "pdf" => "application/pdf",
        "jpg" => "image/jpeg",
        "png" => "image/png",
        _ => "text/plain; charset=utf-8",
    }
}
