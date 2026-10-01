//! A member's lifecycle: turned off and back on, the trash, the guards on
//! both, and pending links that can be copied again because their token is
//! kept while they are pending.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::ledger;
use ffm::db::members::{self, Refusal, Status};
use ffm::db::rules::{self, RuleForm};
use ffm::db::tx::{self, NewTransaction};
use ffm::domain::money::Cents;
use ffm::domain::period::Period;
use ffm::domain::status::{TxKind, Visibility};
use ffm::domain::viewer::Viewer;
use ffm::services::materializer;
use sqlx::SqlitePool;

fn p(s: &str) -> Period {
    Period::parse(s).unwrap()
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn household(pool: &SqlitePool) -> i64 {
    let sid = auth::setup(
        pool,
        auth::NewHousehold {
            household_name: "Casa Stuchi",
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
    auth::session(pool, &sid).await.unwrap().unwrap().member.id
}

/// Invites and accepts `name`, returning their id and their session.
async fn join(pool: &SqlitePool, me: i64, name: &str, role: i64) -> (i64, String) {
    let token = auth::issue_link(pool, Actor::User(me), None, None, role)
        .await
        .unwrap();
    let sid = auth::accept_invite(pool, &token, name, "hunter2hunter2", "en", "ffm", role, "x")
        .await
        .unwrap();
    let id = auth::session(pool, &sid).await.unwrap().unwrap().member.id;
    (id, sid)
}

async fn spend(pool: &SqlitePool, who: i64, period: &str, title: &str) {
    tx::add(
        pool,
        Actor::User(who),
        NewTransaction {
            member_id: who,
            period: p(period),
            kind: TxKind::Out,
            title,
            amount: Some(Cents(1000)),
            date: None,
            bucket: None,
            visibility: Visibility::Household,
        },
    )
    .await
    .unwrap();
}

async fn has_pane(pool: &SqlitePool, who: i64, period: &str) -> bool {
    ledger::members_in(pool, p(period))
        .await
        .unwrap()
        .iter()
        .any(|m| m.id == who)
}

#[tokio::test]
async fn turned_off_hides_them_from_that_month_on_and_back_on_does_not_bring_the_gap_back() {
    let pool = pool().await;
    let me = household(&pool).await;
    let (ana, _) = join(&pool, me, "ana", 1).await;
    for m in ["2026-08", "2026-09", "2026-10", "2026-11"] {
        spend(&pool, ana, m, "Mercado").await;
    }

    assert!(
        members::disable(&pool, me, ana, p("2026-09"))
            .await
            .unwrap()
    );
    // Already off: nothing to do, not an error.
    assert!(
        !members::disable(&pool, me, ana, p("2026-10"))
            .await
            .unwrap()
    );

    assert!(
        has_pane(&pool, ana, "2026-08").await,
        "the months before still show them"
    );
    assert!(!has_pane(&pool, ana, "2026-09").await);
    assert!(!has_pane(&pool, ana, "2026-10").await);

    assert!(members::enable(&pool, me, ana, p("2026-11")).await.unwrap());
    assert!(has_pane(&pool, ana, "2026-11").await);
    assert!(
        !has_pane(&pool, ana, "2026-09").await && !has_pane(&pool, ana, "2026-10").await,
        "the months they were away stay hidden"
    );
    assert!(has_pane(&pool, ana, "2026-08").await);

    // `:grep` finds nothing where there is no pane to land on.
    let hits: Vec<String> = ledger::grep(&pool, Viewer::new(me, true), "mercado")
        .await
        .unwrap()
        .into_iter()
        .map(|h| h.period.to_string())
        .collect();
    assert_eq!(hits, ["2026-11", "2026-08"]);

    // Both ways are in the log, as a status field.
    let logged: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT c.action, f.old_value, f.new_value FROM change c
         JOIN change_field f ON f.change_id = c.id
         WHERE c.entity = 'member' AND f.field = 'status' ORDER BY c.id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        logged,
        vec![
            ("edit".into(), "active".into(), "inactive".into()),
            ("edit".into(), "inactive".into(), "active".into()),
        ]
    );
}

#[tokio::test]
async fn back_on_in_the_same_month_hides_nothing() {
    let pool = pool().await;
    let me = household(&pool).await;
    let (ana, _) = join(&pool, me, "ana", 1).await;
    members::disable(&pool, me, ana, p("2026-09"))
        .await
        .unwrap();
    members::enable(&pool, me, ana, p("2026-09")).await.unwrap();
    assert!(has_pane(&pool, ana, "2026-09").await);
}

#[tokio::test]
async fn the_materializer_skips_the_months_away_and_never_backfills_them() {
    let pool = pool().await;
    let me = household(&pool).await;
    let (ana, _) = join(&pool, me, "ana", 1).await;
    let form = RuleForm {
        member_id: ana,
        kind: TxKind::Out,
        title: "Internet".into(),
        amount: Some(Cents(12990)),
        cadence_months: 1,
        due_day: Some(10),
        start_date: d(2026, 6, 1),
        end_date: None,
        total_installments: None,
        carry_forward_amount: true,
        tracked: false,
        bucket: None,
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
        months: None,
        visibility: Visibility::Household,
    };
    rules::create(&pool, Actor::User(ana), &form).await.unwrap();

    members::disable(&pool, me, ana, p("2026-08"))
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 10, 15))
        .await
        .unwrap();
    members::enable(&pool, me, ana, p("2026-11")).await.unwrap();
    materializer::reconcile(&pool, d(2026, 11, 15))
        .await
        .unwrap();

    let periods: Vec<String> =
        sqlx::query_scalar("SELECT period FROM transactions WHERE member_id = ?1 ORDER BY period")
            .bind(ana)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(periods, ["2026-06", "2026-07", "2026-11"]);
}

#[tokio::test]
async fn a_member_turned_off_cannot_sign_in_and_their_sessions_and_reset_go() {
    let pool = pool().await;
    let me = household(&pool).await;
    let (ana, sid) = join(&pool, me, "ana", 1).await;
    let reset = auth::issue_link(&pool, Actor::User(me), Some(ana), None, 1)
        .await
        .unwrap();
    // The reset signed them out; sign back in for a live session to lose.
    let sid = {
        let _ = sid;
        let id = auth::authenticate(&pool, "ana", "hunter2hunter2")
            .await
            .unwrap()
            .unwrap();
        let mut t = pool.begin().await.unwrap();
        let s = auth::start_session(&mut t, id, "x").await.unwrap();
        t.commit().await.unwrap();
        s
    };

    members::disable(&pool, me, ana, p("2026-09"))
        .await
        .unwrap();

    assert!(auth::session(&pool, &sid).await.unwrap().is_none());
    // The right password still authenticates — the login page then says the
    // account is off rather than "wrong password".
    assert_eq!(
        auth::authenticate(&pool, "ana", "hunter2hunter2")
            .await
            .unwrap(),
        Some(ana)
    );
    assert!(members::is_inactive(&pool, ana).await.unwrap());
    // And the open reset is no way back in.
    assert_eq!(
        auth::read_link(&pool, &reset).await.unwrap(),
        auth::Link::Used
    );
    assert!(
        auth::accept_reset(&pool, &reset, "brandnewpass", "x")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn only_a_member_with_no_rows_goes_to_the_trash_and_comes_back() {
    let pool = pool().await;
    let me = household(&pool).await;
    let (ana, _) = join(&pool, me, "ana", 1).await;
    let (bia, _) = join(&pool, me, "bia", 1).await;
    spend(&pool, ana, "2026-09", "Mercado").await;

    assert_eq!(
        members::trash(&pool, me, ana).await.unwrap(),
        Err(Refusal::HasRows)
    );
    assert_eq!(members::trash(&pool, me, bia).await.unwrap(), Ok(()));

    let status =
        |list: &[members::MemberRow], id: i64| list.iter().find(|m| m.id == id).map(|m| m.status);
    let list = members::list(&pool).await.unwrap();
    assert_eq!(status(&list, bia), Some(Status::Trashed));
    assert!(list.iter().find(|m| m.id == ana).unwrap().has_rows);
    assert_eq!(
        auth::authenticate(&pool, "bia", "hunter2hunter2")
            .await
            .unwrap(),
        None,
        "the trash cannot sign in"
    );
    assert!(!has_pane(&pool, bia, "2026-09").await);

    assert!(members::restore(&pool, me, bia).await.unwrap());
    let list = members::list(&pool).await.unwrap();
    assert_eq!(status(&list, bia), Some(Status::Active));
}

#[tokio::test]
async fn nobody_acts_on_themselves_on_a_stranger_or_on_the_last_admin() {
    let pool = pool().await;
    let me = household(&pool).await;
    let (other, _) = join(&pool, me, "ana", 0).await;

    assert_eq!(
        members::may_change(&pool, me, me)
            .await
            .unwrap()
            .unwrap_err(),
        Refusal::Yourself
    );
    assert_eq!(
        members::may_change(&pool, me, 999)
            .await
            .unwrap()
            .unwrap_err(),
        Refusal::Unknown
    );
    assert!(members::may_change(&pool, me, other).await.unwrap().is_ok());

    // With `me` turned off, `ana` is the only admin who can still sign in:
    // turning her off would leave nobody to turn anyone back on.
    members::disable(&pool, other, me, p("2026-09"))
        .await
        .unwrap();
    assert_eq!(
        members::may_change(&pool, me, other)
            .await
            .unwrap()
            .unwrap_err(),
        Refusal::LastAdmin
    );
}

#[tokio::test]
async fn a_pending_link_keeps_its_token_until_it_is_used_revoked_or_expires() {
    let pool = pool().await;
    let me = household(&pool).await;
    let stored = |pool: SqlitePool| async move {
        sqlx::query_scalar::<_, Option<String>>("SELECT token FROM member_invites ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap()
    };

    // Pending: the pane shows it, and can copy it again.
    let a = auth::issue_link(&pool, Actor::User(me), None, Some("a"), 1)
        .await
        .unwrap();
    let pending = members::pending_invites(&pool).await.unwrap();
    assert_eq!(pending[0].token.as_deref(), Some(a.as_str()));

    // Used: wiped.
    auth::accept_invite(&pool, &a, "ana", "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    // Revoked: wiped.
    let b = auth::issue_link(&pool, Actor::User(me), None, Some("b"), 1)
        .await
        .unwrap();
    let b_id = members::pending_invites(&pool).await.unwrap()[0].id;
    members::revoke_invite(&pool, b_id).await.unwrap();
    // Expired: revoked on its own, wiped, and off the list.
    auth::issue_link(&pool, Actor::User(me), None, Some("c"), 1)
        .await
        .unwrap();
    sqlx::query("UPDATE member_invites SET expires_at = '2000-01-01T00:00:00Z' WHERE label = 'c'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(members::pending_invites(&pool).await.unwrap().is_empty());

    assert_eq!(stored(pool.clone()).await, vec![None, None, None]);
    let _ = b;
    let revoked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM member_invites WHERE label = 'c' AND revoked_at IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revoked, 1);
}

#[tokio::test]
async fn your_own_sign_out_keeps_the_device_you_asked_from() {
    let pool = pool().await;
    let me = household(&pool).await;
    let here = {
        let mut t = pool.begin().await.unwrap();
        let s = auth::start_session(&mut t, me, "Safari on Mac")
            .await
            .unwrap();
        t.commit().await.unwrap();
        s
    };
    // `household()` left one session; with this one, two devices.
    assert_eq!(members::sign_out_others(&pool, me, &here).await.unwrap(), 1);
    assert!(
        auth::session(&pool, &here).await.unwrap().is_some(),
        "this one stays"
    );
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE member_id = ?1")
        .bind(me)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 1);
    // Nothing else to sign out: nothing written to the log either.
    assert_eq!(members::sign_out_others(&pool, me, &here).await.unwrap(), 0);
}
