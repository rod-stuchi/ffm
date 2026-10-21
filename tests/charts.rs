//! `:charts` reads a range of months with one query (`db::charts::rows`)
//! instead of a pane read per month. That is only sound while the two agree,
//! so these hold the query to `pane` — month by month, viewer by viewer — and
//! to the privacy and turned-off rules the pane applies.

mod common;

use chrono::NaiveDate;
use common::seeded;
use ffm::db::{charts, ledger};
use ffm::domain::money::Currency;
use ffm::domain::period::Period;
use ffm::domain::viewer::Viewer;
use ffm::ui::row::RowCtx;
use sqlx::SqlitePool;

fn ctx() -> RowCtx {
    RowCtx {
        today: NaiveDate::from_ymd_opt(2026, 9, 21).unwrap(),
        sort: Default::default(),
        currency: Currency::Brl,
        show_recurring: true,
        show_delta: true,
        narrow: false,
        line_numbers: Default::default(),
    }
}

fn p(s: &str) -> Period {
    Period::parse(s).unwrap()
}

/// What the ledger bar says for one month: every visible pane, summed.
async fn pane_sums(pool: &SqlitePool, viewer: Viewer, period: Period) -> (i64, i64, bool) {
    let (mut ins, mut outs, mut est) = (0, 0, false);
    for m in ledger::members_in(pool, period).await.unwrap() {
        let pane = ledger::pane(pool, &m, period, ctx(), viewer).await.unwrap();
        ins += pane.ins.get();
        outs += pane.outs.get();
        est |= pane.estimated;
    }
    (ins, outs, est)
}

/// What `:charts` reads for the same month.
async fn chart_sums(pool: &SqlitePool, viewer: Viewer, period: Period) -> (i64, i64, bool) {
    let (mut ins, mut outs, mut est) = (0, 0, false);
    for r in charts::rows(pool, None, viewer, period, period)
        .await
        .unwrap()
    {
        let Some(c) = r.cents else { continue };
        if r.is_income() {
            ins += c.get();
            est |= r.estimated;
        } else {
            outs += c.get();
        }
    }
    (ins, outs, est)
}

fn viewers() -> Vec<Viewer> {
    let mut preview = Viewer::new(1, true);
    preview.preview = true;
    vec![
        Viewer::new(1, true),
        preview,
        Viewer::new(2, false),
        Viewer::new(3, false),
    ]
}

#[tokio::test]
async fn every_month_sums_to_what_the_panes_say() {
    let pool = seeded().await;
    // A private row of somebody other than the admin, so an admin's and a
    // previewing admin's sums differ too.
    sqlx::query(
        "UPDATE transactions SET visibility = 'private'
          WHERE id = (SELECT min(id) FROM transactions WHERE member_id = 2 AND period = '2026-09')",
    )
    .execute(&pool)
    .await
    .unwrap();

    for period in [p("2026-08"), p("2026-09")] {
        for v in viewers() {
            assert_eq!(
                chart_sums(&pool, v, period).await,
                pane_sums(&pool, v, period).await,
                "{period} as {v:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_private_row_is_its_owners_and_the_admins() {
    let pool = seeded().await;
    let private: i64 = sqlx::query_scalar(
        "SELECT id FROM transactions WHERE visibility = 'private' AND member_id = 1 LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let sees = |rows: Vec<charts::ChartRow>| rows.iter().any(|r| r.id == private);
    let range = (p("2026-01"), p("2026-12"));

    let owner = charts::rows(&pool, None, Viewer::new(1, true), range.0, range.1);
    assert!(sees(owner.await.unwrap()));
    let other = charts::rows(&pool, None, Viewer::new(2, false), range.0, range.1);
    assert!(!sees(other.await.unwrap()));

    // An admin's own private row stays theirs under preview.
    let mut preview = Viewer::new(1, true);
    preview.preview = true;
    assert!(sees(
        charts::rows(&pool, None, preview, range.0, range.1)
            .await
            .unwrap()
    ));
}

#[tokio::test]
async fn a_turned_off_member_leaves_the_months_they_were_off() {
    let pool = seeded().await;
    sqlx::query("INSERT INTO member_inactive (member_id, from_period, created_at) VALUES (3, '2026-09', '2026-09-01T00:00:00Z')")
        .execute(&pool)
        .await
        .unwrap();
    let rows = charts::rows(
        &pool,
        None,
        Viewer::new(1, true),
        p("2026-08"),
        p("2026-09"),
    )
    .await
    .unwrap();
    assert!(
        rows.iter()
            .all(|r| r.member_id != 3 || r.period < p("2026-09"))
    );
    for v in viewers() {
        assert_eq!(
            chart_sums(&pool, v, p("2026-09")).await,
            pane_sums(&pool, v, p("2026-09")).await
        );
    }
}

#[tokio::test]
async fn the_first_month_is_where_the_years_start() {
    let pool = seeded().await;
    assert_eq!(
        charts::first_period(&pool, Viewer::new(1, true))
            .await
            .unwrap(),
        Some(p("2026-08"))
    );
}
