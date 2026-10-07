//! A row's files follow the path it computes — one builder, and a move when
//! that path changes under them.
//!
//! The production case these pin (v0.0.5, Damha III): a rule's bucket was
//! renamed, uploads were written where the RULE said, the ledger looked where
//! the ROW said, and no upload ever showed. And the file already there was
//! left behind at the old path.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::rules::{self, RuleForm};
use ffm::db::tx;
use ffm::domain::money::Cents;
use ffm::domain::receipt_path::Kind;
use ffm::domain::status::{TxKind, Visibility};
use ffm::services::{materializer, receipts, vault};
use sqlx::SqlitePool;
use std::path::PathBuf;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ffm-refile-{}-{:?}",
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

async fn second_member(pool: &SqlitePool, admin: i64) -> i64 {
    let token = auth::issue_link(pool, Actor::User(admin), None, None, 1)
        .await
        .unwrap();
    let sid = auth::accept_invite(
        pool,
        &token,
        "natália",
        "hunter2hunter2",
        "pt-BR",
        "ffm",
        1,
        "x",
    )
    .await
    .unwrap();
    auth::session(pool, &sid).await.unwrap().unwrap().member.id
}

fn form(me: i64, bucket: &str, in_filename: bool) -> RuleForm {
    RuleForm {
        member_id: me,
        kind: TxKind::Out,
        title: "Damha III".into(),
        amount: Some(Cents(55000)),
        cadence_months: 1,
        due_day: Some(10),
        start_date: d(2026, 9, 1),
        end_date: None,
        total_installments: None,
        carry_forward_amount: true,
        tracked: true,
        bucket: Some(bucket.into()),
        bill_name: Some("Damha III".into()),
        instance: Some("D 19".into()),
        no_barcode: false,
        spans_period: false,
        window_size: None,
        window_start: None,
        varies: false,
        estimate_basis: "lowest".into(),
        estimate_window: 3,
        amount_in_filename: in_filename,
        months: None,
        visibility: Visibility::Household,
        pdf_password: None,
    }
}

/// A tracked rule with September's occurrence; returns (rule id, row id).
async fn rule_with_row(pool: &SqlitePool, me: i64, f: &RuleForm) -> (i64, i64) {
    let rule = rules::create(pool, Actor::User(me), f).await.unwrap();
    materializer::reconcile(pool, d(2026, 9, 21)).await.unwrap();
    let row: i64 = sqlx::query_scalar("SELECT id FROM transactions WHERE rule_id = ?1")
        .bind(rule)
        .fetch_one(pool)
        .await
        .unwrap();
    (rule, row)
}

async fn store(pool: &SqlitePool, root: &std::path::Path, me: i64, row: i64, kind: Kind) -> String {
    let (slot, _) = receipts::slot_of(pool, row).await.unwrap().unwrap();
    receipts::store(
        pool,
        root,
        Actor::User(me),
        &slot,
        kind,
        "bill.pdf",
        b"%PDF-1",
    )
    .await
    .unwrap()
}

/// Where the row looks — the one slot every reader uses.
async fn shows(pool: &SqlitePool, _root: &std::path::Path, row: i64) -> receipts::Slot {
    receipts::slot_of(pool, row).await.unwrap().unwrap().0
}

async fn rename_bucket(pool: &SqlitePool, me: i64, rule: i64, bucket: &str) {
    rules::edit_reporting(pool, Actor::User(me), rule, &form(me, bucket, false))
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn the_path_comes_from_the_rule_whatever_the_row_says() {
    // The row's own bucket is a label for grouping; where its files live is
    // the rule's. The ledger used to read the row's and uploads the rule's.
    let pool = pool().await;
    let me = household(&pool).await;
    let (_, row) = rule_with_row(&pool, me, &form(me, "Condomínio", false)).await;
    sqlx::query("UPDATE transactions SET bucket = 'Outra coisa' WHERE id = ?1")
        .bind(row)
        .execute(&pool)
        .await
        .unwrap();

    let (slot, _) = receipts::slot_of(&pool, row).await.unwrap().unwrap();
    assert_eq!(slot.bucket, "Condomínio");
}

#[tokio::test]
async fn renaming_a_rules_bucket_moves_its_files_and_its_rows_follow() {
    let pool = pool().await;
    let root = root();
    let me = household(&pool).await;
    let (rule, row) = rule_with_row(&pool, me, &form(me, "Condomínio", false)).await;
    let old = store(&pool, &root, me, row, Kind::Barcode).await;
    assert_eq!(old, "rodrigo/condomínio/damha_iii/d_19/2026/09_barcode.pdf");

    rename_bucket(&pool, me, rule, "Moradia").await;
    let refiled = receipts::refile_rule(&pool, &root, rule).await.unwrap();

    let new = "rodrigo/moradia/damha_iii/d_19/2026/09_barcode.pdf";
    assert_eq!(refiled.moved, vec![(old.clone(), new.to_string())]);
    assert!(root.join(new).exists(), "the bill is at the new path");
    assert!(!root.join(&old).exists(), "and not left at the old one");
    assert!(
        !root.join("rodrigo/condomínio").exists(),
        "the folders the move emptied are gone"
    );

    let slot = shows(&pool, &root, row).await;
    let present = receipts::presence(&pool, &root, &slot).await.unwrap();
    assert_eq!(
        present.bill.as_deref(),
        Some("pdf"),
        "the row shows its bill"
    );

    let bucket: String = sqlx::query_scalar("SELECT bucket FROM transactions WHERE id = ?1")
        .bind(row)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(bucket, "Moradia", "the row's own bucket followed the rule");

    // The upload after a rename lands where the row looks — the bug itself.
    let again = store(&pool, &root, me, row, Kind::Barcode).await;
    assert_eq!(again, new);
    let present = receipts::presence(&pool, &root, &slot).await.unwrap();
    assert_eq!(present.bill.as_deref(), Some("pdf"));
}

#[tokio::test]
async fn a_move_never_overwrites_what_is_already_there() {
    let pool = pool().await;
    let root = root();
    let me = household(&pool).await;
    let (rule, row) = rule_with_row(&pool, me, &form(me, "Condomínio", false)).await;
    let old = store(&pool, &root, me, row, Kind::Barcode).await;

    let new = root.join("rodrigo/moradia/damha_iii/d_19/2026/09_barcode.pdf");
    std::fs::create_dir_all(new.parent().unwrap()).unwrap();
    std::fs::write(&new, b"somebody else's").unwrap();

    rename_bucket(&pool, me, rule, "Moradia").await;
    let refiled = receipts::refile_rule(&pool, &root, rule).await.unwrap();

    assert!(refiled.moved.is_empty());
    assert_eq!(refiled.blocked, vec![old.clone()]);
    assert_eq!(std::fs::read(&new).unwrap(), b"somebody else's");
    assert!(
        root.join(&old).exists(),
        "the blocked file stays where it was"
    );
}

#[tokio::test]
async fn a_receipt_named_with_its_amount_follows_a_new_amount() {
    let pool = pool().await;
    let root = root();
    let me = household(&pool).await;
    let (_, row) = rule_with_row(&pool, me, &form(me, "Moradia", true)).await;
    let old = store(&pool, &root, me, row, Kind::Receipt).await;
    assert!(old.ends_with("09_brl550.00.pdf"), "{old}");

    sqlx::query("UPDATE transactions SET amount_cents = 56611 WHERE id = ?1")
        .bind(row)
        .execute(&pool)
        .await
        .unwrap();
    let refiled = receipts::refile_row(&pool, &root, row).await.unwrap();

    assert_eq!(refiled.moved.len(), 1);
    assert!(refiled.moved[0].1.ends_with("09_brl566.11.pdf"));
    let slot = shows(&pool, &root, row).await;
    let present = receipts::presence(&pool, &root, &slot).await.unwrap();
    assert_eq!(present.receipt.as_deref(), Some("pdf"), "still paid");
}

#[tokio::test]
async fn a_row_given_to_another_member_takes_its_files_to_their_folder() {
    let pool = pool().await;
    let root = root();
    let me = household(&pool).await;
    let other = second_member(&pool, me).await;
    let (_, row) = rule_with_row(&pool, me, &form(me, "Moradia", false)).await;
    let old = store(&pool, &root, me, row, Kind::Barcode).await;
    assert!(old.starts_with("rodrigo/"));

    tx::reattribute(&pool, Actor::User(me), row, other, None)
        .await
        .unwrap();
    let refiled = receipts::refile_row(&pool, &root, row).await.unwrap();

    assert_eq!(refiled.moved.len(), 1);
    assert!(
        refiled.moved[0].1.starts_with("natália/"),
        "{:?}",
        refiled.moved
    );
    assert!(root.join(&refiled.moved[0].1).exists());
}

#[tokio::test]
async fn a_copy_deleted_before_a_rename_is_restored_where_the_row_looks_now() {
    let pool = pool().await;
    let root = root();
    let me = household(&pool).await;
    let (rule, row) = rule_with_row(&pool, me, &form(me, "Condomínio", false)).await;
    store(&pool, &root, me, row, Kind::Barcode).await;
    let slot = shows(&pool, &root, row).await;
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
    let receipt: i64 = sqlx::query_scalar("SELECT id FROM receipts WHERE transaction_id = ?1")
        .bind(row)
        .fetch_one(&pool)
        .await
        .unwrap();

    rename_bucket(&pool, me, rule, "Moradia").await;
    receipts::restore(&pool, &root, Actor::User(me), receipt, None)
        .await
        .unwrap();

    let new = "rodrigo/moradia/damha_iii/d_19/2026/09_barcode.pdf";
    assert!(
        root.join(new).exists(),
        "restored at the row's current path"
    );
    let path: String = sqlx::query_scalar("SELECT path FROM receipts WHERE id = ?1")
        .bind(receipt)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(path, new, "and its record says so");
    let present = receipts::presence(&pool, &root, &shows(&pool, &root, row).await)
        .await
        .unwrap();
    assert_eq!(present.bill.as_deref(), Some("pdf"));
}

#[tokio::test]
async fn the_startup_repair_re_files_a_vault_left_by_v0_0_5_and_then_does_nothing() {
    let pool = pool().await;
    let root = root();
    let me = household(&pool).await;
    let (rule, row) = rule_with_row(&pool, me, &form(me, "Condomínio", false)).await;
    let old = store(&pool, &root, me, row, Kind::Barcode).await;

    // v0.0.5's rename: the rule changes and nothing else does.
    sqlx::query("UPDATE recurring_rules SET bucket = 'Moradia__' WHERE id = ?1")
        .bind(rule)
        .execute(&pool)
        .await
        .unwrap();

    let (moved, blocked) = vault::refile_all(&pool, &root).await.unwrap();
    assert_eq!((moved, blocked.len()), (1, 0));
    assert!(!root.join(&old).exists());
    assert!(
        root.join("rodrigo/moradia/damha_iii/d_19/2026/09_barcode.pdf")
            .exists()
    );

    let (moved, blocked) = vault::refile_all(&pool, &root).await.unwrap();
    assert_eq!((moved, blocked.len()), (0, 0), "idempotent");
}
