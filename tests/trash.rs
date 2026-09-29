//! The trash: every deleted row and removed file, each restorable on its own,
//! and `u` still walking back through history around it.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::rules::{self, RuleForm};
use ffm::db::trash::{self, What};
use ffm::db::tx::{self, NewTransaction};
use ffm::domain::money::{Cents, Currency};
use ffm::domain::period::Period;
use ffm::domain::receipt_path::Kind;
use ffm::domain::status::{TxKind, Visibility};
use ffm::domain::viewer::Viewer;
use ffm::services::materializer;
use ffm::services::receipts::{self, Slot};
use sqlx::SqlitePool;
use std::path::PathBuf;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn sep() -> Period {
    Period::parse("2026-09").unwrap()
}

fn vault() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ffm-trash-{}-{:?}",
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

async fn member(pool: &SqlitePool, admin: i64, name: &str) -> i64 {
    let token = auth::issue_link(pool, Actor::User(admin), None, None, 1)
        .await
        .unwrap();
    let sid = auth::accept_invite(pool, &token, name, "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    auth::session(pool, &sid).await.unwrap().unwrap().member.id
}

async fn add(pool: &SqlitePool, who: i64, title: &str) -> i64 {
    tx::add(
        pool,
        Actor::User(who),
        NewTransaction {
            member_id: who,
            period: sep(),
            kind: TxKind::Out,
            title,
            amount: Some(Cents(1000)),
            date: None,
            bucket: None,
            visibility: Visibility::Household,
        },
    )
    .await
    .unwrap()
}

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

    let id: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE rule_id IS NOT NULL LIMIT 1")
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
            period: sep(),
            amount: Some((Cents(34362), Currency::Brl)),
            amount_in_filename: false,
        },
    )
}

async fn store(
    pool: &SqlitePool,
    root: &std::path::Path,
    me: i64,
    slot: &Slot,
    kind: Kind,
    body: &[u8],
) {
    receipts::store(pool, root, Actor::User(me), slot, kind, "f.pdf", body)
        .await
        .unwrap();
}

async fn receipt_id(pool: &SqlitePool, kind: &str, nth: i64) -> i64 {
    sqlx::query_scalar("SELECT id FROM receipts WHERE kind = ? ORDER BY id LIMIT 1 OFFSET ?")
        .bind(kind)
        .bind(nth)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A deleted row and a removed file are listed; a file whose row was deleted
/// is not — the row stands for it, and brings it back.
#[tokio::test]
async fn a_deleted_row_stands_for_its_files() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (occ, slot) = tracked_occurrence(&pool, me).await;
    store(&pool, &root, me, &slot, Kind::Barcode, b"bill").await;
    store(&pool, &root, me, &slot, Kind::Scheduled, b"promise").await;
    receipts::soft_delete(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Scheduled,
        "manual",
        None,
    )
    .await
    .unwrap();
    let one_off = add(&pool, me, "Mercado").await;
    tx::soft_delete(&pool, Actor::User(me), one_off)
        .await
        .unwrap();

    let (items, more) = trash::list(&pool, Viewer::new(me, true), 200)
        .await
        .unwrap();
    assert!(!more);
    let whats: Vec<_> = items.iter().map(|i| (i.tx_id, i.what.clone())).collect();
    assert!(whats.contains(&(one_off, What::Row { files: 0 })));
    assert!(whats.contains(&(
        occ,
        What::File {
            kind: "scheduled".into(),
            replaced: false
        }
    )));
    assert_eq!(items.len(), 2, "the live bill is not in the trash");
    assert_eq!(
        items
            .iter()
            .find(|i| i.tx_id == one_off)
            .unwrap()
            .deleted_by
            .as_deref(),
        Some("rodrigo")
    );

    // Deleting the occurrence moves its live bill with it.
    tx::soft_delete(&pool, Actor::User(me), occ).await.unwrap();
    receipts::retire_with_row(&pool, &root, Actor::User(me), occ)
        .await
        .unwrap();
    let (items, _) = trash::list(&pool, Viewer::new(me, true), 200)
        .await
        .unwrap();
    let occ_items: Vec<_> = items.iter().filter(|i| i.tx_id == occ).collect();
    assert_eq!(
        occ_items.len(),
        1,
        "one entry for the row, none for its files"
    );
    assert_eq!(occ_items[0].what, What::Row { files: 1 });
}

/// Scoped by ledger: what was deleted from a ledger you may write, whoever
/// deleted it. An admin previewing as a member sees only their own.
#[tokio::test]
async fn the_trash_is_read_by_ledger_not_by_who_deleted() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ana = member(&pool, me, "ana").await;
    let hers = add(&pool, ana, "Padaria").await;
    let mine = add(&pool, me, "Mercado").await;
    // The admin deletes a row from Ana's ledger.
    tx::soft_delete(&pool, Actor::User(me), hers).await.unwrap();
    tx::soft_delete(&pool, Actor::User(me), mine).await.unwrap();

    let ids = |v: Viewer| {
        let pool = pool.clone();
        async move {
            let (items, _) = trash::list(&pool, v, 200).await.unwrap();
            let mut ids: Vec<i64> = items.into_iter().map(|i| i.id).collect();
            ids.sort();
            ids
        }
    };
    assert_eq!(ids(Viewer::new(ana, false)).await, vec![hers]);
    assert_eq!(ids(Viewer::new(me, true)).await, vec![hers, mine]);
    let previewing = Viewer {
        member_id: me,
        role_admin: true,
        preview: true,
    };
    assert_eq!(ids(previewing).await, vec![mine]);
}

/// A restore from the trash is an ordinary action: `u` straight afterwards
/// deletes it again, and then walks on through history as before.
#[tokio::test]
async fn u_undoes_a_restore_from_the_trash_first() {
    let pool = pool().await;
    let me = household(&pool).await;
    let a = add(&pool, me, "A").await;
    let b = add(&pool, me, "B").await;
    let c = add(&pool, me, "C").await;
    for id in [a, b, c] {
        tx::soft_delete(&pool, Actor::User(me), id).await.unwrap();
    }

    // B, out of order.
    assert!(
        tx::restore(&pool, Actor::User(me), b, None)
            .await
            .unwrap()
            .is_some()
    );
    let (items, _) = trash::list(&pool, Viewer::new(me, true), 200)
        .await
        .unwrap();
    assert!(items.iter().all(|i| i.id != b), "B left the trash");

    let mut walked = Vec::new();
    for _ in 0..4 {
        let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
        walked.push((u.action.clone(), u.entity_id));
        assert!(tx::undo(&pool, Actor::User(me), &u).await.unwrap());
    }
    assert_eq!(
        walked,
        vec![
            ("restore".into(), b),
            ("delete".into(), c),
            ("delete".into(), b),
            ("delete".into(), a),
        ]
    );
}

/// Picking an older version of a file out of the trash swaps it with the
/// current one, which goes to the trash in its stead — and `u` walks back
/// through both steps.
#[tokio::test]
async fn restoring_an_older_file_swaps_it_with_the_live_one() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;
    store(&pool, &root, me, &slot, Kind::Barcode, b"old").await;
    store(&pool, &root, me, &slot, Kind::Barcode, b"new").await;
    let old = receipt_id(&pool, "barcode", 0).await;
    let live = root.join(format!(
        "{}/consumo/energia/2026/09_barcode.pdf",
        slot.vault
    ));
    assert_eq!(std::fs::read(&live).unwrap(), b"new");

    let (items, _) = trash::list(&pool, Viewer::new(me, true), 200)
        .await
        .unwrap();
    assert_eq!(
        items.iter().find(|i| i.id == old).unwrap().what,
        What::File {
            kind: "barcode".into(),
            replaced: true
        }
    );

    assert!(
        receipts::restore_swapping(&pool, &root, Actor::User(me), old, None, Some(&slot))
            .await
            .unwrap()
    );
    assert_eq!(std::fs::read(&live).unwrap(), b"old");
    let (items, _) = trash::list(&pool, Viewer::new(me, true), 200)
        .await
        .unwrap();
    assert_eq!(items.len(), 1, "the newer file is in the trash now");
    assert_ne!(items[0].id, old);

    // `u`: first the restore goes back, then the newer file comes back.
    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_eq!((u.action.as_str(), u.entity_id), ("file_upload", old));
    receipts::soft_delete(
        &pool,
        &root,
        Actor::User(me),
        &slot,
        Kind::Barcode,
        "manual",
        Some(u.change_id),
    )
    .await
    .unwrap();
    assert!(!live.exists());
    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_eq!(u.action, "file_delete");
    assert!(
        receipts::restore_swapping(
            &pool,
            &root,
            Actor::User(me),
            u.entity_id,
            Some(u.change_id),
            Some(&slot)
        )
        .await
        .unwrap()
    );
    assert_eq!(std::fs::read(&live).unwrap(), b"new");
}

/// `u` reaching a delete whose slot has since been filled swaps instead of
/// failing — it used to refuse "occupied", and since `u` never skips a change
/// it could not undo, every `u` after it refused the same way. The file it
/// moves aside is part of that undo, not a new step to swap back.
#[tokio::test]
async fn u_into_a_taken_slot_swaps_and_moves_on() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;
    store(&pool, &root, me, &slot, Kind::Barcode, b"first").await;
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
    let first = receipt_id(&pool, "barcode", 0).await;
    let removed: i64 = sqlx::query_scalar(
        "SELECT id FROM change WHERE action = 'file_delete' AND entity_id = ? ORDER BY id DESC",
    )
    .bind(first)
    .fetch_one(&pool)
    .await
    .unwrap();
    store(&pool, &root, me, &slot, Kind::Barcode, b"second").await;
    let live = root.join(format!(
        "{}/consumo/energia/2026/09_barcode.pdf",
        slot.vault
    ));

    assert!(
        receipts::restore(&pool, &root, Actor::User(me), first, Some(removed))
            .await
            .is_err(),
        "the plain restore still refuses to overwrite"
    );
    assert!(
        receipts::restore_swapping(
            &pool,
            &root,
            Actor::User(me),
            first,
            Some(removed),
            Some(&slot)
        )
        .await
        .unwrap()
    );
    assert_eq!(std::fs::read(&live).unwrap(), b"first");

    // The displacement points at the undo it belongs to, so `u` does not
    // pick it next and swap the two back.
    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_ne!(u.action, "file_delete");
}

/// A removed file whose copy is gone from the trash on disk — moved by hand —
/// must not cost the row its live file: the swap needs something to put in
/// its place, and without it the restore refuses and nothing moves.
#[tokio::test]
async fn a_swap_with_nothing_on_disk_leaves_the_live_file_alone() {
    let pool = pool().await;
    let root = vault();
    let me = household(&pool).await;
    let (_, slot) = tracked_occurrence(&pool, me).await;
    store(&pool, &root, me, &slot, Kind::Receipt, b"old").await;
    store(&pool, &root, me, &slot, Kind::Receipt, b"new").await;
    let old = receipt_id(&pool, "receipt", 0).await;
    // Somebody tidies the trash by hand.
    let trash_dir = root.join(format!("{}/.delete", slot.vault));
    std::fs::remove_dir_all(&trash_dir).unwrap();

    assert!(
        receipts::restore_swapping(&pool, &root, Actor::User(me), old, None, Some(&slot))
            .await
            .is_err()
    );
    let live = root.join(format!("{}/consumo/energia/2026/09.pdf", slot.vault));
    assert_eq!(std::fs::read(&live).unwrap(), b"new");
    let live_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM receipts WHERE kind = 'receipt' AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(live_rows, 1);
}
