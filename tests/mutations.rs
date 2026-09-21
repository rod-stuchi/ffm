//! Mutations: add, soft delete, restore, undo — and the log entries that must
//! accompany every one of them.
//!
//! The contract these serve is "the row and its record commit together, and
//! nothing is ever destroyed". Both halves are asserted here rather than
//! assumed from the fact that the code calls the right function.

mod common;

use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::ledger;
use ffm::db::tx::{self, NewTransaction};
use ffm::domain::money::{Cents, Currency};
use ffm::domain::period::Period;
use ffm::domain::status::{TxKind, Visibility};
use ffm::domain::viewer::Viewer;
use ffm::ui::row::RowCtx;
use sqlx::SqlitePool;

fn ctx() -> RowCtx {
    RowCtx {
        today: chrono::NaiveDate::from_ymd_opt(2026, 9, 21).unwrap(),
        sort: Default::default(),
        currency: Currency::Brl,
        show_recurring: true,
        show_delta: true,
        narrow: false,
    }
}

fn sep() -> Period {
    Period::parse("2026-09").unwrap()
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

async fn add(pool: &SqlitePool, me: i64, kind: TxKind, title: &str, cents: i64) -> i64 {
    tx::add(
        pool,
        Actor::User(me),
        NewTransaction {
            member_id: me,
            period: sep(),
            kind,
            title,
            amount: Some(Cents(cents)),
            date: None,
            bucket: None,
            visibility: Visibility::Household,
        },
    )
    .await
    .unwrap()
}

async fn titles(pool: &SqlitePool, me: i64) -> Vec<String> {
    let member = ledger::members(pool)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == me)
        .unwrap();
    ledger::pane(pool, &member, sep(), ctx(), Viewer::new(me, true))
        .await
        .unwrap()
        .rows
        .into_iter()
        .map(|r| r.title)
        .collect()
}

#[tokio::test]
async fn a_spend_appends_and_an_income_goes_to_the_top() {
    let pool = pool().await;
    let me = household(&pool).await;

    add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    add(&pool, me, TxKind::Out, "Farmácia", 8740).await;
    add(&pool, me, TxKind::In, "Salário", 980_000).await;

    // A spend joins the bottom of the list, where a household adds to one.
    // Income goes to the top, which is where they read what came in — a
    // starting position, not a pin.
    assert_eq!(titles(&pool, me).await, ["Salário", "Mercado", "Farmácia"]);

    // And nothing was renumbered to achieve it: three inserts, three keys, all
    // distinct and ordered.
    let keys: Vec<String> =
        sqlx::query_scalar("SELECT sort_key FROM transactions ORDER BY sort_key")
            .fetch_all(&pool)
            .await
            .unwrap();
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(keys, sorted);
}

#[tokio::test]
async fn adding_a_row_records_what_it_created() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;

    let fields: Vec<(String, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT f.field, f.old_value, f.new_value, f.kind
           FROM change_field f JOIN change c ON c.id = f.change_id
          WHERE c.entity_id = ? AND c.action = 'create' ORDER BY f.id",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .unwrap();

    // `gl` can answer "where did this row come from" without inferring it from
    // the row's current contents — and money is logged in minor units.
    assert_eq!(fields[0].0, "title");
    assert_eq!(fields[0].2.as_deref(), Some("Mercado"));
    assert_eq!(
        fields[1],
        ("amount".into(), None, Some("65890".into()), "money".into())
    );
}

#[tokio::test]
async fn one_delete_and_one_undo_leave_exactly_one_row() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;

    tx::soft_delete(&pool, Actor::User(me), id).await.unwrap();
    assert!(titles(&pool, me).await.is_empty(), "gone from the ledger");
    // Gone from the ledger, still in the database. Nothing is ever destroyed.
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM transactions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);

    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_eq!(u.action, "delete");
    assert!(tx::undo(&pool, Actor::User(me), &u).await.unwrap());

    // The earlier "restore inserts a superseding row" design would leave two
    // rows here. It does not: the mark is cleared on the same row.
    assert_eq!(titles(&pool, me).await, ["Mercado"]);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM transactions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn undo_walks_back_through_history_instead_of_toggling_one_change() {
    let pool = pool().await;
    let me = household(&pool).await;
    let first = add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    let second = add(&pool, me, TxKind::Out, "Farmácia", 8740).await;
    tx::soft_delete(&pool, Actor::User(me), first)
        .await
        .unwrap();

    // u #1 reverses the delete.
    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_eq!((u.action.as_str(), u.entity_id), ("delete", first));
    tx::undo(&pool, Actor::User(me), &u).await.unwrap();

    // u #2 must not re-delete it — that change has been undone and is spent.
    // It reaches past it, to the add before it.
    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_eq!((u.action.as_str(), u.entity_id), ("create", second));
    tx::undo(&pool, Actor::User(me), &u).await.unwrap();
    assert_eq!(titles(&pool, me).await, ["Mercado"]);

    // u #3 reaches the first add.
    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_eq!((u.action.as_str(), u.entity_id), ("create", first));
    tx::undo(&pool, Actor::User(me), &u).await.unwrap();
    assert!(titles(&pool, me).await.is_empty());

    // And every reversal is its own entry pointing at what it reversed.
    let undos: Vec<(i64, Option<i64>)> =
        sqlx::query_as("SELECT id, undo_of FROM change WHERE undo_of IS NOT NULL ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(undos.len(), 3);
}

#[tokio::test]
async fn undo_is_scoped_to_the_member_who_asks() {
    let pool = pool().await;
    let me = household(&pool).await;
    let token = auth::issue_link(&pool, Actor::User(me), None, None, 1)
        .await
        .unwrap();
    let sid = auth::accept_invite(&pool, &token, "ana", "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    let them = auth::session(&pool, &sid).await.unwrap().unwrap().member.id;

    add(&pool, me, TxKind::Out, "Mercado", 65890).await;

    // `u` means "undo what *I* just did". Reaching into somebody else's last
    // action would be a very different, much more surprising gesture — and
    // this is a shared ledger.
    assert!(tx::last_undoable(&pool, them).await.unwrap().is_none());
    assert!(tx::last_undoable(&pool, me).await.unwrap().is_some());
}

#[tokio::test]
async fn deleting_twice_does_not_write_a_second_entry() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;

    assert!(
        tx::soft_delete(&pool, Actor::User(me), id)
            .await
            .unwrap()
            .is_some()
    );
    // Two people pressing `dd` on the same row is a real case in a shared
    // ledger, and it must not produce two deletions in the log.
    assert!(
        tx::soft_delete(&pool, Actor::User(me), id)
            .await
            .unwrap()
            .is_none()
    );

    let deletes: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE action = 'delete'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(deletes, 1);
}

#[tokio::test]
async fn a_failed_insert_leaves_no_trace_in_the_log() {
    let pool = pool().await;
    let me = household(&pool).await;

    // A member id that does not exist: the foreign key refuses it, and the
    // change entry must go with it rather than recording a row that was never
    // written.
    let bad = tx::add(
        &pool,
        Actor::User(me),
        NewTransaction {
            member_id: 9999,
            period: sep(),
            kind: TxKind::Out,
            title: "Mercado",
            amount: Some(Cents(100)),
            date: None,
            bucket: None,
            visibility: Visibility::Household,
        },
    )
    .await;
    assert!(bad.is_err(), "foreign keys must be on");

    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM change")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(logged, 0);
}
