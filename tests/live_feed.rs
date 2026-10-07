//! Every change reaches every open page: the feed reads the change log and
//! announces a row to re-fetch, or a pane to re-render.

mod common;

use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::tx::{self, NewTransaction};
use ffm::domain::money::Cents;
use ffm::domain::period::Period;
use ffm::domain::status::{TxKind, Visibility};
use ffm::services::live::{Live, LiveEvent, Signal};
use ffm::services::live_feed;
use sqlx::SqlitePool;
use tokio::sync::broadcast::Receiver;

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

async fn last_id(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT coalesce(max(id), 0) FROM change")
        .fetch_one(pool)
        .await
        .unwrap()
}

fn events(rx: &mut Receiver<Signal>) -> Vec<LiveEvent> {
    let mut out = Vec::new();
    while let Ok(s) = rx.try_recv() {
        if let Signal::Row(ev) = s {
            out.push(ev);
        }
    }
    out
}

#[tokio::test]
async fn an_edit_announces_its_row_and_a_delete_its_pane() {
    let pool = pool().await;
    let me = household(&pool).await;
    let (live, _stop) = Live::new();
    let mut rx = live.subscribe();

    let id = tx::add(
        &pool,
        Actor::User(me),
        NewTransaction {
            member_id: me,
            period: sep(),
            kind: TxKind::Out,
            title: "Mercado",
            amount: Some(Cents(65890)),
            date: None,
            bucket: None,
            visibility: Visibility::Household,
        },
    )
    .await
    .unwrap();
    let after_add = last_id(&pool).await;

    // A new row is a pane event: the rows around it moved.
    let read = live_feed::drain(&pool, &live, 0).await.unwrap();
    assert_eq!(read, after_add);
    assert_eq!(
        events(&mut rx),
        vec![LiveEvent::pane(me, Some(sep().to_string()))]
    );

    // An edit is the row alone, named by its owner and month.
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
            paid: None,
            due_date: None,
        },
        None,
        None,
    )
    .await
    .unwrap();
    let read = live_feed::drain(&pool, &live, after_add).await.unwrap();
    assert_eq!(
        events(&mut rx),
        vec![LiveEvent::row(id, me, sep().to_string())]
    );

    // A delete re-renders the pane, and the row inside it is not fetched
    // as well.
    tx::soft_delete(&pool, Actor::User(me), id).await.unwrap();
    let read = live_feed::drain(&pool, &live, read).await.unwrap();
    assert_eq!(
        events(&mut rx),
        vec![LiveEvent::pane(me, Some(sep().to_string()))]
    );

    // Nothing new, nothing said.
    assert_eq!(live_feed::drain(&pool, &live, read).await.unwrap(), read);
    assert!(events(&mut rx).is_empty());
}
