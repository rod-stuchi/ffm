//! A varying income, projected — and the tilde that says so.
//!
//! The arithmetic is unit-tested in `domain::estimate`; these are about what
//! reaches the ledger: which months count as confirmed, what the totals say
//! when part of them is a guess, and the fact that a projection is never
//! written down.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::ledger;
use ffm::db::rules::{self, RuleForm};
use ffm::domain::money::{Cents, Currency};
use ffm::domain::period::Period;
use ffm::domain::status::{RowStatus, TxKind, Visibility};
use ffm::domain::viewer::Viewer;
use ffm::services::materializer;
use ffm::ui::row::RowCtx;
use sqlx::SqlitePool;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn ctx() -> RowCtx {
    RowCtx {
        today: d(2026, 9, 21),
        sort: Default::default(),
        currency: Currency::Brl,
        show_recurring: true,
        show_delta: true,
        narrow: false,
        line_numbers: Default::default(),
    }
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

/// An untracked varying income, so "confirmed" is the manual tick — the one
/// exception the receipt model allows, and the only one available without a
/// vault.
async fn commission(pool: &SqlitePool, me: i64, basis: &str) -> i64 {
    rules::create(
        pool,
        Actor::User(me),
        &RuleForm {
            member_id: me,
            kind: TxKind::In,
            title: "Consultoria".into(),
            amount: Some(Cents(420_000)),
            cadence_months: 1,
            due_day: Some(15),
            start_date: d(2026, 5, 1),
            end_date: None,
            total_installments: None,
            carry_forward_amount: false,
            tracked: false,
            bucket: Some("Renda".into()),
            bill_name: None,
            instance: None,
            no_barcode: false,
            spans_period: false,
            window_size: None,
            window_start: None,
            varies: true,
            estimate_basis: basis.into(),
            estimate_window: 3,
            amount_in_filename: false,
            months: None,
            visibility: Visibility::Household,
        },
    )
    .await
    .unwrap()
}

/// Sets what a past period actually paid, and whether it was confirmed.
async fn settle(pool: &SqlitePool, period: &str, cents: i64, confirmed: bool) {
    sqlx::query("UPDATE transactions SET amount_cents = ?2, manual_paid = ?3 WHERE period = ?1")
        .bind(period)
        .bind(cents)
        .bind(i64::from(confirmed))
        .execute(pool)
        .await
        .unwrap();
}

async fn pane(pool: &SqlitePool, me: i64, period: &str) -> ledger::Pane {
    let member = ledger::members(pool)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == me)
        .unwrap();
    ledger::pane(
        pool,
        &member,
        Period::parse(period).unwrap(),
        ctx(),
        Viewer::new(me, true),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn an_unconfirmed_month_is_skipped_never_folded_in() {
    let pool = pool().await;
    let me = household(&pool).await;
    commission(&pool, me, "lowest").await;
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    settle(&pool, "2026-05", 900_000, true).await;
    settle(&pool, "2026-06", 380_000, true).await;
    settle(&pool, "2026-07", 100, false).await; // never arrived
    settle(&pool, "2026-08", 520_000, true).await;

    let september = pane(&pool, me, "2026-09").await;
    let row = &september.rows[0];

    // July is skipped rather than counted, so the window reaches back past it
    // to three REAL months: 08, 06 and 05. Two unconfirmed periods in a row
    // would otherwise project a guess from another guess.
    assert!(row.is_estimated());
    assert_eq!(row.estimate_cents, Some(Cents(380_000)));
    assert_eq!(row.status(), RowStatus::Expected);
}

#[tokio::test]
async fn the_basis_is_per_rule_and_lowest_is_the_default() {
    for (basis, want) in [
        ("lowest", 380_000),
        ("last", 520_000),
        ("average", (520_000 + 380_000 + 610_000) / 3),
    ] {
        let pool = pool().await;
        let me = household(&pool).await;
        commission(&pool, me, basis).await;
        materializer::reconcile(&pool, d(2026, 9, 21))
            .await
            .unwrap();

        settle(&pool, "2026-06", 610_000, true).await;
        settle(&pool, "2026-07", 380_000, true).await;
        settle(&pool, "2026-08", 520_000, true).await;

        let row = &pane(&pool, me, "2026-09").await.rows[0];
        assert_eq!(row.estimate_cents, Some(Cents(want)), "basis {basis}");
    }
}

#[tokio::test]
async fn a_rules_first_period_falls_back_to_its_own_amount() {
    let pool = pool().await;
    let me = household(&pool).await;
    commission(&pool, me, "lowest").await;
    materializer::reconcile(&pool, d(2026, 5, 20))
        .await
        .unwrap();

    // Nothing behind it — the identical carve-out a bill's carry-forward has.
    let row = &pane(&pool, me, "2026-05").await.rows[0];
    assert_eq!(row.estimate_cents, Some(Cents(420_000)));
}

#[tokio::test]
async fn confirming_a_period_replaces_the_guess_with_the_fact() {
    let pool = pool().await;
    let me = household(&pool).await;
    commission(&pool, me, "lowest").await;
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    settle(&pool, "2026-08", 520_000, true).await;

    let before = &pane(&pool, me, "2026-09").await.rows[0];
    assert!(before.is_estimated(), "a guess until the money lands");

    settle(&pool, "2026-09", 475_000, true).await;
    let after = &pane(&pool, me, "2026-09").await.rows[0];

    assert!(!after.is_estimated(), "and a fact afterwards");
    assert_eq!(after.status(), RowStatus::Received);
    assert_eq!(after.amount_cents, Some(Cents(475_000)));
}

#[tokio::test]
async fn the_totals_include_the_projection_and_say_that_they_do() {
    let pool = pool().await;
    let me = household(&pool).await;
    commission(&pool, me, "lowest").await;
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    settle(&pool, "2026-08", 380_000, true).await;

    let september = pane(&pool, me, "2026-09").await;

    // Leaving projections out would make a future month's balance useless;
    // putting them in unmarked would present a guess as a fact.
    assert!(september.estimated, "the tilde belongs on INs and Balance");
    assert_eq!(
        september.ins,
        Cents(380_000),
        "the total counts the PROJECTION, not the stored baseline"
    );
    assert_eq!(september.balance(), september.ins - september.outs);
}

#[tokio::test]
async fn a_projection_is_never_written_down() {
    let pool = pool().await;
    let me = household(&pool).await;
    commission(&pool, me, "lowest").await;
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    settle(&pool, "2026-08", 380_000, true).await;

    let row = &pane(&pool, me, "2026-09").await.rows[0];
    assert_eq!(row.estimate_cents, Some(Cents(380_000)));

    // Nothing persists at all. A varying rule does not carry an amount
    // forward — that is the whole difference from a bill — so the stored value
    // stays NULL and the number on screen is derived on read. It therefore
    // follows new history instead of freezing an old guess into the data.
    let stored: Option<i64> =
        sqlx::query_scalar("SELECT amount_cents FROM transactions WHERE period = '2026-09'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, None, "the projection was never written down");

    let edits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM change WHERE action = 'edit' AND entity = 'transaction'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(edits, 0);
}

#[tokio::test]
async fn an_unconfirmed_projection_gets_no_delta() {
    let pool = pool().await;
    let me = household(&pool).await;
    commission(&pool, me, "lowest").await;
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    settle(&pool, "2026-08", 380_000, true).await;

    // Comparing a guess against a fact is not a comparison worth drawing.
    let row = &pane(&pool, me, "2026-09").await.rows[0];
    assert!(row.is_estimated());
    assert!(row.delta().is_none());

    // Confirmed, it compares like any other recurring income — and for income
    // the colour inverts, because more money arriving is good news.
    settle(&pool, "2026-09", 475_000, true).await;
    let confirmed = &pane(&pool, me, "2026-09").await.rows[0];
    let delta = confirmed.delta().expect("a fact to compare");
    assert!(delta.up && delta.good, "a raise never paints itself red");
}

#[tokio::test]
async fn a_fixed_income_is_not_projected_at_all() {
    let pool = pool().await;
    let me = household(&pool).await;
    // `varies` off: a salary carries its own number, and a projection would be
    // inventing doubt where there is none.
    rules::create(
        &pool,
        Actor::User(me),
        &RuleForm {
            member_id: me,
            kind: TxKind::In,
            title: "Salário".into(),
            amount: Some(Cents(980_000)),
            cadence_months: 1,
            due_day: Some(5),
            start_date: d(2026, 8, 1),
            end_date: None,
            total_installments: None,
            carry_forward_amount: true,
            tracked: false,
            bucket: Some("Renda".into()),
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
        },
    )
    .await
    .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    let september = pane(&pool, me, "2026-09").await;
    assert!(!september.rows[0].is_estimated());
    assert!(!september.estimated, "no tilde anywhere");
}
