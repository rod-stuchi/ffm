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
//! empty, replaces what was only a seed, and flags what a person typed** — a
//! row's amount copied in by the materializer is a placeholder waiting for
//! this bill, but a number somebody set with `e` is never silently replaced,
//! because the one thing worse than failing to import is replacing a right
//! answer with a wrong one.

use crate::db::changes::{Actor, Change, FileAction, TxAction, Value};
use crate::db::rules;
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::receipt_path::Kind as FileKind;
use crate::services::boleto::Amount;
use crate::services::import::{self, Binding, Ident, Parsed, binding_check};
use crate::services::issuers::Profiles;
use crate::services::pending;
use crate::services::receipts;
use crate::state::AppState;
use crate::ui::fmt;
use crate::ui::i18n::Catalog;
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::response::IntoResponse;
use axum::response::Response;
use chrono::NaiveDate;
use serde::Deserialize;

const MAX_BYTES: usize = 20 * 1024 * 1024;

#[derive(Deserialize)]
pub struct Params {
    /// The month the ledger is showing — a barcode file belongs to a period,
    /// and the pane knows which one is on screen.
    #[serde(default)]
    period: String,
}

/// A file as it arrived, before anything has been decided about it.
pub struct Upload {
    pub filename: String,
    pub bytes: Vec<u8>,
}

/// Why an attach did not happen — always a statusline message, and always
/// "nothing was written".
pub struct Refused(pub String);

/// Reads the `file` field of a multipart body — and `pdf_password`, which the
/// rule pane's dropzone sends with it: what the pane's field says right now,
/// saved or not, so typing the password and then dropping the statement works
/// in one go, before Create or Save.
async fn read_upload(
    t: &Catalog,
    form: &mut Multipart,
) -> Result<(Upload, Option<String>), Refused> {
    let mut filename = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut password = None;
    while let Ok(Some(field)) = form.next_field().await {
        match field.name() {
            Some("file") => {
                filename = field.file_name().unwrap_or_default().to_string();
                match field.bytes().await {
                    Ok(b) if b.len() <= MAX_BYTES => bytes = b.to_vec(),
                    Ok(_) => return Err(Refused(t.file_too_big().to_string())),
                    Err(_) => return Err(Refused(t.error_not_written().to_string())),
                }
            }
            Some("pdf_password") => {
                password = field
                    .text()
                    .await
                    .ok()
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty());
            }
            _ => {}
        }
    }
    if bytes.is_empty() {
        return Err(Refused(t.error_not_written().to_string()));
    }
    Ok((Upload { filename, bytes }, password))
}

/// Opens a password-protected PDF with the rule's password, before anything
/// reads, parks or stores it — the vault keeps the unlocked copy. Anything
/// that is not a locked PDF comes back exactly as it was.
///
/// Called on every way a file reaches a rule's row: the row's own slots, and
/// `A`'s and `E`'s dropzone. A file this password does not open is refused
/// here, so nothing downstream ever sees it.
pub fn unlock_for(t: &Catalog, upload: Upload, password: Option<&str>) -> Result<Upload, Refused> {
    use crate::services::pdflock::{self, LockError};
    match pdflock::unlock(&upload.filename, &upload.bytes, password) {
        Ok(std::borrow::Cow::Borrowed(_)) => Ok(upload),
        Ok(std::borrow::Cow::Owned(bytes)) => Ok(Upload {
            filename: upload.filename,
            bytes,
        }),
        Err(LockError::NeedsPassword) => Err(Refused(t.file_pdf_locked().to_string())),
        Err(LockError::WrongPassword) => Err(Refused(t.file_pdf_wrong_password().to_string())),
        Err(LockError::NoTool) => Err(Refused(t.file_pdf_no_tool().to_string())),
        // qpdf's own reason is for the log, not the statusline.
        Err(LockError::Tool(why)) => {
            tracing::info!("could not open {}: {why}", upload.filename);
            Err(Refused(t.file_pdf_unreadable().to_string()))
        }
    }
}

/// Reads a bill. Pure: nothing lands in the vault or the database here —
/// `pdftotext` takes a path, so the bytes visit a scratch file and leave.
pub fn parse(st: &AppState, upload: &Upload) -> Result<Parsed, Refused> {
    let mut raw = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut raw);
    let tag: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    // The extension is kept: `import` reads a `.txt` as text and hands
    // anything else to `pdftotext`, and it decides by the name.
    let ext = std::path::Path::new(&upload.filename)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("pdf");
    let scratch = std::env::temp_dir().join(format!("ffm-import-{tag}.{ext}"));
    if std::fs::write(&scratch, &upload.bytes).is_err() {
        return Err(Refused("could not read the file".into()));
    }
    let profiles = Profiles::load(&st.cfg.issuers_path()).unwrap_or_default();
    let parsed = import::import(&scratch, &profiles, crate::clock::today());
    let _ = std::fs::remove_file(&scratch);
    // A misread is reported, never worked around: a number that failed its
    // own checksum is not a number to fall back from.
    parsed.map_err(|e| Refused(e.to_string()))
}

/// What a bill is recognized by, as the binding check reads it.
fn read_binding(parsed: &Parsed) -> Binding<'_> {
    Binding {
        client_code: parsed.import.client_code.as_deref(),
        company: parsed.barcode.company.as_deref(),
        kind: Some(parsed.barcode.kind.code()),
        bank: parsed.barcode.bank.as_deref(),
    }
}

/// What a rule's bill is bound to, as the binding check reads it.
fn rule_binding(rule: &rules::Rule) -> Binding<'_> {
    Binding {
        client_code: rule.client_code.as_deref(),
        company: rule.barcode_company.as_deref(),
        kind: rule.barcode_kind.as_deref(),
        bank: None,
    }
}

/// One side of a wrong-row refusal, in words: "company 0162", "bank 077".
fn ident_label(t: &Catalog, id: &Ident) -> String {
    match id {
        Ident::Account(c) => t.import_id_account(c),
        Ident::Company(c) => t.import_id_company(c),
        Ident::Bank(c) => t.import_id_bank(c),
        Ident::Kind(k) => kind_label(t, Some(k)).unwrap_or_else(|| k.clone()),
    }
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
    if let Err(refused) = crate::web::rules::rule_access(&st, &auth, &rule).await {
        return refused;
    }
    // The pane's field wins over the saved password: it is what the person
    // is looking at, and the Save that would store it may not have happened.
    let upload = match read_upload(&t, &mut form)
        .await
        .and_then(|(u, typed)| unlock_for(&t, u, typed.as_deref().or(rule.pdf_password.as_deref())))
    {
        Ok(u) => u,
        Err(Refused(m)) => return say(&m, true),
    };
    let period = Period::parse(&p.period).unwrap_or_else(|| Period::of(crate::clock::today()));
    let Some(tx_id) = occurrence_of(&st, rule_id, period).await else {
        return say(t.import_no_occurrence(), true);
    };
    let parsed = match parse(&st, &upload) {
        Ok(p) => p,
        Err(Refused(m)) => return say(&m, true),
    };
    match attach_to(&st, &auth, &rule, tx_id, period, &upload, &parsed).await {
        Ok((member_id, message)) => {
            // The row is re-read through the pane query, so what comes back
            // was decided by the database and the disk rather than by this
            // handler.
            crate::web::tx::row_fragment(&st, &auth, tx_id, member_id, period, &message).await
        }
        Err(Refused(m)) => say(&m, true),
    }
}

/// The row's own slot 1 — the file rail's `+` menu, a drop on target `1`,
/// and `gu1` all land here via `POST /tx/{id}/file/barcode`.
///
/// **It is the same attach as the rule pane's, not a second one.** It used to
/// store whatever it was given: a credit-card statement replaced a rule's
/// bound utility bill without a word, and the row went on recording the old
/// barcode beside a file that no longer matched it. Now the bill is read and
/// checked against the rule's binding exactly as `E`'s dropzone does.
///
/// A file that cannot be read — a photo, a scan with no text layer — is
/// refused on a BOUND rule, because there is nothing to confirm it is that
/// rule's bill, and accepting it would let any file past the check. On a rule
/// with nothing bound it is filed as a plain document, which is all it ever
/// was before.
pub async fn attach_on_row(st: &AppState, auth: &Auth, tx_id: i64, upload: Upload) -> Response {
    let t = auth.catalog();
    let say = |m: &str, bad: bool| crate::web::tx::say_only(m, bad);

    let row = sqlx::query!(
        r#"SELECT rule_id AS "rule_id: i64", period AS "period!: String"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        tx_id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten();
    let Some((Some(rule_id), Some(period))) = row.map(|r| (r.rule_id, Period::parse(&r.period)))
    else {
        return say(t.file_not_tracked(), true);
    };
    let Ok(Some(rule)) = rules::get(&st.pool, rule_id).await else {
        return say(t.file_not_tracked(), true);
    };

    match parse(st, &upload) {
        Ok(parsed) => match attach_to(st, auth, &rule, tx_id, period, &upload, &parsed).await {
            Ok((member_id, message)) => {
                crate::web::tx::row_fragment(st, auth, tx_id, member_id, period, &message).await
            }
            Err(Refused(m)) => say(&m, true),
        },
        // The reader's own reason (pdftotext's stderr, usually) is for the
        // log, not the statusline.
        Err(Refused(why)) if rule_binding(&rule).is_bound() => {
            tracing::info!("unreadable bill for rule {}: {why}", rule.id);
            say(&t.import_unreadable_bound(&rule.title), true)
        }
        Err(_) => {
            let Some((slot, _, member_id)) = crate::web::files::slot_for(st, auth, tx_id).await
            else {
                return say(t.file_not_tracked(), true);
            };
            match receipts::store(
                &st.pool,
                &st.cfg.receipts_root,
                Actor::User(auth.viewer.member_id),
                &slot,
                FileKind::Barcode,
                &upload.filename,
                &upload.bytes,
            )
            .await
            {
                Ok(_) => {
                    crate::web::tx::row_fragment(st, auth, tx_id, member_id, period, t.file_added())
                        .await
                }
                Err(receipts::StoreError::Locked) => say(t.file_pdf_locked(), true),
                Err(e) => say(&e.to_string(), true),
            }
        }
    }
}

/// The live occurrence of a rule in one period, if it has been materialized.
pub async fn occurrence_of(st: &AppState, rule_id: i64, period: Period) -> Option<i64> {
    let period_s = period.to_string();
    sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM transactions
           WHERE rule_id = ?1 AND period = ?2 AND deleted_at IS NULL"#,
        rule_id,
        period_s
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten()
}

/// Attaches an already-parsed bill to one occurrence of a rule, and binds the
/// rule to it where nothing is bound yet.
///
/// **Every refusal happens before the first write.** A bill for another
/// account is the wrong row, and a bill already filed elsewhere is a
/// duplicate; both used to be discovered after the file was already in the
/// vault. Returns the owning member and the statusline message.
pub async fn attach_to(
    st: &AppState,
    auth: &Auth,
    rule: &rules::Rule,
    tx_id: i64,
    period: Period,
    upload: &Upload,
    parsed: &Parsed,
) -> Result<(i64, String), Refused> {
    let t = auth.catalog();
    let not_written = || Refused(t.error_not_written().to_string());

    // --- is this the right row? ---------------------------------------------
    if let Err(m) = binding_check(rule_binding(rule), read_binding(parsed)) {
        return Err(Refused(t.import_wrong_row(
            &ident_label(&t, &m.found),
            &rule.title,
            &ident_label(&t, &m.known),
        )));
    }

    // --- is it already filed somewhere? -------------------------------------
    // `barcode_raw` is the natural idempotency key, with a unique index behind
    // it: the same bill cannot end up on two rows.
    //
    // **A row holds a bill only while it holds the file.** One whose bill was
    // removed still carries the barcode, and used to refuse every other row
    // the same bill — "already on Moto CG", about a row showing no bill at
    // all. Such a row gives the bill up to this one (`takeover`, below),
    // provided the viewer may write to it.
    let raw = parsed.barcode.raw.clone();
    let already = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.period AS "period!: String",
                  t.title AS "title!: String", t.rule_id AS "rule_id: i64",
                  t.amount_cents AS "amount_cents: i64", t.member_id AS "member_id!: i64",
                  t.visibility AS "visibility!: String",
                  EXISTS (SELECT 1 FROM receipts r
                          WHERE r.transaction_id = t.id AND r.kind = 'barcode'
                            AND r.deleted_at IS NULL) AS "holds!: bool"
           FROM transactions t
           WHERE t.barcode_raw = ?1 AND t.id <> ?2 AND t.deleted_at IS NULL"#,
        raw,
        tx_id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten();
    let (refuse, takeover) = match already {
        Some(r) if r.holds || !auth.viewer.may_write(r.member_id) => (Some(r), None),
        other => (None, other),
    };
    if let Some(row) = refuse {
        let label = Period::parse(&row.period)
            .map(fmt::period)
            .unwrap_or(row.period);
        // Naming the row that holds it is what makes the refusal useful —
        // unless that row is private to somebody else, when its title and
        // amount are exactly what `private` hides. The month alone says no
        // more than the refusal itself already does.
        if row.visibility == "private" && !auth.viewer.sees_private_of(row.member_id) {
            return Err(Refused(t.import_already_attached_hidden(&label)));
        }
        let currency = crate::db::ledger::household(&st.pool)
            .await
            .ok()
            .flatten()
            .map(|h| h.currency)
            .unwrap_or_default();
        let amount = row
            .amount_cents
            .map(|c| fmt::money(Cents(c), currency))
            .unwrap_or_else(|| fmt::money_placeholder(currency));
        return Err(Refused(
            t.import_already_attached(&label, &row.title, &amount),
        ));
    }

    // The file itself is kept, under the deterministic name every other
    // receipt uses.
    let Some((slot, _, member_id)) = crate::web::files::slot_for(st, auth, tx_id).await else {
        return Err(Refused(t.file_not_tracked().to_string()));
    };
    if receipts::store(
        &st.pool,
        &st.cfg.receipts_root,
        Actor::User(auth.viewer.member_id),
        &slot,
        FileKind::Barcode,
        &upload.filename,
        &upload.bytes,
    )
    .await
    .is_err()
    {
        return Err(not_written());
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
        r#"SELECT amount_cents AS "amount_cents: i64", due_date AS "due_date: String",
                  amount_source AS "amount_source: String",
                  due_date_source AS "due_date_source: String"
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

    // Fill what is empty; replace a seed; flag what a person typed. The policy
    // itself lives in `services::import` so it can be pinned by a test rather
    // than re-derived in each handler that imports something.
    use crate::services::import::{Fill, reconcile};

    // The amount always follows the bill, whoever set the row's figure: the
    // checksummed barcode is the evidence of what is owed, and the change is
    // its own entry in the log (`import`, "read from a bill") that `u` puts
    // back. The due date keeps the older rule — a date a person set with `e`
    // is kept and the difference flagged.
    let typed = |source: Option<&str>| source == Some("user");
    let due_typed = typed(row.as_ref().and_then(|r| r.due_date_source.as_deref()));

    let amount_fill = reconcile(barcode_amount, existing_amount, false);
    match amount_fill {
        Fill::Differs { found, kept } => notes.push(
            t.import_amount_differs(&fmt::money(found, currency), &fmt::money(kept, currency)),
        ),
        Fill::Replace { was, .. } => notes.push(t.import_replaced(&fmt::money(was, currency))),
        _ => {}
    }
    let due_fill = reconcile(barcode_due, existing_due, due_typed);
    match due_fill {
        Fill::Differs { found, kept } => {
            notes.push(t.import_date_differs(&fmt::date(found), &fmt::date(kept)))
        }
        Fill::Replace { was, .. } => notes.push(t.import_replaced(&fmt::date(was))),
        _ => {}
    }

    let write_amount = amount_fill.to_write();
    let write_due = due_fill.to_write();

    let Ok(mut tx) = st.pool.begin().await else {
        return Err(not_written());
    };

    // --- a row whose bill was removed gives it up -------------------------
    // Its barcode is cleared first, so this row can take `idx_tx_barcode`;
    // what claimed to come from that barcode goes with it, since the schema
    // ties those sources to one. And its copy in the trash is retired
    // (`receipts::MOVED`): a bill is on one row only, so that copy can never
    // come back. The file stays in `.delete/` — nothing is destroyed.
    if let Some(old) = &takeover {
        let released =
            crate::db::tx::release_bill(&mut tx, Actor::User(auth.viewer.member_id), old.id, &raw)
                .await;
        if released.is_err() {
            return Err(not_written());
        }
        notes.push(
            t.import_bill_moved(
                &old.title,
                &Period::parse(&old.period)
                    .map(fmt::period)
                    .unwrap_or_else(|| old.period.clone()),
            ),
        );
    }

    let kind = parsed.barcode.kind.code();
    // A source is stamped only with the value it describes. It used to be
    // written whenever the bill carried one, so a row that KEPT its own figure
    // still claimed the barcode as the source of it.
    let amount_source = write_amount.and(parsed.import.amount_source);
    let due_source = write_due.and(parsed.import.due_source);
    let amount_value = write_amount.map(|c| c.get());
    let due_value = write_due.map(|d| d.format("%Y-%m-%d").to_string());
    let issuer = parsed.import.issuer.clone();

    let ok = sqlx::query!(
        r#"UPDATE transactions SET
               barcode_raw = ?2, barcode_kind = ?3, issuer_id = ?4,
               amount_cents = coalesce(?5, amount_cents),
               due_date = coalesce(?6, due_date),
               amount_source = coalesce(?7, amount_source),
               due_date_source = coalesce(?8, due_date_source),
               -- A new amount moved under any `e` form open on this row, so
               -- its Save must be refused rather than write the old figure
               -- back: the lost-update guard, doing its job.
               version = version + (?5 IS NOT NULL AND ?5 IS NOT amount_cents)
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
        return Err(not_written());
    }

    // --- the binding, which lives on the RULE ---------------------------
    // The same account every period, unlike the value and the due date, which
    // are genuinely per-period. Only what is empty is filled: the check above
    // already refused anything that disagreed with what is bound.
    let client_code = rule
        .client_code
        .clone()
        .or(parsed.import.client_code.clone());
    let company = rule
        .barcode_company
        .clone()
        .or(parsed.barcode.company.clone());
    let issuer_id = rule.issuer_id.clone().or(issuer.clone());
    let barcode_kind = rule.barcode_kind.clone().or(Some(kind.to_string()));
    let rule_id = rule.id;
    let bound_ok = sqlx::query!(
        "UPDATE recurring_rules SET client_code = ?2, barcode_company = ?3,
                issuer_id = ?4, barcode_kind = ?5
         WHERE id = ?1",
        rule_id,
        client_code,
        company,
        issuer_id,
        barcode_kind
    )
    .execute(&mut *tx)
    .await
    .is_ok();
    if !bound_ok {
        return Err(not_written());
    }

    let entry = Change::file(
        Actor::User(auth.viewer.member_id),
        FileAction::Attach,
        tx_id,
    )
    .from_rule(Some(rule.id))
    .in_period(period)
    .diff("barcode", Value::Text(None), Value::text(&raw))
    .diff(
        "client_code",
        Value::Text(rule.client_code.clone()),
        Value::Text(client_code),
    )
    .diff(
        "company_code",
        Value::Text(rule.barcode_company.clone()),
        Value::Text(company),
    );
    let _ = entry.write(&mut tx).await;

    // The amount, as an entry of its own rather than one field folded inside
    // "file attached · 3 fields": the one change a person needs to see is that
    // the row's figure moved, and why. Written after the upload's entry, so
    // `u` puts the old amount back before it takes the file away.
    let amount_entry = Change::transaction(
        Actor::Import(Some(auth.viewer.member_id)),
        TxAction::Import,
        tx_id,
    )
    .from_rule(Some(rule.id))
    .in_period(period)
    .diff(
        "amount",
        Value::Money(existing_amount),
        Value::Money(write_amount.or(existing_amount)),
    );
    if !amount_entry.is_empty() && amount_entry.write(&mut tx).await.is_err() {
        return Err(not_written());
    }
    if tx.commit().await.is_err() {
        return Err(not_written());
    }

    // What actually happened, in one line.
    let headline = match (barcode_amount, barcode_due) {
        (Some(a), Some(d)) => t.import_filled(&fmt::money(a, currency), &fmt::date(d)),
        // The amount is the news; what the bill did not carry follows it,
        // rather than being the only thing said.
        (Some(a), None) => {
            let read = t.import_read_amount(&fmt::money(a, currency));
            let what = parsed.import.unresolved.join(", ");
            if what.is_empty() {
                read
            } else {
                format!("{read} · {}", t.import_unresolved(&what))
            }
        }
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
    Ok((member_id, message))
}

// ---------------------------------------------------------------------------
// what the pane shows about the bill
// ---------------------------------------------------------------------------

/// "Bill on file": what a rule is bound to, or — in `A`, before the rule
/// exists — what the dropped bill will bind it to.
///
/// No amount and no due date: those change every period, and this box is
/// about the identity that does not.
#[derive(Template)]
#[template(path = "partials/barcode_meta.html")]
pub struct BarcodeMeta {
    pub t: Catalog,
    /// `E`: the rule, for Unlink. `None` in `A`.
    pub rule_id: Option<i64>,
    pub issuer: Option<String>,
    /// Already a label, not the stored code.
    pub kind: Option<String>,
    pub company: Option<String>,
    pub client_code: Option<String>,
    /// `A` only: the parked file Create will attach.
    pub token: Option<String>,
    pub filename: Option<String>,
}

fn issuer_name(profiles: &Profiles, id: Option<&str>) -> Option<String> {
    let id = id?;
    Some(
        profiles
            .issuers
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| id.to_string()),
    )
}

fn kind_label(t: &Catalog, code: Option<&str>) -> Option<String> {
    match code? {
        "consumo" => Some(t.import_meta_kind_consumo().to_string()),
        "cobranca" => Some(t.import_meta_kind_cobranca().to_string()),
        other => Some(other.to_string()),
    }
}

impl BarcodeMeta {
    pub fn empty(t: Catalog) -> BarcodeMeta {
        BarcodeMeta {
            t,
            rule_id: None,
            issuer: None,
            kind: None,
            company: None,
            client_code: None,
            token: None,
            filename: None,
        }
    }

    pub fn of_rule(st: &AppState, t: Catalog, rule: &rules::Rule) -> BarcodeMeta {
        let profiles = Profiles::load(&st.cfg.issuers_path()).unwrap_or_default();
        BarcodeMeta {
            rule_id: Some(rule.id),
            issuer: issuer_name(&profiles, rule.issuer_id.as_deref()),
            kind: kind_label(&t, rule.barcode_kind.as_deref()),
            company: rule.barcode_company.clone(),
            client_code: rule.client_code.clone(),
            token: None,
            filename: None,
            t,
        }
    }

    pub fn of_parsed(t: Catalog, parsed: &Parsed, token: String, filename: String) -> BarcodeMeta {
        BarcodeMeta {
            issuer: parsed
                .import
                .issuer_name
                .clone()
                .or(parsed.import.issuer.clone()),
            kind: kind_label(&t, Some(parsed.barcode.kind.code())),
            company: parsed.barcode.company.clone(),
            client_code: parsed.import.client_code.clone(),
            rule_id: None,
            token: Some(token),
            filename: Some(filename),
            t,
        }
    }

    /// Nothing bound and nothing parked: the box is not drawn at all — the
    /// same "absent, not a blank field" treatment `instance` gets.
    pub fn is_empty(&self) -> bool {
        self.token.is_none()
            && self.issuer.is_none()
            && self.kind.is_none()
            && self.company.is_none()
            && self.client_code.is_none()
    }
}

/// `GET /rules/{id}/meta` — the box on its own, refreshed after an `E` drop.
pub async fn meta(State(st): State<AppState>, auth: Auth, Path(rule_id): Path<i64>) -> Response {
    let t = auth.catalog();
    match rules::get(&st.pool, rule_id).await {
        Ok(Some(rule))
            if rule.visibility != crate::domain::status::Visibility::Private
                || auth.viewer.sees_private_of(rule.member_id) =>
        {
            Html(BarcodeMeta::of_rule(&st, t, &rule)).into_response()
        }
        _ => Html(BarcodeMeta::empty(t)).into_response(),
    }
}

/// `POST /rules/barcode/preview` — the dropzone in `A`.
///
/// The rule does not exist yet, so nothing is attached: the bill is read,
/// parked under a token, and the pane gets back what it says. Create carries
/// the token and attaches it — see `services::pending`.
pub async fn preview(State(st): State<AppState>, auth: Auth, mut form: Multipart) -> Response {
    let t = auth.catalog();
    // No rule yet, so the only password is the one typed in the pane. What
    // is parked is the UNLOCKED copy, so Create attaches it as it is.
    let upload = match read_upload(&t, &mut form)
        .await
        .and_then(|(u, typed)| unlock_for(&t, u, typed.as_deref()))
    {
        Ok(u) => u,
        Err(Refused(m)) => return crate::web::tx::say_only(&m, true),
    };
    let parsed = match parse(&st, &upload) {
        Ok(p) => p,
        Err(Refused(m)) => return crate::web::tx::say_only(&m, true),
    };
    let Ok(token) = pending::park(auth.viewer.member_id, &upload.filename, &upload.bytes) else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    Html(BarcodeMeta::of_parsed(t, &parsed, token, upload.filename)).into_response()
}

/// The same box for a parked bill, re-read — a rejected Create renders the
/// pane again and must not lose what was dropped.
pub fn meta_of_parked(
    st: &AppState,
    t: Catalog,
    token: &str,
    member_id: i64,
) -> Option<BarcodeMeta> {
    let filename = pending::filename_of(token, member_id)?;
    let bytes = pending::peek(token, member_id)?;
    let parsed = parse(
        st,
        &Upload {
            filename: filename.clone(),
            bytes,
        },
    )
    .ok()?;
    Some(BarcodeMeta::of_parsed(
        t,
        &parsed,
        token.to_string(),
        filename,
    ))
}

/// Clears what the rule is bound to — a narrow correction for "the wrong
/// file got attached the first time", distinct from `u`'s full revert of the
/// last change. It touches nothing else about the rule, and it answers with
/// the (now empty) box, so the pane stays open for the corrected drop.
pub async fn unlink(State(st): State<AppState>, auth: Auth, Path(rule_id): Path<i64>) -> Response {
    let t = auth.catalog();
    let Ok(Some(rule)) = rules::get(&st.pool, rule_id).await else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    if let Err(refused) = crate::web::rules::rule_access(&st, &auth, &rule).await {
        return refused;
    }

    let Ok(mut tx) = st.pool.begin().await else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    let _ = sqlx::query!(
        "UPDATE recurring_rules SET client_code = NULL, barcode_company = NULL,
                issuer_id = NULL, barcode_kind = NULL
         WHERE id = ?1",
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
    .diff(
        "company_code",
        Value::Text(rule.barcode_company),
        Value::Text(None),
    )
    .write(&mut tx)
    .await;
    if tx.commit().await.is_err() {
        return crate::web::tx::say_only(t.error_not_written(), true);
    }

    let res = Html(BarcodeMeta::empty(t)).into_response();
    crate::web::tx::say(res, t.import_unlinked(), false)
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
