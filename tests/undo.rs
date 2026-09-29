//! `u` and `U`: scoped to the pane and the month on screen, redo as an undo
//! of an undo, and never reversing somebody else's later work.

mod common;

use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::tx::{self, Direction, NewTransaction, Scope};
use ffm::domain::money::Cents;
use ffm::domain::period::Period;
use ffm::domain::status::{TxKind, Visibility};
use sqlx::SqlitePool;

fn sep() -> Period {
    Period::parse("2026-09").unwrap()
}

fn aug() -> Period {
    Period::parse("2026-08").unwrap()
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

/// A row in `owner`'s ledger, added by `who`.
async fn add(pool: &SqlitePool, who: i64, owner: i64, period: Period, title: &str) -> i64 {
    tx::add(
        pool,
        Actor::User(who),
        NewTransaction {
            member_id: owner,
            period,
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

async fn set_amount(pool: &SqlitePool, who: i64, id: i64, cents: i64) {
    let title: String = sqlx::query_scalar("SELECT title FROM transactions WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    let saved = tx::edit(
        pool,
        Actor::User(who),
        id,
        tx::EditFields {
            title: &title,
            amount: Some(Cents(cents)),
            day: None,
            bucket: None,
            visibility: Visibility::Household,
        },
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(saved, tx::Saved::Ok);
}

async fn amount(pool: &SqlitePool, id: i64) -> i64 {
    sqlx::query_scalar("SELECT amount_cents FROM transactions WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn deleted(pool: &SqlitePool, id: i64) -> bool {
    sqlx::query_scalar::<_, Option<String>>("SELECT deleted_at FROM transactions WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
        .is_some()
}

fn pane(me: i64, owner: i64, period: Period) -> Scope {
    Scope {
        me,
        owner: Some(owner),
        period: Some(period),
    }
}

/// One `u` or `U`, as the handler takes it for a row change. Returns what it
/// picked, or `None` when there was nothing.
async fn step(pool: &SqlitePool, scope: Scope, dir: Direction) -> Option<(String, i64)> {
    let next = tx::next(pool, scope, dir).await.unwrap();
    let u = next.pick?;
    assert!(tx::undo(pool, Actor::User(scope.me), &u).await.unwrap());
    Some((u.action, u.entity_id))
}

#[tokio::test]
async fn u_reaches_only_the_pane_and_the_month_on_screen() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ana = member(&pool, me, "ana").await;
    let august = add(&pool, me, me, aug(), "Agosto").await;
    let theirs = add(&pool, me, ana, sep(), "Padaria").await;

    // Nothing of mine in my own September pane: August and Ana's pane are
    // somewhere else.
    assert!(
        step(&pool, pane(me, me, sep()), Direction::Undo)
            .await
            .is_none()
    );
    // In Ana's pane, the admin's own add is reachable…
    assert_eq!(
        step(&pool, pane(me, ana, sep()), Direction::Undo).await,
        Some(("create".into(), theirs))
    );
    // …and August is reachable from August.
    assert_eq!(
        step(&pool, pane(me, me, aug()), Direction::Undo).await,
        Some(("create".into(), august))
    );
}

#[tokio::test]
async fn an_admin_never_undoes_the_members_own_change() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ana = member(&pool, me, "ana").await;
    add(&pool, ana, ana, sep(), "Padaria").await;

    // `u` means "undo what I did", in whoever's pane it is pressed.
    assert!(
        step(&pool, pane(me, ana, sep()), Direction::Undo)
            .await
            .is_none()
    );
    assert!(
        step(&pool, pane(ana, ana, sep()), Direction::Undo)
            .await
            .is_some()
    );
}

#[tokio::test]
async fn redo_puts_back_what_undo_took_and_is_last_in_first_out() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, me, sep(), "Mercado").await;
    set_amount(&pool, me, id, 2000).await;
    set_amount(&pool, me, id, 3000).await;
    let here = pane(me, me, sep());

    step(&pool, here, Direction::Undo).await.unwrap();
    step(&pool, here, Direction::Undo).await.unwrap();
    assert_eq!(amount(&pool, id).await, 1000);

    step(&pool, here, Direction::Redo).await.unwrap();
    assert_eq!(amount(&pool, id).await, 2000);
    step(&pool, here, Direction::Redo).await.unwrap();
    assert_eq!(amount(&pool, id).await, 3000);
    assert!(
        step(&pool, here, Direction::Redo).await.is_none(),
        "the stack is spent"
    );

    // And `u` after `U` undoes the redo.
    step(&pool, here, Direction::Undo).await.unwrap();
    assert_eq!(amount(&pool, id).await, 2000);
}

#[tokio::test]
async fn a_fresh_change_clears_the_redo_stack() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, me, sep(), "Mercado").await;
    set_amount(&pool, me, id, 2000).await;
    let here = pane(me, me, sep());

    step(&pool, here, Direction::Undo).await.unwrap();
    set_amount(&pool, me, id, 5000).await;
    assert!(step(&pool, here, Direction::Redo).await.is_none());
    assert_eq!(amount(&pool, id).await, 5000);
}

#[tokio::test]
async fn every_verb_survives_undo_redo_undo() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ana = member(&pool, me, "ana").await;
    let here = pane(me, me, sep());

    // delete
    let a = add(&pool, me, me, sep(), "A").await;
    tx::soft_delete(&pool, Actor::User(me), a).await.unwrap();
    step(&pool, here, Direction::Undo).await.unwrap();
    assert!(!deleted(&pool, a).await);
    step(&pool, here, Direction::Redo).await.unwrap();
    assert!(deleted(&pool, a).await);
    step(&pool, here, Direction::Undo).await.unwrap();
    assert!(!deleted(&pool, a).await);

    // privacy
    tx::toggle_privacy(&pool, Actor::User(me), a, None)
        .await
        .unwrap();
    let vis = |pool: SqlitePool| async move {
        sqlx::query_scalar::<_, String>("SELECT visibility FROM transactions WHERE id = ?")
            .bind(a)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    assert_eq!(vis(pool.clone()).await, "private");
    step(&pool, here, Direction::Undo).await.unwrap();
    assert_eq!(vis(pool.clone()).await, "household");
    step(&pool, here, Direction::Redo).await.unwrap();
    assert_eq!(vis(pool.clone()).await, "private");
    step(&pool, here, Direction::Undo).await.unwrap();
    assert_eq!(vis(pool.clone()).await, "household");

    // reorder
    let b = add(&pool, me, me, sep(), "B").await;
    let order = |pool: SqlitePool| async move {
        sqlx::query_scalar::<_, String>(
            "SELECT title FROM transactions WHERE member_id = ? AND deleted_at IS NULL
             ORDER BY sort_key",
        )
        .bind(me)
        .fetch_all(&pool)
        .await
        .unwrap()
    };
    assert_eq!(order(pool.clone()).await, ["A", "B"]);
    tx::reorder(&pool, Actor::User(me), b, -1, None)
        .await
        .unwrap();
    assert_eq!(order(pool.clone()).await, ["B", "A"]);
    step(&pool, here, Direction::Undo).await.unwrap();
    assert_eq!(order(pool.clone()).await, ["A", "B"]);
    step(&pool, here, Direction::Redo).await.unwrap();
    assert_eq!(order(pool.clone()).await, ["B", "A"]);
    step(&pool, here, Direction::Undo).await.unwrap();
    assert_eq!(order(pool.clone()).await, ["A", "B"]);

    // reattribute — reachable from the pane the row LEFT
    tx::reattribute(&pool, Actor::User(me), b, ana, None)
        .await
        .unwrap();
    let owner = |pool: SqlitePool| async move {
        sqlx::query_scalar::<_, i64>("SELECT member_id FROM transactions WHERE id = ?")
            .bind(b)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    assert_eq!(owner(pool.clone()).await, ana);
    assert_eq!(
        step(&pool, here, Direction::Undo).await,
        Some(("reattribute".into(), b))
    );
    assert_eq!(owner(pool.clone()).await, me);
    step(&pool, here, Direction::Redo).await.unwrap();
    assert_eq!(owner(pool.clone()).await, ana);
    // Now it sits in Ana's pane, and the admin's `u` there brings it back.
    step(&pool, pane(me, ana, sep()), Direction::Undo)
        .await
        .unwrap();
    assert_eq!(owner(pool.clone()).await, me);
}

#[tokio::test]
async fn a_change_somebody_else_changed_since_is_skipped_and_named() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ana = member(&pool, me, "ana").await;
    let first = add(&pool, ana, ana, sep(), "Farmácia").await;
    let row = add(&pool, ana, ana, sep(), "Padaria").await;
    set_amount(&pool, ana, row, 2000).await;
    // The admin changes the same row afterwards.
    set_amount(&pool, me, row, 9000).await;

    let next = tx::next(&pool, pane(ana, ana, sep()), Direction::Undo)
        .await
        .unwrap();
    assert_eq!(
        next.skipped.len(),
        2,
        "her edit and her add, both overtaken"
    );
    assert_eq!(next.skipped[0].who, "rodrigo");
    assert_eq!(next.skipped[0].title, "Padaria");
    // She reaches her previous change instead, and the admin's amount stays.
    let pick = next.pick.unwrap();
    assert_eq!((pick.action.as_str(), pick.entity_id), ("create", first));
    assert!(tx::undo(&pool, Actor::User(ana), &pick).await.unwrap());
    assert_eq!(amount(&pool, row).await, 9000);

    // Reported once, not on every press after.
    let again = tx::next(&pool, pane(ana, ana, sep()), Direction::Undo)
        .await
        .unwrap();
    assert!(again.skipped.is_empty());
}

#[tokio::test]
async fn the_materializer_never_overtakes() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, me, sep(), "Mercado").await;
    set_amount(&pool, me, id, 2000).await;
    // A system entry on the same row, as the materializer writes them.
    sqlx::query(
        "INSERT INTO change (at, member_id, source, action, entity, entity_id)
         VALUES ('2099-01-01T00:00:00Z', NULL, 'materializer', 'materialize', 'transaction', ?)",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    let next = tx::next(&pool, pane(me, me, sep()), Direction::Undo)
        .await
        .unwrap();
    assert!(next.skipped.is_empty());
    assert_eq!(next.pick.unwrap().action, "edit");
}
