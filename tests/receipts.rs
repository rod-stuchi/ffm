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
        pdf_password: None,
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
        pdf_password: None,
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
        pdf_password: None,
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
