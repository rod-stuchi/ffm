//! Every change reaches every open page: the feed reads the change log and
//! announces a row to re-fetch, or a pane to re-render — naming the entries,
//! so the page whose own answer already drew them can let the echo pass.

mod common;

use axum::body::Body;
use axum::http::Request;
use common::pool;
use ffm::config::Config;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::tx::{self, NewTransaction};
use ffm::domain::money::Cents;
use ffm::domain::period::Period;
use ffm::domain::status::{TxKind, Visibility};
use ffm::services::live::{self, Drawn, Live, LiveEvent, Signal};
use ffm::services::live_feed;
use ffm::state::AppState;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast::Receiver;
use tower::ServiceExt;

fn sep() -> Period {
    Period::parse("2026-09").unwrap()
}

async fn household(pool: &SqlitePool) -> i64 {
    signed_in(pool).await.0
}

/// The household's admin, and a session of theirs.
async fn signed_in(pool: &SqlitePool) -> (i64, String) {
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
    let me = auth::session(pool, &sid).await.unwrap().unwrap().member.id;
    (me, sid)
}

async fn row(pool: &SqlitePool, me: i64, title: &str) -> i64 {
    tx::add(
        pool,
        Actor::User(me),
        NewTransaction {
            member_id: me,
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

fn edit(title: &str) -> tx::EditFields<'_> {
    tx::EditFields {
        title,
        amount: Some(Cents(2000)),
        day: None,
        bucket: None,
        visibility: Visibility::Household,
        paid: None,
        due_date: None,
    }
}

/// The entries appended after `after`, oldest first.
async fn ids_after(pool: &SqlitePool, after: i64) -> Vec<i64> {
    sqlx::query_scalar("SELECT id FROM change WHERE id > ?1 ORDER BY id")
        .bind(after)
        .fetch_all(pool)
        .await
        .unwrap()
}

fn carrying(ev: LiveEvent, changes: Vec<i64>) -> LiveEvent {
    LiveEvent { changes, ..ev }
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
    let before = last_id(&pool).await;

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
        vec![carrying(
            LiveEvent::pane(me, Some(sep().to_string())),
            ids_after(&pool, before).await
        )]
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
        vec![carrying(
            LiveEvent::row(id, me, sep().to_string()),
            ids_after(&pool, after_add).await
        )]
    );

    // A delete re-renders the pane, and the row inside it is not fetched
    // as well.
    tx::soft_delete(&pool, Actor::User(me), id).await.unwrap();
    let after_edit = read;
    let read = live_feed::drain(&pool, &live, read).await.unwrap();
    assert_eq!(
        events(&mut rx),
        vec![carrying(
            LiveEvent::pane(me, Some(sep().to_string())),
            ids_after(&pool, after_edit).await
        )]
    );

    // Nothing new, nothing said.
    assert_eq!(live_feed::drain(&pool, &live, read).await.unwrap(), read);
    assert!(events(&mut rx).is_empty());
}

#[tokio::test]
async fn a_row_inside_a_pane_hands_its_entries_to_the_pane() {
    let pool = pool().await;
    let me = household(&pool).await;
    let a = row(&pool, me, "Mercado").await;
    let b = row(&pool, me, "Padaria").await;
    let (live, _stop) = Live::new();
    let mut rx = live.subscribe();
    let before = last_id(&pool).await;

    // An edit and a delete in one pane, read in one go: the pane's fetch
    // re-renders the edited row too, so the pane carries both entries — a
    // page letting the delete pass as its own must still see the edit.
    tx::edit(&pool, Actor::User(me), a, edit("Mercado"), None, None)
        .await
        .unwrap();
    tx::soft_delete(&pool, Actor::User(me), b).await.unwrap();
    live_feed::drain(&pool, &live, before).await.unwrap();

    assert_eq!(
        events(&mut rx),
        vec![carrying(
            LiveEvent::pane(me, Some(sep().to_string())),
            ids_after(&pool, before).await
        )]
    );
}

#[tokio::test]
async fn an_answer_claims_only_the_changes_it_drew_after_them() {
    let pool = pool().await;
    let me = household(&pool).await;
    let a = row(&pool, me, "Mercado").await;
    let b = row(&pool, me, "Padaria").await;
    let pane = || Drawn::Pane(me, sep().to_string());

    // A move answered with its pane: the pane is all its event fetches.
    let before = last_id(&pool).await;
    let ((), made) = live::recording(async {
        tx::reorder(&pool, Actor::User(me), a, 1, None)
            .await
            .unwrap();
        live::drew(pane());
    })
    .await;
    let claimed = live_feed::drawn(&pool, &made).await.unwrap();
    assert!(!claimed.is_empty());
    assert_eq!(claimed, ids_after(&pool, before).await);

    // An edit answered with its row.
    let before = last_id(&pool).await;
    let ((), made) = live::recording(async {
        tx::edit(&pool, Actor::User(me), a, edit("Mercado"), None, None)
            .await
            .unwrap();
        live::drew(Drawn::Row(a));
    })
    .await;
    assert_eq!(
        live_feed::drawn(&pool, &made).await.unwrap(),
        ids_after(&pool, before).await
    );

    // Drawn before the change: it shows what was there, not the change.
    let ((), made) = live::recording(async {
        live::drew(Drawn::Row(a));
        tx::edit(&pool, Actor::User(me), a, edit("Mercadinho"), None, None)
            .await
            .unwrap();
    })
    .await;
    assert!(live_feed::drawn(&pool, &made).await.unwrap().is_empty());

    // A delete answered with its row, struck through, while its pane lost a
    // line: the pane's fetch is still owed.
    let ((), made) = live::recording(async {
        tx::soft_delete(&pool, Actor::User(me), b).await.unwrap();
        live::drew(Drawn::Row(b));
    })
    .await;
    assert!(!made.changes.is_empty());
    assert!(live_feed::drawn(&pool, &made).await.unwrap().is_empty());

    // Another row, or another member's pane, is not this one.
    let ((), made) = live::recording(async {
        tx::edit(&pool, Actor::User(me), a, edit("Mercado"), None, None)
            .await
            .unwrap();
        live::drew(Drawn::Row(b));
        live::drew(Drawn::Pane(me + 1, sep().to_string()));
        live::drew(Drawn::Pane(me, "2026-10".into()));
    })
    .await;
    assert!(live_feed::drawn(&pool, &made).await.unwrap().is_empty());

    // Outside a request there is no record, and appending is not an error.
    tx::edit(&pool, Actor::User(me), a, edit("Mercado"), None, None)
        .await
        .unwrap();
}

/// One write through the app's own router, signed in, as htmx sends it.
async fn post(app: &axum::Router, sid: &str, uri: &str, form: &str) -> axum::response::Response {
    let res = app
        .clone()
        .oneshot(
            Request::post(uri)
                .header("cookie", format!("ffm.sid={sid}"))
                .header("hx-request", "true")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(res.status().is_success(), "{uri}: {}", res.status());
    res
}

fn claimed(res: &axum::response::Response) -> Option<String> {
    res.headers()
        .get("x-ffm-changes")
        .map(|v| v.to_str().unwrap().to_string())
}

fn joined(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

#[tokio::test]
async fn a_write_names_the_changes_its_answer_drew_and_no_others() {
    let pool = pool().await;
    let (me, sid) = signed_in(&pool).await;
    let a = row(&pool, me, "Mercado").await;
    let b = row(&pool, me, "Padaria").await;
    let dir = std::env::temp_dir().join(format!("ffm-test-{}", std::process::id()));
    let (live, _stop) = Live::new();
    let app = ffm::web::router(AppState {
        pool: pool.clone(),
        cfg: Arc::new(Config {
            db_path: dir.join("unused.db"),
            receipts_root: dir.clone(),
            backup_dir: dir,
            file: None,
        }),
        live: live.clone(),
    });
    let mut rx = live.subscribe();

    // `J` answers with the pane, so its move is the page's own — and the
    // event the feed makes of it carries exactly those entries.
    let before = last_id(&pool).await;
    let res = post(&app, &sid, &format!("/tx/{a}/move"), "delta=1").await;
    let moved = ids_after(&pool, before).await;
    assert!(!moved.is_empty());
    assert_eq!(claimed(&res), Some(joined(&moved)));
    live_feed::drain(&pool, &live, before).await.unwrap();
    assert_eq!(
        events(&mut rx),
        vec![carrying(
            LiveEvent::pane(me, Some(sep().to_string())),
            moved
        )]
    );

    // `p` answers with its row.
    let before = last_id(&pool).await;
    let res = post(&app, &sid, &format!("/tx/{a}/privacy"), "").await;
    assert_eq!(claimed(&res), Some(joined(&ids_after(&pool, before).await)));

    // `dd` answers with the row struck through, while its pane lost a line:
    // nothing is claimed, and the pane comes back to this page too.
    let before = last_id(&pool).await;
    let res = post(&app, &sid, &format!("/tx/{b}/delete"), "").await;
    assert!(!ids_after(&pool, before).await.is_empty());
    assert_eq!(claimed(&res), None);

    // A read makes nothing to name.
    let res = app
        .clone()
        .oneshot(
            Request::get(format!("/tx/{a}"))
                .header("cookie", format!("ffm.sid={sid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(claimed(&res), None);
}
