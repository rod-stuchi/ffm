//! `:order` — one pane's manual order copied to other months, and one `u`
//! (from the month it was copied from) taking the whole copy back.

mod common;

use common::pool;
use ffm::db::auth;
use ffm::db::changes::{self, Actor, Filter};
use ffm::db::order;
use ffm::db::tx::{self, Direction, NewTransaction, Overtaker, Scope, Step};
use ffm::domain::money::Cents;
use ffm::domain::period::Period;
use ffm::domain::status::{TxKind, Visibility};
use ffm::services::live::{Live, LiveEvent, Signal};
use ffm::services::live_feed;
use sqlx::SqlitePool;

fn p(m: u32) -> Period {
    Period::new(2026, m).unwrap()
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

/// Rows added in this order, so this is their manual order.
async fn rows(pool: &SqlitePool, me: i64, period: Period, titles: &[&str]) -> Vec<i64> {
    let mut ids = Vec::new();
    for title in titles {
        ids.push(
            tx::add(
                pool,
                Actor::User(me),
                NewTransaction {
                    member_id: me,
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
            .unwrap(),
        );
    }
    ids
}

async fn titles(pool: &SqlitePool, me: i64, period: Period) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT title FROM transactions
         WHERE member_id = ? AND period = ? AND deleted_at IS NULL
         ORDER BY sort_key, id",
    )
    .bind(me)
    .bind(period.to_string())
    .fetch_all(pool)
    .await
    .unwrap()
}

fn here(me: i64, period: Period) -> Scope {
    Scope {
        me,
        owner: Some(me),
        period: Some(period),
    }
}

async fn step(pool: &SqlitePool, scope: Scope, dir: Direction) -> Option<tx::Undoable> {
    let u = tx::next(pool, scope, dir).await.unwrap().pick?;
    assert!(tx::undo(pool, Actor::User(scope.me), &u).await.unwrap());
    Some(u)
}

/// October in one order; September and August each in another.
async fn three_months(pool: &SqlitePool) -> i64 {
    let me = household(pool).await;
    rows(pool, me, p(8), &["Luz", "Aluguel"]).await;
    rows(pool, me, p(9), &["Água", "Padaria", "Luz", "Aluguel"]).await;
    rows(pool, me, p(10), &["Aluguel", "Luz", "Água"]).await;
    me
}

#[tokio::test]
async fn each_month_takes_this_order_and_rows_with_no_match_go_last() {
    let pool = pool().await;
    let me = three_months(&pool).await;

    let done = order::copy(&pool, Actor::User(me), me, p(10), &[p(8), p(9), p(10)])
        .await
        .unwrap();
    assert!(done.anchor.is_some());
    // The source itself is never a target.
    assert_eq!(
        done.months.iter().map(|(m, _)| *m).collect::<Vec<_>>(),
        [p(8), p(9)]
    );
    assert_eq!(titles(&pool, me, p(8)).await, ["Aluguel", "Luz"]);
    assert_eq!(
        titles(&pool, me, p(9)).await,
        ["Aluguel", "Luz", "Água", "Padaria"]
    );
    // The month copied from is exactly as it was.
    assert_eq!(titles(&pool, me, p(10)).await, ["Aluguel", "Luz", "Água"]);

    // One anchor, on the pane copied from; one part per row that moved.
    let (anchor_entity, anchor_period): (String, String) =
        sqlx::query_as("SELECT entity, period FROM change WHERE id = ?")
            .bind(done.anchor.unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (anchor_entity.as_str(), anchor_period.as_str()),
        ("member", "2026-10")
    );
    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE part_of = ?")
        .bind(done.anchor.unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(parts as usize, done.rows());
}

#[tokio::test]
async fn a_month_already_in_this_order_is_not_written() {
    let pool = pool().await;
    let me = three_months(&pool).await;
    order::copy(&pool, Actor::User(me), me, p(10), &[p(9)])
        .await
        .unwrap();
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM change")
        .fetch_one(&pool)
        .await
        .unwrap();

    let again = order::copy(&pool, Actor::User(me), me, p(10), &[p(9)])
        .await
        .unwrap();
    assert_eq!(again.anchor, None, "nothing moved, so no empty step for u");
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM change")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, after);
}

#[tokio::test]
async fn one_u_in_the_month_copied_from_puts_every_month_back_and_u_redoes_it() {
    let pool = pool().await;
    let me = three_months(&pool).await;
    order::copy(&pool, Actor::User(me), me, p(10), &[p(8), p(9)])
        .await
        .unwrap();

    let u = step(&pool, here(me, p(10)), Direction::Undo).await.unwrap();
    assert_eq!(
        u.step,
        Step::OrderCopy {
            owner: me,
            period: p(10)
        }
    );
    assert_eq!(titles(&pool, me, p(8)).await, ["Luz", "Aluguel"]);
    assert_eq!(
        titles(&pool, me, p(9)).await,
        ["Água", "Padaria", "Luz", "Aluguel"]
    );

    let r = step(&pool, here(me, p(10)), Direction::Redo).await.unwrap();
    assert!(matches!(r.step, Step::OrderCopy { .. }));
    assert_eq!(titles(&pool, me, p(8)).await, ["Aluguel", "Luz"]);
    assert_eq!(
        titles(&pool, me, p(9)).await,
        ["Aluguel", "Luz", "Água", "Padaria"]
    );

    // And back once more: an undo of the redo.
    step(&pool, here(me, p(10)), Direction::Undo).await.unwrap();
    assert_eq!(titles(&pool, me, p(8)).await, ["Luz", "Aluguel"]);
}

#[tokio::test]
async fn a_copied_row_is_never_a_step_of_its_own() {
    let pool = pool().await;
    let me = three_months(&pool).await;
    order::copy(&pool, Actor::User(me), me, p(10), &[p(9)])
        .await
        .unwrap();
    // In September, the newest change of mine is the copy's move of a row —
    // a part, which `u` there never picks: it reaches the adds before it.
    let pick = tx::next(&pool, here(me, p(9)), Direction::Undo)
        .await
        .unwrap()
        .pick
        .unwrap();
    assert_eq!(pick.action, "create");
}

#[tokio::test]
async fn a_row_moved_since_the_copy_stays_where_it_was_put() {
    let pool = pool().await;
    let me = three_months(&pool).await;
    order::copy(&pool, Actor::User(me), me, p(10), &[p(9)])
        .await
        .unwrap();
    // September reads Aluguel, Luz, Água, Padaria now — Luz one of the rows
    // the copy moved — and then Luz is moved to the bottom by hand.
    let luz: i64 = sqlx::query_scalar(
        "SELECT id FROM transactions WHERE period = '2026-09' AND title = 'Luz'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        tx::reorder(&pool, Actor::User(me), luz, 2, None)
            .await
            .unwrap()
    );
    let key = |pool: SqlitePool| async move {
        sqlx::query_scalar::<_, String>("SELECT sort_key FROM transactions WHERE id = ?")
            .bind(luz)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let put = key(pool.clone()).await;

    let anchor = tx::next(&pool, here(me, p(10)), Direction::Undo)
        .await
        .unwrap()
        .pick
        .unwrap();
    let rev = order::preview_reverse(&pool, anchor.change_id)
        .await
        .unwrap();
    assert_eq!(rev.left, 1, "{rev:?}");
    assert!(tx::undo(&pool, Actor::User(me), &anchor).await.unwrap());
    // Luz keeps the place it was moved to by hand; undoing the copy does
    // not undo somebody's later move.
    assert_eq!(key(pool.clone()).await, put);
}

#[tokio::test]
async fn a_move_by_hand_that_a_copy_moved_again_is_skipped_by_u() {
    let pool = pool().await;
    let me = household(&pool).await;
    rows(&pool, me, p(10), &["A", "B", "C", "D"]).await;
    let sep = rows(&pool, me, p(9), &["A", "B", "C", "D"]).await;
    // D moved to the top of September by hand…
    tx::reorder(&pool, Actor::User(me), sep[3], -3, None)
        .await
        .unwrap();
    // …and then October's order copied onto September, which can only put
    // D back by moving it again.
    order::copy(&pool, Actor::User(me), me, p(10), &[p(9)])
        .await
        .unwrap();
    assert_eq!(titles(&pool, me, p(9)).await, ["A", "B", "C", "D"]);

    let next = tx::next(&pool, here(me, p(9)), Direction::Undo)
        .await
        .unwrap();
    assert_eq!(next.skipped.len(), 1, "{:?}", next.skipped);
    assert_eq!(next.skipped[0].who, Overtaker::OrderCopy);
    assert_eq!(next.skipped[0].title, "D");
    assert_eq!(next.pick.unwrap().action, "create");
}

#[tokio::test]
async fn a_copy_clears_the_redo_stack_in_the_months_it_touched() {
    let pool = pool().await;
    let me = three_months(&pool).await;
    // Something in September undone: there is a redo waiting.
    let extra = rows(&pool, me, p(9), &["Farmácia"]).await[0];
    let u = step(&pool, here(me, p(9)), Direction::Undo).await.unwrap();
    assert_eq!(u.entity_id, extra);
    assert!(
        tx::next(&pool, here(me, p(9)), Direction::Redo)
            .await
            .unwrap()
            .pick
            .is_some()
    );
    // A copy onto September is a fresh change there, as in Vim.
    order::copy(&pool, Actor::User(me), me, p(10), &[p(9)])
        .await
        .unwrap();
    assert!(
        tx::next(&pool, here(me, p(9)), Direction::Redo)
            .await
            .unwrap()
            .pick
            .is_none()
    );
}

#[tokio::test]
async fn each_moved_row_says_so_in_its_own_history() {
    let pool = pool().await;
    let me = three_months(&pool).await;
    order::copy(&pool, Actor::User(me), me, p(10), &[p(8)])
        .await
        .unwrap();
    // August was Luz, Aluguel: Aluguel keeps its key, Luz is the row moved.
    let luz: i64 = sqlx::query_scalar(
        "SELECT id FROM transactions WHERE period = '2026-08' AND title = 'Luz'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let log = changes::history(&pool, luz, None, Filter::Row, &[])
        .await
        .unwrap();
    let moved = log.iter().find(|e| e.action == "reorder").unwrap();
    assert_eq!(moved.note.as_deref(), Some("order copied from 10/2026"));
    // The position the log reads, and never the key behind it.
    let fields: Vec<&str> = moved.fields.iter().map(|f| f.field.as_str()).collect();
    assert_eq!(fields, ["position"]);
}

#[tokio::test]
async fn the_months_a_copy_reached_hear_it() {
    let pool = pool().await;
    let me = three_months(&pool).await;
    let (live, _stop) = Live::new();
    let mut rx = live.subscribe();
    let from: i64 = sqlx::query_scalar("SELECT max(id) FROM change")
        .fetch_one(&pool)
        .await
        .unwrap();

    order::copy(&pool, Actor::User(me), me, p(10), &[p(8), p(9)])
        .await
        .unwrap();
    live_feed::drain(&pool, &live, from).await.unwrap();

    let mut panes = Vec::new();
    while let Ok(s) = rx.try_recv() {
        if let Signal::Row(ev) = s {
            panes.push(ev);
        }
    }
    for m in [p(8), p(9)] {
        assert!(
            panes.contains(&LiveEvent::pane(me, Some(m.to_string()))),
            "{m}: {panes:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// `J` / `K` on a selection — the same keys, the same one-step undo
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_selection_moves_together_and_one_u_puts_it_back() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ids = rows(&pool, me, p(10), &["A", "B", "C", "D", "E", "F"]).await;
    // A and C selected, two places down: the gap between them is kept.
    let moved = tx::reorder_many(&pool, Actor::User(me), me, p(10), &[ids[0], ids[2]], 2)
        .await
        .unwrap();
    assert_eq!(moved, 2);
    assert_eq!(
        titles(&pool, me, p(10)).await,
        ["B", "D", "A", "E", "C", "F"]
    );
    // Far past the end, they pile up there in their own order.
    tx::reorder_many(&pool, Actor::User(me), me, p(10), &[ids[0], ids[2]], 99)
        .await
        .unwrap();
    assert_eq!(
        titles(&pool, me, p(10)).await,
        ["B", "D", "E", "F", "A", "C"]
    );
    // Nowhere further to go: nothing written, nothing to undo.
    let quiet = tx::reorder_many(&pool, Actor::User(me), me, p(10), &[ids[0], ids[2]], 1)
        .await
        .unwrap();
    assert_eq!(quiet, 0);

    // One `u` for the whole selection, each time.
    let u = step(&pool, here(me, p(10)), Direction::Undo).await.unwrap();
    assert_eq!(u.action, "reorder");
    assert_eq!(
        titles(&pool, me, p(10)).await,
        ["B", "D", "A", "E", "C", "F"]
    );
    step(&pool, here(me, p(10)), Direction::Undo).await.unwrap();
    assert_eq!(
        titles(&pool, me, p(10)).await,
        ["A", "B", "C", "D", "E", "F"]
    );
    // And `U` moves them again, whole.
    step(&pool, here(me, p(10)), Direction::Redo).await.unwrap();
    assert_eq!(
        titles(&pool, me, p(10)).await,
        ["B", "D", "A", "E", "C", "F"]
    );
    step(&pool, here(me, p(10)), Direction::Undo).await.unwrap();
    assert_eq!(
        titles(&pool, me, p(10)).await,
        ["A", "B", "C", "D", "E", "F"]
    );
}

#[tokio::test]
async fn a_selection_moved_up_and_rows_outside_the_pane_left_out() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ids = rows(&pool, me, p(10), &["A", "B", "C", "D"]).await;
    let other = rows(&pool, me, p(9), &["X"]).await[0];
    let moved = tx::reorder_many(
        &pool,
        Actor::User(me),
        me,
        p(10),
        &[ids[2], ids[3], other],
        -1,
    )
    .await
    .unwrap();
    assert_eq!(moved, 2);
    assert_eq!(titles(&pool, me, p(10)).await, ["A", "C", "D", "B"]);
    assert_eq!(titles(&pool, me, p(9)).await, ["X"]);
}

#[tokio::test]
async fn a_selection_move_is_not_taken_for_a_copied_order() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ids = rows(&pool, me, p(10), &["A", "B", "C", "D"]).await;
    // D moved up by hand, then D and A moved together: undoing the selection
    // and then the hand move must not call the hand move "overtaken by a
    // copied order" — a selection's parts are not a copy's.
    tx::reorder(&pool, Actor::User(me), ids[3], -1, None)
        .await
        .unwrap();
    tx::reorder_many(&pool, Actor::User(me), me, p(10), &[ids[0], ids[3]], 1)
        .await
        .unwrap();
    step(&pool, here(me, p(10)), Direction::Undo).await.unwrap();
    let next = tx::next(&pool, here(me, p(10)), Direction::Undo)
        .await
        .unwrap();
    assert!(next.skipped.is_empty(), "{:?}", next.skipped);
    let pick = next.pick.unwrap();
    assert_eq!((pick.action.as_str(), pick.entity_id), ("reorder", ids[3]));
}

#[tokio::test]
async fn a_selection_move_is_one_entry_with_its_rows_as_parts() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ids = rows(&pool, me, p(10), &["A", "B", "C"]).await;
    tx::reorder_many(&pool, Actor::User(me), me, p(10), &[ids[0], ids[1]], 1)
        .await
        .unwrap();
    let u = tx::next(&pool, here(me, p(10)), Direction::Undo)
        .await
        .unwrap()
        .pick
        .unwrap();
    assert_eq!(tx::parts_of(&pool, u.change_id).await.unwrap(), 1);
}

#[tokio::test]
async fn two_rows_of_one_step_that_trade_keys_each_get_theirs_back() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ids = rows(&pool, me, p(10), &["A", "B", "C", "D", "E", "F", "G"]).await;
    let sel = [ids[0], ids[2]];
    // Down 1, down 3, then up 1 — the up can hand one row the very key the
    // other had after the down, and undoing it must not read that as taken.
    for d in [1, 3, -1] {
        tx::reorder_many(&pool, Actor::User(me), me, p(10), &sel, d)
            .await
            .unwrap();
    }
    for _ in 0..3 {
        step(&pool, here(me, p(10)), Direction::Undo).await.unwrap();
    }
    assert_eq!(
        titles(&pool, me, p(10)).await,
        ["A", "B", "C", "D", "E", "F", "G"]
    );
}
