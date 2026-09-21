//! `gl` — reading back what the writer has been recording all along.
//!
//! Phase 3 built the write path before anything could mutate; this is the
//! other half finally being looked at. The tests are mostly about *scope*:
//! which entries belong to a row, which to its rule, and which to neither.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::{self, Actor, Filter};
use ffm::db::rules::{self, RuleForm};
use ffm::db::tx;
use ffm::domain::money::Cents;
use ffm::domain::status::{TxKind, Visibility};
use ffm::services::materializer;
use sqlx::SqlitePool;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
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

fn form(member_id: i64) -> RuleForm {
    RuleForm {
        member_id,
        kind: TxKind::Out,
        title: "Energia · Enel SP".into(),
        amount: Some(Cents(34062)),
        cadence_months: 1,
        due_day: Some(11),
        start_date: d(2026, 9, 1),
        end_date: None,
        total_installments: None,
        carry_forward_amount: true,
        tracked: false,
        bucket: Some("Consumo".into()),
        bill_name: None,
        instance: None,
        no_barcode: false,
        spans_period: false,
        window_size: None,
        window_start: None,
        varies: false,
        estimate_basis: "lowest".into(),
        estimate_window: 3,
        amount_in_filename: false,
        visibility: Visibility::Household,
    }
}

/// A rule, its September occurrence, and one edit to each.
async fn history_fixture(pool: &SqlitePool) -> (i64, i64, i64) {
    let me = household(pool).await;
    let rule = rules::create(pool, Actor::User(me), &form(me))
        .await
        .unwrap();
    materializer::reconcile(pool, d(2026, 9, 21)).await.unwrap();

    let tx_id: i64 = sqlx::query_scalar("SELECT id FROM transactions LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();

    let mut edited = form(me);
    edited.due_day = Some(18);
    rules::edit(pool, Actor::User(me), rule, &edited)
        .await
        .unwrap();
    tx::soft_delete(pool, Actor::User(me), tx_id).await.unwrap();

    (me, rule, tx_id)
}

#[tokio::test]
async fn the_chips_narrow_to_the_row_the_rule_or_both() {
    let pool = pool().await;
    let (_, rule, tx_id) = history_fixture(&pool).await;

    let row = changes::history(&pool, tx_id, Some(rule), Filter::Row, &[])
        .await
        .unwrap();
    let just_rule = changes::history(&pool, tx_id, Some(rule), Filter::Rule, &[])
        .await
        .unwrap();
    let all = changes::history(&pool, tx_id, Some(rule), Filter::All, &[])
        .await
        .unwrap();

    // The row: materialized, then deleted.
    assert_eq!(
        row.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(),
        ["delete", "materialize"]
    );
    // The rule: created, then edited.
    assert_eq!(
        just_rule
            .iter()
            .map(|e| e.action.as_str())
            .collect::<Vec<_>>(),
        ["rule_edit", "rule_create"]
    );
    assert_eq!(all.len(), row.len() + just_rule.len());

    // Rule-level entries sit inline in date order and are TAGGED, so the two
    // stories are never confused where they sit side by side.
    assert!(all.iter().any(|e| e.is_rule));
    assert!(all.iter().any(|e| !e.is_rule));
}

#[tokio::test]
async fn the_newest_entry_is_first_and_an_undo_points_at_what_it_reversed() {
    let pool = pool().await;
    let (me, rule, tx_id) = history_fixture(&pool).await;

    let deleted = changes::history(&pool, tx_id, Some(rule), Filter::Row, &[])
        .await
        .unwrap()[0]
        .id;

    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    tx::undo(&pool, Actor::User(me), &u).await.unwrap();

    let after = changes::history(&pool, tx_id, Some(rule), Filter::Row, &[])
        .await
        .unwrap();
    assert_eq!(after[0].action, "restore", "newest first");
    assert_eq!(after[0].undo_of, Some(deleted));

    // Nothing in the log is ever updated or deleted: the delete is still
    // there, underneath its own reversal.
    assert!(
        after
            .iter()
            .any(|e| e.id == deleted && e.action == "delete")
    );
}

#[tokio::test]
async fn a_materialized_entry_has_no_actor_and_says_which_thing_did_it() {
    let pool = pool().await;
    let (_, rule, tx_id) = history_fixture(&pool).await;

    let row = changes::history(&pool, tx_id, Some(rule), Filter::Row, &[])
        .await
        .unwrap();
    let m = row.iter().find(|e| e.action == "materialize").unwrap();
    assert_eq!(m.actor, None);
    assert_eq!(m.source, "materializer");
}

#[tokio::test]
async fn an_edit_carries_its_diff_in_storage_form() {
    let pool = pool().await;
    let (_, rule, tx_id) = history_fixture(&pool).await;

    let entries = changes::history(&pool, tx_id, Some(rule), Filter::Rule, &[])
        .await
        .unwrap();
    let edit = entries.iter().find(|e| e.action == "rule_edit").unwrap();
    let due = edit.fields.iter().find(|f| f.field == "due_day").unwrap();

    // Storage-neutral, and the KIND travels with it — so the pane formats it
    // the way the app does without inferring anything from the field's name,
    // and a stored amount never depends on the household's current currency.
    assert_eq!(
        (due.old.as_deref(), due.new.as_deref()),
        (Some("11"), Some("18"))
    );
    assert_eq!(due.kind, "day");

    // And delete carries none: it is the row's existence that changed.
    let row = changes::history(&pool, tx_id, Some(rule), Filter::Row, &[])
        .await
        .unwrap();
    let deleted = row.iter().find(|e| e.action == "delete").unwrap();
    assert!(deleted.fields.is_empty());
}

#[tokio::test]
async fn a_row_with_nothing_recorded_returns_an_empty_history_not_an_error() {
    let pool = pool().await;
    let me = household(&pool).await;

    // Filed straight into the database, the way a migration or a hand-edit
    // would. An empty history is an answer, and the pane says so.
    sqlx::query(
        "INSERT INTO transactions (member_id, period, kind, title, sort_key, created_at)
         VALUES (?1, '2026-09', 'out', 'Filed by hand', 'z', '2026-09-01T00:00:00Z')",
    )
    .bind(me)
    .execute(&pool)
    .await
    .unwrap();
    let id: i64 = sqlx::query_scalar("SELECT max(id) FROM transactions")
        .fetch_one(&pool)
        .await
        .unwrap();

    let history = changes::history(&pool, id, None, Filter::Row, &[])
        .await
        .unwrap();
    assert!(history.is_empty());
}

#[tokio::test]
async fn a_file_entry_belongs_to_the_row_it_is_attached_to() {
    let pool = pool().await;
    let (me, rule, tx_id) = history_fixture(&pool).await;

    // A file change names a `receipts` row rather than the transaction, so the
    // pane passes the ids that belong to this occurrence — without them a
    // file entry would either go missing or, worse, show up on every row.
    sqlx::query(
        "INSERT INTO receipts (transaction_id, kind, path, ext, original_filename,
                               uploaded_by, uploaded_at)
         VALUES (?1, 'receipt', 'a/b/09.pdf', 'pdf', 'scan.pdf', ?2, '2026-09-20T10:00:00Z')",
    )
    .bind(tx_id)
    .bind(me)
    .execute(&pool)
    .await
    .unwrap();
    let receipt_id: i64 = sqlx::query_scalar("SELECT max(id) FROM receipts")
        .fetch_one(&pool)
        .await
        .unwrap();

    let mut t = pool.begin().await.unwrap();
    changes::Change::file(Actor::User(me), changes::FileAction::Upload, receipt_id)
        .write(&mut t)
        .await
        .unwrap();
    t.commit().await.unwrap();

    let with = changes::history(&pool, tx_id, Some(rule), Filter::Row, &[receipt_id])
        .await
        .unwrap();
    assert!(with.iter().any(|e| e.action == "file_upload"));

    let without = changes::history(&pool, tx_id, Some(rule), Filter::Row, &[])
        .await
        .unwrap();
    assert!(!without.iter().any(|e| e.action == "file_upload"));
}
