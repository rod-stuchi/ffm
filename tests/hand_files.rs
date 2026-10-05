//! Files changed by hand in the vault: `receipts::reconcile_row` brings the
//! record in line with the disk, and logs what it found, as the system.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::rules::{self, RuleForm};
use ffm::db::trash;
use ffm::domain::money::Cents;
use ffm::domain::receipt_path::Kind;
use ffm::domain::status::{TxKind, Visibility};
use ffm::domain::viewer::Viewer;
use ffm::services::{materializer, receipts};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn vault() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ffm-hand-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A household with one tracked bill, materialized for 09/2026; the row's id.
async fn bill(pool: &SqlitePool) -> (i64, i64) {
    let sid = auth::setup(
        pool,
        auth::NewHousehold {
            household_name: "Casa",
            currency: "BRL",
            language: "en",
            admin_name: "rodrigo",
            password: "abcd1234",
            theme: "ffm",
            line_numbers: "relative",
            device_label: "test",
        },
    )
    .await
    .unwrap();
    let me = auth::session(pool, &sid).await.unwrap().unwrap().member.id;
    let form = RuleForm {
        member_id: me,
        kind: TxKind::Out,
        title: "Energia".into(),
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
    };
    rules::create(pool, Actor::User(me), &form).await.unwrap();
    materializer::reconcile(pool, d(2026, 9, 21)).await.unwrap();
    let tx: i64 = sqlx::query_scalar("SELECT id FROM transactions WHERE period = '2026-09'")
        .fetch_one(pool)
        .await
        .unwrap();
    (me, tx)
}

/// Where the row's receipt goes, from the same slot the pane builds.
async fn receipt_path(pool: &SqlitePool, root: &Path, tx: i64) -> PathBuf {
    let (slot, _) = receipts::slot_of(pool, tx).await.unwrap().unwrap();
    let rel = ffm::domain::receipt_path::file(
        &ffm::domain::receipt_path::Taxonomy {
            vault: &slot.vault,
            bucket: &slot.bucket,
            bill_name: &slot.bill_name,
            instance: slot.instance.as_deref(),
        },
        slot.period,
        Kind::Receipt,
        "pdf",
        None,
    );
    root.join(rel)
}

async fn system_notes(pool: &SqlitePool) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT action, note FROM change WHERE source = 'system' AND entity = 'file' ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_receipt_filed_by_hand_is_recorded_once() {
    let pool = pool().await;
    let root = vault();
    let (_, tx) = bill(&pool).await;
    let path = receipt_path(&pool, &root, tx).await;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"%PDF").unwrap();

    let r = receipts::reconcile_row(&pool, &root, tx).await.unwrap();
    assert_eq!((r.found, r.removed), (1, 0));
    assert_eq!(
        system_notes(&pool).await,
        vec![("file_upload".into(), receipts::FOUND_BY_HAND.into())]
    );
    // The record now agrees with the disk: a second pass writes nothing.
    assert!(
        !receipts::reconcile_row(&pool, &root, tx)
            .await
            .unwrap()
            .changed()
    );
}

#[tokio::test]
async fn a_receipt_removed_by_hand_is_recorded_and_not_offered_by_the_trash() {
    let pool = pool().await;
    let root = vault();
    let (me, tx) = bill(&pool).await;
    let (slot, _) = receipts::slot_of(&pool, tx).await.unwrap().unwrap();
    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "r.pdf",
        b"%PDF",
    )
    .await
    .unwrap();
    // An upload through the app: the record already agrees, nothing to log.
    assert!(
        !receipts::reconcile_row(&pool, &root, tx)
            .await
            .unwrap()
            .changed()
    );

    std::fs::remove_file(receipt_path(&pool, &root, tx).await).unwrap();
    let r = receipts::reconcile_row(&pool, &root, tx).await.unwrap();
    assert_eq!((r.found, r.removed), (0, 1));
    assert_eq!(
        system_notes(&pool).await,
        vec![("file_delete".into(), receipts::REMOVED_BY_HAND.into())]
    );
    // Nothing went through the trash, so there is nothing there to restore.
    let (items, _) = trash::list(&pool, Viewer::new(me, true), 50).await.unwrap();
    assert!(
        items.is_empty(),
        "a file deleted by hand is not in the trash"
    );
}

#[tokio::test]
async fn a_file_left_where_it_was_stored_is_not_read_as_removed() {
    let pool = pool().await;
    let root = vault();
    let (me, tx) = bill(&pool).await;
    let (slot, _) = receipts::slot_of(&pool, tx).await.unwrap().unwrap();
    receipts::store(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "r.pdf",
        b"%PDF",
    )
    .await
    .unwrap();
    // A bucket edit moves the path the row would compute, never the file —
    // the file is still where its row says it was stored.
    sqlx::query("UPDATE transactions SET bucket = 'Utilidades' WHERE id = ?1")
        .bind(tx)
        .execute(&pool)
        .await
        .unwrap();
    let r = receipts::reconcile_row(&pool, &root, tx).await.unwrap();
    assert_eq!((r.found, r.removed), (0, 0));
    assert!(system_notes(&pool).await.is_empty());
}
