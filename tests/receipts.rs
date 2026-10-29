//! The receipt vault: the disk decides, and nothing is destroyed.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::rules::{self, RuleForm};
use ffm::domain::money::{Cents, Currency};
use ffm::domain::period::Period;
use ffm::domain::receipt_path::Kind;
use ffm::domain::status::{TxKind, Visibility};
use ffm::services::receipts::Slot;
use ffm::services::{materializer, receipts};
use sqlx::SqlitePool;
use std::path::PathBuf;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

/// The soft-deleted copies of one file name in the member's trash, under the
/// test occurrence's folder,
/// whatever row id they carry: `.delete/09.r7.pdf` for `09`.
fn deleted_copies(root: &std::path::Path, vault: &str, stem: &str) -> Vec<PathBuf> {
    let dir = root.join(format!("{vault}/.delete/consumo/energia/2026"));
    let prefix = format!("{stem}.r");
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(&prefix))
                })
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

fn deleted_copy(root: &std::path::Path, vault: &str, stem: &str) -> Option<PathBuf> {
    deleted_copies(root, vault, stem).into_iter().next()
}

fn vault() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ffm-vault-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn household(pool: &SqlitePool) -> i64 {
    let sid = auth::setup(
        pool,
        auth::NewHousehold {
            household_name: "Casa Stuchi",
            currency: "BRL",
            language: "pt-BR",
            admin_name: "rodrigo",
            password: "abcd1234",
            theme: "ffm",
            line_numbers: "relative",
            device_label: "test",
        },
    )
    .await
    .unwrap();
    auth::session(pool, &sid).await.unwrap().unwrap().member.id
}

/// A tracked rule with one materialized occurrence, and the slot that
/// describes where its files go.
async fn tracked_occurrence(pool: &SqlitePool, me: i64) -> (i64, Slot) {
    rules::create(
        pool,
        Actor::User(me),
        &RuleForm {
            member_id: me,
            kind: TxKind::Out,
            title: "Energia · Enel SP".into(),
            amount: Some(Cents(34362)),
            cadence_months: 1,
            due_day: Some(11),
            start_date: d(2026, 9, 1),
            end_date: None,
            total_installments: None,
            carry_forward_amount: true,
            tracked: true,
            bucket: Some("Consumo".into()),
            bill_name: Some("Energia".into()),
            instance: None,
            no_barcode: false,
            spans_period: false,
            window_size: None,
            window_start: None,
            varies: false,
            estimate_basis: "lowest".into(),
            estimate_window: 3,
            amount_in_filename: false,
            months: None,
            visibility: Visibility::Household,
            pdf_password: None,
        },
    )
    .await
    .unwrap();
    materializer::reconcile(pool, d(2026, 9, 21)).await.unwrap();

    let id: i64 = sqlx::query_scalar("SELECT id FROM transactions LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    let slug: String = sqlx::query_scalar("SELECT slug FROM members WHERE id = ?")
        .bind(me)
        .fetch_one(pool)
        .await
        .unwrap();

    (
        id,
        Slot {
            transaction_id: id,
            vault: ffm::domain::receipt_path::vault(&slug),
            bucket: "Consumo".into(),
            bill_name: "Energia".into(),
            instance: None,
            period: Period::parse("2026-09").unwrap(),
            amount: Some((Cents(34362), Currency::Brl)),
            amount_in_filename: false,
        },
    )
}

#[tokio::test]
async fn a_receipt_on_disk_is_what_paid_means() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    let before = receipts::presence(&pool, &root, &slot).await.unwrap();
    assert_eq!(before.receipt, None);

    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "comprovante.pdf",
        b"%PDF-",
    )
    .await
    .unwrap();

    let after = receipts::presence(&pool, &root, &slot).await.unwrap();
    assert_eq!(after.receipt.as_deref(), Some("pdf"));
    assert!(
        root.join(format!("{}/consumo/energia/2026/09.pdf", slot.vault))
            .exists(),
        "renamed to the deterministic convention on disk"
    );
}

#[tokio::test]
async fn a_file_deleted_by_hand_reads_as_unpaid() {
    // The whole reason the filesystem is authoritative: the vault is meant to
    // be manipulated directly, and a database flag that could disagree with
    // the folder is the failure mode this app avoids everywhere else.
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "r.pdf",
        b"x",
    )
    .await
    .unwrap();
    std::fs::remove_file(root.join(format!("{}/consumo/energia/2026/09.pdf", slot.vault))).unwrap();

    let present = receipts::presence(&pool, &root, &slot).await.unwrap();
    assert_eq!(
        present.receipt, None,
        "the row still says pdf; the disk says no"
    );
}

#[tokio::test]
async fn a_file_added_by_hand_is_found() {
    // Step two of the check, and the one that is easy to skip: with no
    // receipts row there is no extension to build a path from, so the four
    // allowed ones are probed. Skip it and hand-filing silently stops working.
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    let dir = root.join(format!("{}/consumo/energia/2026", slot.vault));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("09.jpg"), b"photo").unwrap();

    let present = receipts::presence(&pool, &root, &slot).await.unwrap();
    assert_eq!(present.receipt.as_deref(), Some("jpg"));
}

#[tokio::test]
async fn uploading_the_receipt_supersedes_the_scheduled_file() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Scheduled,
        "s.pdf",
        b"s",
    )
    .await
    .unwrap();
    assert!(
        receipts::presence(&pool, &root, &slot)
            .await
            .unwrap()
            .scheduled
            .is_some()
    );

    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "r.pdf",
        b"r",
    )
    .await
    .unwrap();

    // The promise is resolved, so only the definitive proof is left standing.
    let present = receipts::presence(&pool, &root, &slot).await.unwrap();
    assert_eq!(present.scheduled, None);
    assert!(present.receipt.is_some());

    // Moved, not destroyed — and the log says WHY it moved, so the audit trail
    // does not read as somebody deleting it on purpose.
    assert!(deleted_copy(&root, &slot.vault, "09_scheduled").is_some());
    let reason: Option<String> =
        sqlx::query_scalar("SELECT delete_reason FROM receipts WHERE kind = 'scheduled'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(reason.as_deref(), Some("superseded"));
}

#[tokio::test]
async fn deleting_a_file_moves_it_and_restoring_puts_it_back() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "r.pdf",
        b"r",
    )
    .await
    .unwrap();
    receipts::soft_delete(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "manual",
        None,
    )
    .await
    .unwrap();

    let live = root.join(format!("{}/consumo/energia/2026/09.pdf", slot.vault));
    assert!(
        !live.exists() && deleted_copy(&root, &slot.vault, "09").is_some(),
        "never removed from disk"
    );
    assert_eq!(
        receipts::presence(&pool, &root, &slot)
            .await
            .unwrap()
            .receipt,
        None
    );

    let receipt_id: i64 = sqlx::query_scalar("SELECT id FROM receipts WHERE kind = 'receipt'")
        .fetch_one(&pool)
        .await
        .unwrap();
    receipts::restore(&pool, &root, Actor::User(me), receipt_id, None)
        .await
        .unwrap();

    assert!(live.exists() && deleted_copy(&root, &slot.vault, "09").is_none());
    assert!(
        receipts::presence(&pool, &root, &slot)
            .await
            .unwrap()
            .receipt
            .is_some()
    );
}

#[tokio::test]
async fn a_hand_filed_file_can_still_be_deleted_and_the_move_is_recorded() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    let dir = root.join(format!("{}/consumo/energia/2026", slot.vault));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("09.png"), b"shot").unwrap();

    assert!(
        receipts::soft_delete(
            &pool,
            &root,
            Actor::User(me),
            &slot,
            Kind::Receipt,
            "manual",
            None
        )
        .await
        .unwrap()
    );
    // It had no receipts row, so the delete wrote one: the move has to be
    // undoable, and the row is where the old path lives.
    let path: Option<String> =
        sqlx::query_scalar("SELECT deleted_from_path FROM receipts WHERE kind = 'receipt'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(path.unwrap().ends_with("2026/09.png"));
}

#[tokio::test]
async fn only_the_four_allowed_extensions_are_kept() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    for name in ["bill.exe", "bill.zip", "noextension"] {
        assert!(
            receipts::store(
                &pool,
                &root,
                Actor::User(me),
                &slot,
                Kind::Receipt,
                name,
                b"x"
            )
            .await
            .is_err(),
            "{name}"
        );
    }
    // And the extension is normalized, so a JPG and a jpg are one file.
    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "R.JPG",
        b"x",
    )
    .await
    .unwrap();
    assert!(
        root.join(format!("{}/consumo/energia/2026/09.jpg", slot.vault))
            .exists()
    );
}

#[tokio::test]
async fn every_upload_and_delete_is_in_the_append_only_log() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Barcode,
        "b.pdf",
        b"b",
    )
    .await
    .unwrap();
    receipts::soft_delete(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Barcode,
        "manual",
        None,
    )
    .await
    .unwrap();

    let entries: Vec<(String, String)> =
        sqlx::query_as("SELECT action, entity FROM change WHERE entity = 'file' ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        entries,
        vec![
            ("file_upload".into(), "file".into()),
            ("file_delete".into(), "file".into()),
        ],
        "the file records are entries in the ONE audit mechanism, not a second one"
    );
}

// ---------------------------------------------------------------------------
// a bill covering several months — each month takes the code due in it
// ---------------------------------------------------------------------------

/// A tracked rule whose bill covers several months, materialized through
/// `today`.
async fn covering_rule(pool: &SqlitePool, me: i64, today: NaiveDate) -> i64 {
    let form = RuleForm {
        member_id: me,
        kind: TxKind::Out,
        title: "IPTU".into(),
        amount: None,
        cadence_months: 1,
        due_day: Some(10),
        start_date: d(2026, 1, 1),
        end_date: None,
        total_installments: None,
        carry_forward_amount: false,
        tracked: true,
        bucket: Some("Casa".into()),
        bill_name: Some("IPTU".into()),
        instance: None,
        no_barcode: false,
        spans_period: true,
        // Retired fields the schema's CHECK still wants set.
        window_size: Some(1),
        window_start: Period::parse("2026-01"),
        varies: false,
        estimate_basis: "lowest".into(),
        estimate_window: 3,
        amount_in_filename: false,
        months: None,
        visibility: Visibility::Household,
        pdf_password: None,
    };
    let rule = rules::create(pool, Actor::User(me), &form).await.unwrap();
    materializer::reconcile(pool, today).await.unwrap();
    rule
}

/// A made-up utility code for `cents`, due `yyyymmdd`, built with the app's
/// own check digits — what `services::installments` reads off a real file.
fn code(cents: u64, ymd: &str, serial: u32) -> ffm::services::installments::FoundCode {
    use ffm::services::boleto::{self, mod10};
    let mut digits: Vec<u32> = vec![8, 1, 6, 0];
    digits.extend(
        format!("{cents:011}")
            .chars()
            .map(|c| c.to_digit(10).unwrap()),
    );
    digits.extend([0, 3, 0, 6]);
    digits.extend(
        format!("{ymd}{serial:017}")
            .chars()
            .map(|c| c.to_digit(10).unwrap()),
    );
    let mut body = digits[0..3].to_vec();
    body.extend_from_slice(&digits[4..]);
    digits[3] = mod10(&body);
    let raw: String = digits
        .iter()
        .map(|x| char::from_digit(*x, 10).unwrap())
        .collect();
    ffm::services::installments::FoundCode {
        barcode: boleto::parse(&raw, d(2026, 10, 7)).unwrap(),
        page: 1 + serial / 4,
    }
}

/// The ten installments of a made-up IPTU, January to October.
fn plan(cents: u64) -> Vec<(Period, ffm::services::installments::FoundCode)> {
    (1..=10u32)
        .map(|m| {
            let p = Period::new(2026, m).unwrap();
            (p, code(cents, &format!("2026{m:02}10"), m))
        })
        .collect()
}

async fn row_in(pool: &SqlitePool, rule: i64, period: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM transactions WHERE rule_id = ?1 AND period = ?2")
        .bind(rule)
        .bind(period)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The shared file, stored on `row` as an upload would, and its record id.
async fn shared_file(pool: &SqlitePool, root: &std::path::Path, me: i64, row: i64) -> i64 {
    let (slot, _) = receipts::slot_of(pool, row).await.unwrap().unwrap();
    receipts::store(
        pool,
        root,
        Actor::User(me),
        &slot,
        Kind::Barcode,
        "iptu.pdf",
        b"%PDF-",
    )
    .await
    .unwrap();
    sqlx::query_scalar("SELECT id FROM receipts WHERE transaction_id = ?1 AND deleted_at IS NULL")
        .bind(row)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn keep(
    pool: &SqlitePool,
    rule: i64,
    receipt: i64,
    plan: &[(Period, ffm::services::installments::FoundCode)],
) {
    let mut tx = pool.begin().await.unwrap();
    ffm::db::bill_codes::replace(&mut tx, rule, receipt, plan, "2026-10-07T12:00:00Z")
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn page_of(pool: &SqlitePool, root: &std::path::Path, me: i64, period: &str) -> Option<u32> {
    use ffm::db::ledger;
    use ffm::domain::viewer::Viewer;
    use ffm::ui::row::RowCtx;

    let member = ledger::members(pool)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == me)
        .unwrap();
    let ctx = RowCtx {
        today: d(2026, 10, 7),
        sort: Default::default(),
        currency: Currency::Brl,
        show_recurring: true,
        show_delta: true,
        narrow: false,
        line_numbers: Default::default(),
    };
    let pane = ledger::pane_with_vault(
        pool,
        &member,
        Period::parse(period).unwrap(),
        ctx,
        Viewer::new(me, true),
        Some(root),
    )
    .await
    .unwrap();
    let row = pane.rows.first()?;
    row.files.bill.as_ref()?;
    row.barcode_page().map(|p| p.page)
}

#[tokio::test]
async fn every_covered_month_shows_the_one_file_on_its_codes_page() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let rule = covering_rule(&pool, me, d(2026, 10, 7)).await;
    let october = row_in(&pool, rule, "2026-10").await;
    let receipt = shared_file(&pool, &root, me, october).await;
    keep(&pool, rule, receipt, &plan(12162)).await;

    // January's row holds no file of its own, and shows October's — on the
    // page its own code is printed on.
    assert_eq!(page_of(&pool, &root, me, "2026-01").await, Some(1));
    assert_eq!(page_of(&pool, &root, me, "2026-06").await, Some(2));
    assert_eq!(page_of(&pool, &root, me, "2026-10").await, Some(3));
}

#[tokio::test]
async fn each_month_is_stamped_with_its_own_code_and_a_later_month_too() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let rule = covering_rule(&pool, me, d(2026, 8, 15)).await;
    let august = row_in(&pool, rule, "2026-08").await;
    let receipt = shared_file(&pool, &root, me, august).await;
    let plan = plan(12162);
    keep(&pool, rule, receipt, &plan).await;

    for (p, _) in &plan {
        let Ok(id) = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM transactions WHERE rule_id = ?1 AND period = ?2",
        )
        .bind(rule)
        .bind(p.to_string())
        .fetch_one(&pool)
        .await
        else {
            continue;
        };
        let code = ffm::db::bill_codes::for_period(&pool, rule, *p)
            .await
            .unwrap()
            .unwrap();
        ffm::db::bill_codes::stamp(&pool, Actor::Import(Some(me)), id, &code)
            .await
            .unwrap();
    }
    let (amount, due): (i64, String) = sqlx::query_as(
        "SELECT amount_cents, due_date FROM transactions WHERE rule_id = ?1 AND period = '2026-03'",
    )
    .bind(rule)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((amount, due.as_str()), (12162, "2026-03-10"));

    // October did not exist when the file came: the materializer stamps it
    // from its code when it does.
    materializer::reconcile(&pool, d(2026, 10, 7))
        .await
        .unwrap();
    let (amount, due): (i64, String) = sqlx::query_as(
        "SELECT amount_cents, due_date FROM transactions WHERE rule_id = ?1 AND period = '2026-10'",
    )
    .bind(rule)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((amount, due.as_str()), (12162, "2026-10-10"));
}

#[tokio::test]
async fn a_month_whose_code_another_row_holds_still_gets_its_amount() {
    // The same IPTU attached to a second rule while the first rule's rows
    // still hold its codes: the code stays where it is (one barcode, one
    // row), and the month still takes its amount and due date — read off the
    // bill's text, since a `barcode` source needs a code on the row.
    let pool = pool().await;
    let me = household(&pool).await;
    let first = covering_rule(&pool, me, d(2026, 3, 15)).await;
    let second = covering_rule(&pool, me, d(2026, 3, 15)).await;
    let plan = plan(12162);
    let march = Period::parse("2026-03").unwrap();
    let mut held = Vec::new();
    for rule in [first, second] {
        let row = row_in(&pool, rule, "2026-03").await;
        let receipt: i64 = sqlx::query_scalar(
            "INSERT INTO receipts (transaction_id, kind, path, ext, original_filename, uploaded_by, uploaded_at)
             VALUES (?1, 'barcode', 'x/2026/03_barcode.pdf', 'pdf', 'iptu.pdf', ?2, '2026-03-15T12:00:00Z')
             RETURNING id",
        )
        .bind(row)
        .bind(me)
        .fetch_one(&pool)
        .await
        .unwrap();
        keep(&pool, rule, receipt, &plan).await;
        let code = ffm::db::bill_codes::for_period(&pool, rule, march)
            .await
            .unwrap()
            .unwrap();
        assert!(
            ffm::db::bill_codes::stamp(&pool, Actor::Import(Some(me)), row, &code)
                .await
                .unwrap()
        );
        let got: (i64, String, Option<String>, String) = sqlx::query_as(
            "SELECT amount_cents, due_date, barcode_raw, amount_source FROM transactions WHERE id = ?1",
        )
        .bind(row)
        .fetch_one(&pool)
        .await
        .unwrap();
        held.push(got);
    }
    assert_eq!(held[0].0, 12162);
    assert!(held[0].2.is_some());
    assert_eq!(held[0].3, "barcode");
    assert_eq!((held[1].0, held[1].1.as_str()), (12162, "2026-03-10"));
    assert_eq!(held[1].2, None, "the code stays on the row that holds it");
    assert_eq!(held[1].3, "text");
}

#[tokio::test]
async fn a_new_file_retires_the_last_files_codes() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let rule = covering_rule(&pool, me, d(2026, 10, 7)).await;
    let october = row_in(&pool, rule, "2026-10").await;
    let receipt = shared_file(&pool, &root, me, october).await;
    keep(&pool, rule, receipt, &plan(12162)).await;
    keep(&pool, rule, receipt, &plan(13000)).await;

    let (live, retired): (i64, i64) = sqlx::query_as(
        "SELECT sum(retired_at IS NULL), sum(retired_at IS NOT NULL) FROM rule_bill_code",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (live, retired),
        (10, 10),
        "history keeps the codes it was billed with"
    );
    let code = ffm::db::bill_codes::for_period(&pool, rule, Period::parse("2026-05").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(code.amount, Some(Cents(13000)));
}

#[tokio::test]
async fn u_reaches_a_deleted_file_and_puts_it_back() {
    use ffm::db::tx;

    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (tx_id, slot) = tracked_occurrence(&pool, me).await;

    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "r.pdf",
        b"r",
    )
    .await
    .unwrap();
    receipts::soft_delete(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "manual",
        None,
    )
    .await
    .unwrap();

    // The statusline promises `u` puts it back, so undo has to reach a file
    // entry and not only a transaction one.
    let u = tx::last_undoable(&pool, me)
        .await
        .unwrap()
        .expect("something to undo");
    assert_eq!(
        (u.entity.as_str(), u.action.as_str()),
        ("file", "file_delete")
    );

    let receipt_id: i64 = sqlx::query_scalar("SELECT id FROM receipts WHERE kind = 'receipt'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(u.entity_id, receipt_id, "the entry names the receipts row");

    assert!(
        receipts::restore(&pool, &root, Actor::User(me), u.entity_id, None)
            .await
            .unwrap()
    );
    assert!(
        receipts::presence(&pool, &root, &slot)
            .await
            .unwrap()
            .receipt
            .is_some()
    );
    assert_eq!(tx_id, slot.transaction_id);
}

/// A vault written by an earlier version — `{uuid}-{slug}` — is moved to
/// `{slug}` at startup, with every stored path following it, and running the
/// step again changes nothing.
#[tokio::test]
async fn the_uuid_prefix_is_dropped_once_and_the_paths_follow() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, mut slot) = tracked_occurrence(&pool, me).await;
    let uuid: String = sqlx::query_scalar("SELECT uuid FROM members WHERE id = ?")
        .bind(me)
        .fetch_one(&pool)
        .await
        .unwrap();
    let old_vault = format!("{uuid}-{}", slot.vault);
    let new_vault = std::mem::replace(&mut slot.vault, old_vault.clone());

    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "c.pdf",
        b"%PDF-",
    )
    .await
    .unwrap();
    assert!(root.join(&old_vault).is_dir());

    let moved = ffm::services::vault::drop_uuid_prefix(&pool, &root)
        .await
        .unwrap();
    assert_eq!(moved, 1);
    assert!(!root.join(&old_vault).exists());
    assert!(root.join(&new_vault).is_dir());

    let path: String = sqlx::query_scalar("SELECT path FROM receipts LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(path.starts_with(&format!("{new_vault}/")), "{path}");

    // The disk and the database agree: the row reads as paid under the new name.
    slot.vault = new_vault;
    let now = receipts::presence(&pool, &root, &slot).await.unwrap();
    assert_eq!(now.receipt.as_deref(), Some("pdf"));

    // Idempotent: a second startup does nothing.
    assert_eq!(
        ffm::services::vault::drop_uuid_prefix(&pool, &root)
            .await
            .unwrap(),
        0
    );
}

/// Replacing a bill used to write over it: the old bytes were gone, and the
/// row marked deleted pointed at the new file. Every replaced version is now
/// kept in `.delete/` under a name of its own, and `u` walks back through them.
#[tokio::test]
async fn replacing_a_file_keeps_every_earlier_version() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    for body in [b"first".as_slice(), b"second", b"third"] {
        receipts::store(
            &pool,
            &root,
            Actor::User(me),
            &slot,
            Kind::Barcode,
            "b.pdf",
            body,
        )
        .await
        .unwrap();
    }

    let live = root.join(format!(
        "{}/consumo/energia/2026/09_barcode.pdf",
        slot.vault
    ));
    assert_eq!(std::fs::read(&live).unwrap(), b"third");
    let kept: Vec<Vec<u8>> = deleted_copies(&root, &slot.vault, "09_barcode")
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect();
    assert_eq!(kept.len(), 2, "both earlier versions are on disk");
    assert!(kept.contains(&b"first".to_vec()) && kept.contains(&b"second".to_vec()));

    // The row the second upload retired restores to exactly its own bytes —
    // once the live file has been moved aside, which is what `u` on the
    // third upload does first.
    receipts::soft_delete(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Barcode,
        "manual",
        None,
    )
    .await
    .unwrap();
    let second: i64 = sqlx::query_scalar(
        "SELECT id FROM receipts WHERE kind = 'barcode' ORDER BY id LIMIT 1 OFFSET 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        receipts::restore(&pool, &root, Actor::User(me), second, None)
            .await
            .unwrap()
    );
    assert_eq!(std::fs::read(&live).unwrap(), b"second");
}

/// Restoring never overwrites a live file — that would be the hard delete
/// this module exists to prevent, pointed the other way.
#[tokio::test]
async fn restoring_refuses_to_overwrite_a_live_file() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    for body in [b"old".as_slice(), b"new"] {
        receipts::store(
            &pool,
            &root,
            Actor::User(me),
            &slot,
            Kind::Barcode,
            "b.pdf",
            body,
        )
        .await
        .unwrap();
    }
    let old: i64 =
        sqlx::query_scalar("SELECT id FROM receipts WHERE kind = 'barcode' ORDER BY id LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        receipts::restore(&pool, &root, Actor::User(me), old, None)
            .await
            .is_err()
    );
    let live = root.join(format!(
        "{}/consumo/energia/2026/09_barcode.pdf",
        slot.vault
    ));
    assert_eq!(std::fs::read(&live).unwrap(), b"new");
}

/// A deleted row's files follow it into the member's trash, the folder they
/// leave empty goes, and `u` brings the row AND its files back — one
/// keystroke, because the file moves are part of the row's delete.
#[tokio::test]
async fn a_rows_files_follow_it_into_the_trash_and_back() {
    use ffm::db::tx;
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (id, slot) = tracked_occurrence(&pool, me).await;
    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Barcode,
        "b.pdf",
        b"bill",
    )
    .await
    .unwrap();
    let live = root.join(format!(
        "{}/consumo/energia/2026/09_barcode.pdf",
        slot.vault
    ));
    assert!(live.exists());

    // What the `dd` handler does: the row, then its files.
    tx::soft_delete(&pool, Actor::User(me), id).await.unwrap();
    assert_eq!(
        receipts::retire_with_row(&pool, &root, Actor::User(me), id)
            .await
            .unwrap(),
        1
    );
    assert!(!live.exists());
    assert_eq!(
        std::fs::read(deleted_copy(&root, &slot.vault, "09_barcode").unwrap()).unwrap(),
        b"bill"
    );
    assert!(
        !root.join(format!("{}/consumo", slot.vault)).exists(),
        "the emptied folders are gone from the live tree"
    );

    // `u` picks the row's delete, not the file move it caused.
    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_eq!(
        (u.entity.as_str(), u.action.as_str()),
        ("transaction", "delete")
    );
    assert!(tx::undo(&pool, Actor::User(me), &u).await.unwrap());
    assert_eq!(
        receipts::restore_with_row(&pool, &root, Actor::User(me), id)
            .await
            .unwrap(),
        1
    );
    assert_eq!(std::fs::read(&live).unwrap(), b"bill");
    assert!(deleted_copy(&root, &slot.vault, "09_barcode").is_none());
}

/// The startup step: a deleted file still in a `.delete/` beside it moves to
/// the member's trash, a deleted row's live files follow it there, and a
/// second run does nothing.
#[tokio::test]
async fn the_startup_step_tidies_the_trash_once() {
    use ffm::db::tx;
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (id, slot) = tracked_occurrence(&pool, me).await;
    let folder = format!("{}/consumo/energia/2026", slot.vault);

    // A deleted file where the old layout put it: beside, row-named.
    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Barcode,
        "b.pdf",
        b"old",
    )
    .await
    .unwrap();
    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Barcode,
        "b.pdf",
        b"new",
    )
    .await
    .unwrap();
    let first: i64 = sqlx::query_scalar("SELECT min(id) FROM receipts")
        .fetch_one(&pool)
        .await
        .unwrap();
    let trash = root.join(ffm::domain::receipt_path::deleted_path_for(
        &format!("{folder}/09_barcode.pdf"),
        first,
    ));
    let beside = root.join(format!("{folder}/.delete/09_barcode.r{first}.pdf"));
    std::fs::create_dir_all(beside.parent().unwrap()).unwrap();
    std::fs::rename(&trash, &beside).unwrap();

    // A row deleted the old way: its live file left standing.
    tx::soft_delete(&pool, Actor::User(me), id).await.unwrap();

    let moved = ffm::services::vault::tidy_trash(&pool, &root)
        .await
        .unwrap();
    assert_eq!(moved, 2);
    assert_eq!(std::fs::read(&trash).unwrap(), b"old");
    assert!(!root.join(format!("{folder}/.delete")).exists());
    assert!(
        !root.join(&folder).exists(),
        "nothing live is left in the folder"
    );
    assert_eq!(
        ffm::services::vault::tidy_trash(&pool, &root)
            .await
            .unwrap(),
        0
    );
}

// ---------------------------------------------------------------------------
// a removed bill, attached to another row, is retired from the trash
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_removed_bill_gives_its_barcode_up_and_its_trashed_copy_is_retired() {
    use ffm::db::tx::{self, Direction, Scope};
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (id, slot) = tracked_occurrence(&pool, me).await;

    // The bill on the row, as `attach_to` leaves it: the file and its barcode.
    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Barcode,
        "bill.pdf",
        b"%PDF-1.4",
    )
    .await
    .unwrap();
    let raw = "83650000001314500403157368061800310128719865";
    sqlx::query(
        "UPDATE transactions SET barcode_raw = ?, barcode_kind = 'consumo',
                amount_source = 'barcode' WHERE id = ?",
    )
    .bind(raw)
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    // Removed: the file goes to the trash, the barcode stays on the row.
    assert!(
        receipts::soft_delete(
            &pool,
            &root,
            Actor::User(me),
            &slot,
            Kind::Barcode,
            "manual",
            None
        )
        .await
        .unwrap()
    );
    let copy: i64 = sqlx::query_scalar("SELECT id FROM receipts WHERE kind = 'barcode'")
        .fetch_one(&pool)
        .await
        .unwrap();

    // The same bill is attached elsewhere: this row gives it up.
    let mut t = pool.begin().await.unwrap();
    tx::release_bill(&mut t, Actor::User(me), id, raw)
        .await
        .unwrap();
    t.commit().await.unwrap();

    let (barcode, source): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT barcode_raw, amount_source FROM transactions WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(barcode, None, "free for the row that has the bill now");
    assert_eq!(
        source, None,
        "nothing claims to come from a barcode it no longer has"
    );

    // Retired: not in the trash, not a step for `u`, and no restore.
    let viewer = ffm::domain::viewer::Viewer::new(me, true);
    let (items, _) = ffm::db::trash::list(&pool, viewer, 50).await.unwrap();
    assert!(items.iter().all(|i| i.id != copy), "U no longer offers it");

    let scope = Scope {
        me,
        owner: Some(me),
        period: Some(Period::parse("2026-09").unwrap()),
    };
    let next = tx::next(&pool, scope, Direction::Undo).await.unwrap();
    assert!(
        next.pick
            .as_ref()
            .is_none_or(|u| !(u.entity == "file" && u.entity_id == copy)),
        "u skips its delete"
    );
    assert!(
        !receipts::restore(&pool, &root, Actor::User(me), copy, None)
            .await
            .unwrap(),
        "and a restore refuses it"
    );

    // The file itself is still on disk: nothing is destroyed.
    assert!(deleted_copy(&root, &slot.vault, "09_barcode").is_some());
}

/// A one-page PDF that opens only with `1234`, made by qpdf — `None` where
/// qpdf is not installed, and the test says so and passes.
fn locked_pdf() -> Option<Vec<u8>> {
    let dir = std::env::temp_dir();
    let plain = dir.join(format!("ffm-locked-{}-plain.pdf", std::process::id()));
    let locked = dir.join(format!("ffm-locked-{}.pdf", std::process::id()));
    let ok = std::process::Command::new("qpdf")
        .arg("--empty")
        .arg(&plain)
        .status()
        .is_ok_and(|s| s.success())
        && std::process::Command::new("qpdf")
            .args([
                "--encrypt",
                "--user-password=1234",
                "--owner-password=x",
                "--bits=256",
                "--",
            ])
            .arg(&plain)
            .arg(&locked)
            .status()
            .is_ok_and(|s| s.success());
    let bytes = ok.then(|| std::fs::read(&locked).unwrap());
    let _ = std::fs::remove_file(&plain);
    let _ = std::fs::remove_file(&locked);
    if bytes.is_none() {
        eprintln!("qpdf not on PATH — skipping");
    }
    bytes
}

/// The backstop behind every upload path: a PDF that still needs a password
/// never lands in the vault — and is refused BEFORE the file already on the
/// row is moved aside for it.
#[tokio::test]
async fn a_locked_pdf_is_refused_and_the_file_on_the_row_stays() {
    let Some(locked) = locked_pdf() else { return };
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;

    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "r.pdf",
        b"r",
    )
    .await
    .unwrap();

    let refused = receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "fatura.pdf",
        &locked,
    )
    .await;
    assert!(matches!(refused, Err(receipts::StoreError::Locked)));

    let present = receipts::presence(&pool, &root, &slot).await.unwrap();
    assert_eq!(present.receipt.as_deref(), Some("pdf"));
    assert_eq!(
        std::fs::read(root.join(format!("{}/consumo/energia/2026/09.pdf", slot.vault))).unwrap(),
        b"r",
        "the live file was not touched"
    );
    assert!(deleted_copies(&root, &slot.vault, "09").is_empty());
}

/// A pane asks the vault once for all its rows (`receipts::files_of`), so a
/// batch must answer each row with its OWN files: a receipt stored for July
/// is July's alone, a bill filed by hand in August is August's alone, and
/// every answer is what asking for that one row would have said.
#[tokio::test]
async fn a_batch_answers_each_row_with_its_own_files() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, sep) = tracked_occurrence(&pool, me).await;
    // The same rule, two months back: July and August materialize too.
    sqlx::query("UPDATE recurring_rules SET start_date = '2026-07-01'")
        .execute(&pool)
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    let jul = row_of(&pool, "2026-07").await;
    let aug = row_of(&pool, "2026-08").await;

    let jul_slot = receipts::slot_of(&pool, jul).await.unwrap().unwrap().0;
    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &jul_slot,
        Kind::Receipt,
        "comprovante.pdf",
        b"%PDF-",
    )
    .await
    .unwrap();
    // Filed by hand: no `receipts` row, so only the probe can find it.
    let hand = root.join(format!("{}/consumo/energia/2026/08_barcode.png", sep.vault));
    std::fs::create_dir_all(hand.parent().unwrap()).unwrap();
    std::fs::write(&hand, b"png").unwrap();

    let ids = [jul, aug, sep.transaction_id, 999_999];
    let batch = receipts::files_of(&pool, &root, &ids).await.unwrap();

    assert_eq!(batch[&jul].receipt.as_deref(), Some("pdf"));
    assert_eq!(batch[&jul].bill, None);
    assert_eq!(batch[&aug].bill.as_deref(), Some("png"));
    assert_eq!(batch[&aug].receipt, None);
    assert_eq!(batch[&sep.transaction_id], Default::default());
    assert!(!batch.contains_key(&999_999), "no row, no path, no answer");

    for id in [jul, aug, sep.transaction_id] {
        let slot = receipts::slot_of(&pool, id).await.unwrap().unwrap().0;
        let one = receipts::presence(&pool, &root, &slot).await.unwrap();
        assert_eq!(batch[&id], one, "row {id}: the batch agrees with one row");
    }
}

async fn row_of(pool: &SqlitePool, period: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM transactions WHERE period = ?")
        .bind(period)
        .fetch_one(pool)
        .await
        .unwrap()
}
