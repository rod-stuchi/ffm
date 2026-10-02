//! `:prune` — byte-identical files in the vault, and removing the trash's
//! copies of them: only from the trash, never the last copy, and a pruned
//! copy is retired like a bill that moved to another row.

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
use ffm::services::prune::{self, PruneError, Refusal};
use ffm::services::{materializer, receipts};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn vault() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ffm-prune-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A household with one tracked bill, materialized for 09/2026.
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
    };
    rules::create(pool, Actor::User(me), &form).await.unwrap();
    materializer::reconcile(pool, d(2026, 9, 21)).await.unwrap();
    let tx: i64 = sqlx::query_scalar("SELECT id FROM transactions WHERE period = '2026-09'")
        .fetch_one(pool)
        .await
        .unwrap();
    (me, tx)
}

/// The same bill uploaded twice: the first goes to the trash, superseded,
/// and the trash then holds an exact copy of the live file. The receipt id
/// of the trashed copy.
async fn replaced_twice(pool: &SqlitePool, root: &Path, me: i64, tx: i64) -> i64 {
    let (slot, _) = receipts::slot_of(pool, tx).await.unwrap().unwrap();
    for _ in 0..2 {
        receipts::store(
            pool,
            root,
            Actor::User(me),
            &slot,
            Kind::Barcode,
            "conta.pdf",
            b"%PDF same bytes",
        )
        .await
        .unwrap();
    }
    sqlx::query_scalar("SELECT id FROM receipts WHERE deleted_at IS NOT NULL")
        .fetch_one(pool)
        .await
        .unwrap()
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

#[tokio::test]
async fn a_trashed_copy_of_a_live_file_is_found_and_pruned() {
    let pool = pool().await;
    let root = vault();
    let (me, tx) = bill(&pool).await;
    let copy = replaced_twice(&pool, &root, me, tx).await;

    let groups = prune::scan(&pool, &root).await.unwrap();
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert!(g.prunable());
    assert_eq!(g.copies.len(), 2);
    // What stays reads first.
    assert!(!g.copies[0].in_trash() && g.copies[1].in_trash());
    let trashed = g.copies[1].rel.clone();
    assert!(trashed.starts_with("rodrigo/.delete/"), "{trashed}");
    // The trash offers it before the prune.
    let (items, _) = trash::list(&pool, Viewer::new(me, true), 50).await.unwrap();
    assert_eq!(items.len(), 1);

    let done = prune::prune(&pool, &root, me, std::slice::from_ref(&trashed))
        .await
        .unwrap();
    assert_eq!(done.files, 1);
    assert!(!root.join(&trashed).exists(), "the copy left the disk");
    assert!(root.join(&g.copies[0].rel).exists(), "its twin stays");

    // Logged as a retirement of that receipt, by the admin.
    let (action, entity_id, note): (String, i64, String) = sqlx::query_as(
        "SELECT action, entity_id, note FROM change WHERE note = ?1 ORDER BY id DESC LIMIT 1",
    )
    .bind(prune::PRUNED)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (action.as_str(), entity_id, note.as_str()),
        ("file_unlink", copy, prune::PRUNED)
    );

    // Retired: not offered by the trash, and never put back.
    let (items, _) = trash::list(&pool, Viewer::new(me, true), 50).await.unwrap();
    assert!(items.is_empty(), "a pruned copy is not in the trash");
    assert!(
        !receipts::restore(&pool, &root, Actor::User(me), copy, None)
            .await
            .unwrap(),
        "a pruned copy cannot be restored"
    );
    // Nothing doubled is left.
    assert!(prune::scan(&pool, &root).await.unwrap().is_empty());
}

#[tokio::test]
async fn only_the_trash_is_pruned_and_never_the_last_copy() {
    let pool = pool().await;
    let root = vault();
    let (me, _) = bill(&pool).await;

    // Two live copies of one file: listed, never offered.
    put(&root, "rodrigo/a/2026/09.pdf", b"live twice");
    put(&root, "rodrigo/b/2026/09.pdf", b"live twice");
    // Two trashed copies of another, with no record anywhere.
    put(&root, "rodrigo/.delete/x/01.pdf", b"trash twice");
    put(&root, "rodrigo/.delete/y/01.pdf", b"trash twice");
    // Noise the scan must not count.
    put(&root, "rodrigo/a/2026/.DS_Store", b"live twice");

    let groups = prune::scan(&pool, &root).await.unwrap();
    assert_eq!(groups.len(), 2);
    // A group with something to prune comes first.
    assert!(groups[0].prunable() && !groups[1].prunable());

    let refused = |r| Err::<prune::Pruned, _>(PruneError::Refused(r));
    let live = "rodrigo/a/2026/09.pdf".to_string();
    assert_eq!(
        format!(
            "{:?}",
            prune::prune(&pool, &root, me, std::slice::from_ref(&live)).await
        ),
        format!("{:?}", refused(Refusal::NotInTrash(live)))
    );
    let sneaky = "rodrigo/.delete/../a/2026/09.pdf".to_string();
    assert!(matches!(
        prune::prune(&pool, &root, me, &[sneaky]).await,
        Err(PruneError::Refused(Refusal::NotInTrash(_)))
    ));
    let both = [
        "rodrigo/.delete/x/01.pdf".to_string(),
        "rodrigo/.delete/y/01.pdf".to_string(),
    ];
    assert!(matches!(
        prune::prune(&pool, &root, me, &both).await,
        Err(PruneError::Refused(Refusal::LastCopy(_)))
    ));
    assert!(
        root.join(&both[0]).exists() && root.join(&both[1]).exists(),
        "a refused batch removes nothing"
    );

    // One of the two goes; the file with no record is logged by its path.
    prune::prune(&pool, &root, me, &both[..1]).await.unwrap();
    assert!(!root.join(&both[0]).exists() && root.join(&both[1]).exists());
    let entity_id: i64 = sqlx::query_scalar("SELECT entity_id FROM change WHERE note = ?1")
        .bind(prune::PRUNED)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(entity_id, 0);
    // The folder the removal emptied goes with it.
    assert!(!root.join("rodrigo/.delete/x").exists());
}
