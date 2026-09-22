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
        },
    )
    .await
    .unwrap();
    materializer::reconcile(pool, d(2026, 9, 21)).await.unwrap();

    let id: i64 = sqlx::query_scalar("SELECT id FROM transactions LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    let (uuid, slug): (String, String) =
        sqlx::query_as("SELECT uuid, slug FROM members WHERE id = ?")
            .bind(me)
            .fetch_one(pool)
            .await
            .unwrap();

    (
        id,
        Slot {
            transaction_id: id,
            vault: format!("{uuid}-{slug}"),
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
    assert!(
        root.join(format!(
            "{}/consumo/energia/2026/.delete/09_scheduled.pdf",
            slot.vault
        ))
        .exists()
    );
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
    let graveyard = root.join(format!(
        "{}/consumo/energia/2026/.delete/09.pdf",
        slot.vault
    ));
    assert!(
        !live.exists() && graveyard.exists(),
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

    assert!(live.exists() && !graveyard.exists());
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
    // And the extension is normalised, so a JPG and a jpg are one file.
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
// the multi-period barcode window — the KNOWN HOLE, closed
// ---------------------------------------------------------------------------

/// Builds a tracked, window-carrying rule and materializes a year of it.
async fn cyclic_rule(pool: &SqlitePool, me: i64, window_start: &str, size: i64) -> i64 {
    let form = RuleForm {
        member_id: me,
        kind: TxKind::Out,
        title: "IPTU".into(),
        amount: Some(Cents(21000)),
        cadence_months: 1,
        due_day: Some(10),
        start_date: d(2026, 1, 1),
        end_date: None,
        total_installments: None,
        carry_forward_amount: true,
        tracked: true,
        bucket: Some("Casa".into()),
        bill_name: Some("IPTU".into()),
        instance: None,
        no_barcode: false,
        spans_period: true,
        window_size: Some(size),
        window_start: Period::parse(window_start),
        varies: false,
        estimate_basis: "lowest".into(),
        estimate_window: 3,
        amount_in_filename: false,
        months: None,
        visibility: Visibility::Household,
    };
    rules::create(pool, Actor::User(me), &form).await.unwrap()
}

async fn page_of(pool: &SqlitePool, me: i64, period: &str) -> Option<(i32, bool)> {
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
        today: d(2027, 3, 1),
        sort: Default::default(),
        currency: Currency::Brl,
        show_recurring: true,
        show_delta: true,
        narrow: false,
        line_numbers: Default::default(),
    };
    let pane = ledger::pane(
        pool,
        &member,
        Period::parse(period).unwrap(),
        ctx,
        Viewer::new(me, true),
    )
    .await
    .unwrap();
    pane.rows
        .first()
        .and_then(|r| r.barcode_page())
        .map(|p| (p.page, p.in_window))
}

#[tokio::test]
async fn renewing_a_window_does_not_rewrite_last_years_pages() {
    // The recorded KNOWN HOLE: the page was derived from the rule's CURRENT
    // window_start, and a cyclic bill renews by re-entering it — so setting
    // the 2027 window turned January 2026 into "page -11, outside this file's
    // 10 pages", flagging history that was never wrong.
    let pool = pool().await;
    let me = household(&pool).await;
    let rule = cyclic_rule(&pool, me, "2026-01", 10).await;
    materializer::reconcile(&pool, d(2026, 6, 15))
        .await
        .unwrap();

    assert_eq!(page_of(&pool, me, "2026-01").await, Some((1, true)));
    assert_eq!(page_of(&pool, me, "2026-04").await, Some((4, true)));

    // Next January the tax office sends a new document, and the household
    // re-enters the two fields. That IS the renewal — there is no separate
    // control, and none is needed.
    let mut next = rules::get(&pool, rule).await.unwrap().unwrap();
    next.window_start = Period::parse("2027-01");
    let form = RuleForm {
        member_id: next.member_id,
        kind: next.kind,
        title: next.title.clone(),
        amount: next.amount,
        cadence_months: next.cadence_months,
        due_day: next.due_day,
        start_date: next.start_date,
        end_date: next.end_date,
        total_installments: next.total_installments,
        carry_forward_amount: next.carry_forward_amount,
        tracked: next.tracked,
        bucket: next.bucket.clone(),
        bill_name: next.bill_name.clone(),
        instance: next.instance.clone(),
        no_barcode: next.no_barcode,
        spans_period: true,
        window_size: Some(10),
        window_start: Period::parse("2027-01"),
        varies: next.varies,
        estimate_basis: next.estimate_basis.clone(),
        estimate_window: next.estimate_window,
        amount_in_filename: next.amount_in_filename,
        months: None,
        visibility: next.visibility,
    };
    rules::edit(&pool, Actor::User(me), rule, &form)
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2027, 3, 1)).await.unwrap();

    // Last year still reads the way it always did...
    assert_eq!(
        page_of(&pool, me, "2026-01").await,
        Some((1, true)),
        "history resolves against the window that covered IT"
    );
    assert_eq!(page_of(&pool, me, "2026-04").await, Some((4, true)));
    // ...and the new cycle counts from 1 again.
    assert_eq!(page_of(&pool, me, "2027-01").await, Some((1, true)));
    assert_eq!(page_of(&pool, me, "2027-03").await, Some((3, true)));

    // Exactly one live window, with the old one retired rather than deleted.
    let windows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT window_start, retired_at FROM rule_barcode_window ORDER BY window_start",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(windows.len(), 2);
    assert!(windows[0].1.is_some(), "2026 retired");
    assert!(windows[1].1.is_none(), "2027 live");
}

#[tokio::test]
async fn a_period_past_the_windows_page_count_is_flagged_not_silently_wrong() {
    let pool = pool().await;
    let me = household(&pool).await;
    cyclic_rule(&pool, me, "2026-01", 10).await;
    materializer::reconcile(&pool, d(2026, 12, 20))
        .await
        .unwrap();

    // November is page 11 of a ten-page file: the cue that a fresh document
    // needs uploading, not a bug to chase.
    assert_eq!(page_of(&pool, me, "2026-11").await, Some((11, false)));
}

#[tokio::test]
async fn an_ordinary_save_does_not_pile_up_windows() {
    let pool = pool().await;
    let me = household(&pool).await;
    let rule = cyclic_rule(&pool, me, "2026-01", 10).await;
    let r = rules::get(&pool, rule).await.unwrap().unwrap();

    let form = RuleForm {
        member_id: r.member_id,
        kind: r.kind,
        title: "IPTU 2026".into(),
        amount: r.amount,
        cadence_months: r.cadence_months,
        due_day: r.due_day,
        start_date: r.start_date,
        end_date: r.end_date,
        total_installments: r.total_installments,
        carry_forward_amount: r.carry_forward_amount,
        tracked: r.tracked,
        bucket: r.bucket.clone(),
        bill_name: r.bill_name.clone(),
        instance: r.instance.clone(),
        no_barcode: r.no_barcode,
        spans_period: true,
        window_size: r.window_size,
        window_start: r.window_start,
        varies: r.varies,
        estimate_basis: r.estimate_basis.clone(),
        estimate_window: r.estimate_window,
        amount_in_filename: r.amount_in_filename,
        months: None,
        visibility: r.visibility,
    };
    rules::edit(&pool, Actor::User(me), rule, &form)
        .await
        .unwrap();

    // Renaming the rule is not a new document: unchanged values must not
    // retire the live window and insert an identical one.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM rule_barcode_window")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
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
