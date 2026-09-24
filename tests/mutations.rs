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
        line_numbers: Default::default(),
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

// ---------------------------------------------------------------------------
// edit, privacy, re-attribution — and undoing each of them
// ---------------------------------------------------------------------------
//
// `u` must restore the exact prior state, which is a stronger claim than "it
// runs the opposite verb". Every test below asserts the row afterwards, not
// just that the call returned true.

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

async fn amount_of(pool: &SqlitePool, id: i64) -> Option<i64> {
    sqlx::query_scalar("SELECT amount_cents FROM transactions WHERE id = ?1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn undo_last(pool: &SqlitePool, me: i64) -> bool {
    let u = tx::last_undoable(pool, me)
        .await
        .unwrap()
        .expect("something to undo");
    tx::undo(pool, Actor::User(me), &u).await.unwrap()
}

#[tokio::test]
async fn an_edit_records_only_what_moved_and_undo_puts_it_back() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;

    assert_eq!(
        tx::edit(
            &pool,
            Actor::User(me),
            id,
            tx::EditFields {
                title: "Mercado",
                amount: Some(Cents(61230)),
                day: None,
                bucket: None,
                visibility: Visibility::Household,
            },
            None,
            None,
        )
        .await
        .unwrap(),
        tx::Saved::Ok
    );

    // Only the amount moved, so only the amount is in the log. An entry that
    // listed every field of the form would bury the one fact it records.
    let fields: Vec<String> = sqlx::query_scalar(
        "SELECT field FROM change_field f JOIN change c ON c.id = f.change_id
         WHERE c.action = 'edit'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(fields, ["amount"]);
    assert_eq!(amount_of(&pool, id).await, Some(61230));

    assert!(undo_last(&pool, me).await);
    assert_eq!(amount_of(&pool, id).await, Some(65890));
}

#[tokio::test]
async fn privacy_is_its_own_verb_and_undo_toggles_it_back() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Presente", 24000).await;

    assert_eq!(
        tx::toggle_privacy(&pool, Actor::User(me), id, None)
            .await
            .unwrap(),
        Some(true)
    );
    let v: String = sqlx::query_scalar("SELECT visibility FROM transactions WHERE id = ?1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(v, "private");

    // Its own action, never folded into `edit`: who can see a row is a
    // different kind of fact from what the row says.
    let action: String = sqlx::query_scalar(
        "SELECT action FROM change WHERE entity = 'transaction' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(action, "privacy");

    assert!(undo_last(&pool, me).await);
    let v: String = sqlx::query_scalar("SELECT visibility FROM transactions WHERE id = ?1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(v, "household");
}

#[tokio::test]
async fn undoing_a_reattribution_restores_the_exact_prior_position() {
    let pool = pool().await;
    let me = household(&pool).await;
    let other = second_member(&pool, me).await;

    let a = add(&pool, me, TxKind::Out, "Energia", 34362).await;
    let b = add(&pool, me, TxKind::Out, "Água", 10840).await;
    let c = add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    // The destination must already have rows, or "appended" and "where it
    // was" are the same key and the test proves nothing.
    add(&pool, other, TxKind::Out, "Academia", 9990).await;
    assert_eq!(titles(&pool, me).await, ["Energia", "Água", "Mercado"]);

    assert!(
        tx::reattribute(&pool, Actor::User(me), b, other, None)
            .await
            .unwrap()
    );
    assert_eq!(titles(&pool, me).await, ["Energia", "Mercado"]);
    assert_eq!(titles(&pool, other).await, ["Academia", "Água"]);

    // Appending is the right placement for a move you chose and the wrong one
    // for undoing it, so the old key is recorded — and put back.
    assert!(undo_last(&pool, me).await);
    assert_eq!(titles(&pool, me).await, ["Energia", "Água", "Mercado"]);
    assert_eq!(titles(&pool, other).await, ["Academia"]);
    let _ = (a, c);
}

#[tokio::test]
async fn the_recorded_sort_key_stays_out_of_the_log_pane() {
    let pool = pool().await;
    let me = household(&pool).await;
    let other = second_member(&pool, me).await;
    let id = add(&pool, me, TxKind::Out, "Energia", 34362).await;
    add(&pool, other, TxKind::Out, "Academia", 9990).await;

    tx::reattribute(&pool, Actor::User(me), id, other, None)
        .await
        .unwrap();

    // It is in the table, because undo needs it...
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM change_field WHERE field = 'sort_key'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, 1);

    // ...and out of what the pane reads, because a fractional key means
    // nothing to a reader and would make every move read as two changes.
    let entries = ffm::db::changes::history(&pool, id, None, ffm::db::changes::Filter::All, &[])
        .await
        .unwrap();
    let reattributed = entries
        .iter()
        .find(|e| e.action == "reattribute")
        .expect("the move is in the log");
    assert_eq!(
        reattributed
            .fields
            .iter()
            .map(|f| f.field.as_str())
            .collect::<Vec<_>>(),
        ["member"]
    );
}

#[tokio::test]
async fn undoing_a_reorder_puts_the_row_back_between_its_neighbors() {
    let pool = pool().await;
    let me = household(&pool).await;
    add(&pool, me, TxKind::Out, "Energia", 34362).await;
    add(&pool, me, TxKind::Out, "Água", 10840).await;
    add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    let first: i64 = sqlx::query_scalar("SELECT id FROM transactions ORDER BY sort_key LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert!(
        tx::reorder(&pool, Actor::User(me), first, 2, None)
            .await
            .unwrap()
    );
    assert_eq!(titles(&pool, me).await, ["Água", "Mercado", "Energia"]);

    assert!(undo_last(&pool, me).await);
    assert_eq!(titles(&pool, me).await, ["Energia", "Água", "Mercado"]);
}

#[tokio::test]
async fn an_undo_is_an_entry_never_a_rewrite() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;

    tx::toggle_privacy(&pool, Actor::User(me), id, None)
        .await
        .unwrap();
    let undone = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    undo_last(&pool, me).await;

    // The reversal points back at what it reversed, and nothing was deleted:
    // the log reads "made private", then "made household · undo of …".
    let (count, undo_of): (i64, Option<i64>) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM change WHERE action = 'privacy'),
                (SELECT undo_of FROM change WHERE action = 'privacy' ORDER BY id DESC LIMIT 1)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 2);
    assert_eq!(undo_of, Some(undone.change_id));

    // And `u` twice does not toggle the same change forever: the second press
    // walks further back, to the row's own creation.
    let next = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_eq!(next.action, "create");
}

// ---------------------------------------------------------------------------
// concurrency — the version check
// ---------------------------------------------------------------------------
//
// The failure being prevented is a LOST UPDATE: a form held open across
// somebody else's save, then saved over the top of it. Nothing here is about
// resolving a conflict; it is about refusing to lose one.

async fn edit_at(
    pool: &SqlitePool,
    me: i64,
    id: i64,
    title: &str,
    version: Option<i64>,
) -> tx::Saved {
    tx::edit(
        pool,
        Actor::User(me),
        id,
        tx::EditFields {
            title,
            amount: Some(Cents(100)),
            day: None,
            bucket: None,
            visibility: Visibility::Household,
        },
        None,
        version,
    )
    .await
    .unwrap()
}

async fn version_of(pool: &SqlitePool, id: i64) -> i64 {
    sqlx::query_scalar("SELECT version FROM transactions WHERE id = ?1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_save_against_a_stale_version_is_refused_and_writes_nothing() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    let opened_at = version_of(&pool, id).await;

    // Somebody else saves first.
    assert_eq!(
        edit_at(&pool, me, id, "Mercado · Ana", None).await,
        tx::Saved::Ok
    );

    // The form that was open still holds the old version.
    assert_eq!(
        edit_at(&pool, me, id, "Mercado · Rodrigo", Some(opened_at)).await,
        tx::Saved::Stale
    );

    let title: String = sqlx::query_scalar("SELECT title FROM transactions WHERE id = ?1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(title, "Mercado · Ana", "the first save must survive");
}

#[tokio::test]
async fn a_refused_save_leaves_nothing_in_the_log_either() {
    // The entry and the row it describes commit together or not at all. A
    // change log that records an edit nobody made is worse than no log.
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    let opened_at = version_of(&pool, id).await;
    edit_at(&pool, me, id, "Ana", None).await;

    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM change")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        edit_at(&pool, me, id, "Rodrigo", Some(opened_at)).await,
        tx::Saved::Stale
    );
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM change")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, after);
}

#[tokio::test]
async fn every_write_to_a_row_moves_its_version() {
    // The check is only on the edit form's save, but a form held open across
    // ANY of these is the case being caught — so they all have to move it.
    let pool = pool().await;
    let me = household(&pool).await;
    let other = second_member(&pool, me).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    add(&pool, me, TxKind::Out, "Farmácia", 8740).await;

    let mut seen = version_of(&pool, id).await;
    let mut moved = |v: i64, what: &str| {
        assert!(v > seen, "{what} did not move the version");
        seen = v;
    };

    tx::toggle_privacy(&pool, Actor::User(me), id, None)
        .await
        .unwrap();
    moved(version_of(&pool, id).await, "privacy");

    tx::reorder(&pool, Actor::User(me), id, 1, None)
        .await
        .unwrap();
    moved(version_of(&pool, id).await, "reorder");

    tx::reattribute(&pool, Actor::User(me), id, other, None)
        .await
        .unwrap();
    moved(version_of(&pool, id).await, "reattribute");

    tx::soft_delete(&pool, Actor::User(me), id).await.unwrap();
    moved(version_of(&pool, id).await, "delete");

    tx::restore(&pool, Actor::User(me), id, None).await.unwrap();
    moved(version_of(&pool, id).await, "restore");
}

#[tokio::test]
async fn a_save_with_no_version_still_writes() {
    // A form that lost its hidden field must behave like the old unguarded
    // save, not refuse every row whose version has ever moved.
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    tx::toggle_privacy(&pool, Actor::User(me), id, None)
        .await
        .unwrap();
    assert_eq!(
        edit_at(&pool, me, id, "Mercado · 2", None).await,
        tx::Saved::Ok
    );
}

#[tokio::test]
async fn saving_into_a_deleted_row_says_it_is_gone_not_that_it_is_stale() {
    // Different copy, because the next move is different: `u` brings a
    // deleted row back, and reopening it does nothing for one.
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    let opened_at = version_of(&pool, id).await;
    tx::soft_delete(&pool, Actor::User(me), id).await.unwrap();

    assert_eq!(
        edit_at(&pool, me, id, "Mercado · 2", Some(opened_at)).await,
        tx::Saved::Gone
    );
}

#[tokio::test]
async fn undo_writes_regardless_of_the_version() {
    // An undo reverses a change that is already in the log; it is not
    // competing with one. Refusing it because the materializer touched the
    // row would make `u` unreliable for the one thing it exists to do.
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    edit_at(&pool, me, id, "Mercado · editado", None).await;
    // Something else moves the row after the edit that will be undone.
    tx::toggle_privacy(&pool, Actor::User(me), id, None)
        .await
        .unwrap();

    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_eq!(u.action, "privacy");
    tx::undo(&pool, Actor::User(me), &u).await.unwrap();
    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    assert_eq!(u.action, "edit");
    assert!(tx::undo(&pool, Actor::User(me), &u).await.unwrap());

    let title: String = sqlx::query_scalar("SELECT title FROM transactions WHERE id = ?1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(title, "Mercado");
}

async fn amount_source_of(pool: &SqlitePool, id: i64) -> Option<String> {
    sqlx::query_scalar("SELECT amount_source FROM transactions WHERE id = ?1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn edit_amount(pool: &SqlitePool, me: i64, id: i64, amount: Option<i64>) {
    tx::edit(
        pool,
        Actor::User(me),
        id,
        tx::EditFields {
            title: "Mercado",
            amount: amount.map(Cents),
            day: None,
            bucket: None,
            visibility: Visibility::Household,
        },
        None,
        None,
    )
    .await
    .unwrap();
}

/// An amount set with `e` is a person's, and a bill attached later must flag
/// it rather than replace it — which only works if the row remembers it. A
/// seeded amount stays unmarked, clearing one makes it a placeholder again,
/// and an undo leaves the mark alone: it only puts back what was there.
#[tokio::test]
async fn an_edited_amount_is_marked_as_the_persons() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, TxKind::Out, "Mercado", 65890).await;
    assert_eq!(amount_source_of(&pool, id).await, None);

    // Saving the form without touching the amount marks nothing.
    edit_amount(&pool, me, id, Some(65890)).await;
    assert_eq!(amount_source_of(&pool, id).await, None);

    edit_amount(&pool, me, id, Some(61230)).await;
    assert_eq!(amount_source_of(&pool, id).await.as_deref(), Some("user"));

    assert!(undo_last(&pool, me).await);
    assert_eq!(amount_of(&pool, id).await, Some(65890));
    assert_eq!(amount_source_of(&pool, id).await.as_deref(), Some("user"));

    edit_amount(&pool, me, id, None).await;
    assert_eq!(amount_source_of(&pool, id).await, None);
}
