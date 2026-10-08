//! The materializer: reconcile per rule, no watermark, never past this month.
//!
//! Most of these assert something that would *look* fine in production for
//! weeks before anyone noticed — a resurrected deleted row, a month that never
//! appeared, a second copy of an occurrence. They are cheap here and expensive
//! in a household's ledger.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::rules::{self, RuleForm};
use ffm::db::tx;
use ffm::domain::money::Cents;
use ffm::domain::months::Months;
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

fn form(member_id: i64, title: &str, start: NaiveDate) -> RuleForm {
    RuleForm {
        member_id,
        kind: TxKind::Out,
        title: title.into(),
        amount: Some(Cents(34362)),
        cadence_months: 1,
        due_day: Some(11),
        start_date: start,
        end_date: None,
        total_installments: None,
        carry_forward_amount: true,
        tracked: false,
        bucket: Some("Consumo".into()),
        bill_name: Some("Energia".into()),
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
        pdf_password: None,
    }
}

async fn periods(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar("SELECT period FROM transactions ORDER BY period")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_rule_created_today_materializes_its_whole_history() {
    let pool = pool().await;
    let me = household(&pool).await;
    // Six months back — exactly the case a `last_checked` watermark gets
    // wrong, by declaring itself already caught up and never looking.
    rules::create(&pool, Actor::User(me), &form(me, "Energia", d(2026, 4, 1)))
        .await
        .unwrap();

    let r = materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    assert_eq!(r.created, 6);
    assert_eq!(
        periods(&pool).await,
        [
            "2026-04", "2026-05", "2026-06", "2026-07", "2026-08", "2026-09"
        ]
    );
}

#[tokio::test]
async fn it_never_materializes_past_the_current_month() {
    let pool = pool().await;
    let me = household(&pool).await;
    rules::create(&pool, Actor::User(me), &form(me, "Energia", d(2026, 8, 1)))
        .await
        .unwrap();

    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    // Future months stay projections rendered on the fly. Persisting them
    // would be inventing facts about money that has not moved.
    assert_eq!(periods(&pool).await, ["2026-08", "2026-09"]);
}

#[tokio::test]
async fn running_it_twice_changes_nothing() {
    let pool = pool().await;
    let me = household(&pool).await;
    rules::create(&pool, Actor::User(me), &form(me, "Energia", d(2026, 7, 1)))
        .await
        .unwrap();

    let first = materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    let second = materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    // The startup run and an hourly tick can race harmlessly, which is what
    // takes double-insertion off the open concurrency question entirely.
    assert_eq!(first.created, 3);
    assert_eq!(second.created, 0);
    assert_eq!(periods(&pool).await.len(), 3);
}

#[tokio::test]
async fn a_deleted_occurrence_is_not_resurrected_an_hour_later() {
    let pool = pool().await;
    let me = household(&pool).await;
    rules::create(&pool, Actor::User(me), &form(me, "Energia", d(2026, 8, 1)))
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    let id: i64 = sqlx::query_scalar("SELECT id FROM transactions WHERE period = '2026-08'")
        .fetch_one(&pool)
        .await
        .unwrap();
    tx::soft_delete(&pool, Actor::User(me), id).await.unwrap();

    // Deletes are soft, so a `dd`'d occurrence is still a row — and the gap
    // query has to count it as PRESENT, or the reconciler fights the household
    // once an hour, forever.
    let again = materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    assert_eq!(again.created, 0);

    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM transactions WHERE period = '2026-08' AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(live, 0, "it stays deleted");
}

#[tokio::test]
async fn the_due_date_is_stamped_from_the_rule_and_clamped_to_the_month() {
    let pool = pool().await;
    let me = household(&pool).await;
    let mut f = form(me, "Aluguel", d(2026, 1, 1));
    // A rule due on the 31st: February has no such day.
    f.due_day = Some(31);
    rules::create(&pool, Actor::User(me), &f).await.unwrap();

    materializer::reconcile(&pool, d(2026, 4, 15))
        .await
        .unwrap();
    let dates: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT period, due_date FROM transactions ORDER BY period")
            .fetch_all(&pool)
            .await
            .unwrap();

    assert_eq!(dates[0].1.as_deref(), Some("2026-01-31"));
    assert_eq!(
        dates[1].1.as_deref(),
        Some("2026-02-28"),
        "clamped, not skipped"
    );
    assert_eq!(dates[2].1.as_deref(), Some("2026-03-31"));
    assert_eq!(dates[3].1.as_deref(), Some("2026-04-30"), "April has 30");
}

#[tokio::test]
async fn editing_the_due_day_changes_future_occurrences_only() {
    let pool = pool().await;
    let me = household(&pool).await;
    let f = form(me, "Energia", d(2026, 7, 1));
    let rule = rules::create(&pool, Actor::User(me), &f).await.unwrap();
    materializer::reconcile(&pool, d(2026, 8, 20))
        .await
        .unwrap();

    let mut edited = f.clone();
    edited.due_day = Some(20);
    rules::edit(&pool, Actor::User(me), rule, &edited)
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    let dates: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT period, due_date FROM transactions ORDER BY period")
            .fetch_all(&pool)
            .await
            .unwrap();
    // Past occurrences are left exactly as they were recorded, which is what
    // the rule form's own notebox promises.
    assert_eq!(dates[0].1.as_deref(), Some("2026-07-11"));
    assert_eq!(dates[1].1.as_deref(), Some("2026-08-11"));
    assert_eq!(dates[2].1.as_deref(), Some("2026-09-20"));
}

#[tokio::test]
async fn carry_forward_seeds_from_the_previous_period_and_off_seeds_nothing() {
    let pool = pool().await;
    let me = household(&pool).await;
    let f = form(me, "Energia", d(2026, 8, 1));
    let rule = rules::create(&pool, Actor::User(me), &f).await.unwrap();
    materializer::reconcile(&pool, d(2026, 8, 20))
        .await
        .unwrap();

    // August's actual bill came in higher than the rule's seed.
    sqlx::query("UPDATE transactions SET amount_cents = 41000 WHERE period = '2026-08'")
        .execute(&pool)
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    let amounts: Vec<(String, Option<i64>)> =
        sqlx::query_as("SELECT period, amount_cents FROM transactions ORDER BY period")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        amounts[1].1,
        Some(41000),
        "carried from the previous PERIOD"
    );

    // With it off, a new period starts with no amount at all — the
    // `R$ __,__` placeholder — forcing an explicit edit. For a credit-card
    // statement, last month's number would be actively wrong.
    let mut off = f.clone();
    off.carry_forward_amount = false;
    rules::edit(&pool, Actor::User(me), rule, &off)
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 10, 2))
        .await
        .unwrap();

    let october: Option<i64> =
        sqlx::query_scalar("SELECT amount_cents FROM transactions WHERE period = '2026-10'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(october, None);
}

#[tokio::test]
async fn a_finite_run_stops_at_its_installment_count() {
    let pool = pool().await;
    let me = household(&pool).await;
    let mut f = form(me, "Financiamento · Carro", d(2026, 3, 5));
    f.total_installments = Some(4);
    f.end_date = rules::end_from_installments(d(2026, 3, 5), 1, 4);
    rules::create(&pool, Actor::User(me), &f).await.unwrap();

    materializer::reconcile(&pool, d(2026, 12, 1))
        .await
        .unwrap();
    assert_eq!(
        periods(&pool).await,
        ["2026-03", "2026-04", "2026-05", "2026-06"],
        "four installments, then it is done — permanently"
    );
}

#[tokio::test]
async fn cadence_skips_the_months_in_between() {
    let pool = pool().await;
    let me = household(&pool).await;
    let mut f = form(me, "IPVA", d(2026, 1, 10));
    f.cadence_months = 3;
    rules::create(&pool, Actor::User(me), &f).await.unwrap();

    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    assert_eq!(periods(&pool).await, ["2026-01", "2026-04", "2026-07"]);
}

#[tokio::test]
async fn a_deleted_rule_stops_producing_rows_but_keeps_the_ones_it_made() {
    let pool = pool().await;
    let me = household(&pool).await;
    let rule = rules::create(&pool, Actor::User(me), &form(me, "Energia", d(2026, 7, 1)))
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 8, 20))
        .await
        .unwrap();

    rules::soft_delete(&pool, Actor::User(me), rule)
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    // The occurrences it already made are what happened; deleting the rule
    // that produced them would rewrite history. What stops is the future.
    assert_eq!(periods(&pool).await, ["2026-07", "2026-08"]);
}

#[tokio::test]
async fn every_materialized_row_says_nobody_typed_it() {
    let pool = pool().await;
    let me = household(&pool).await;
    rules::create(&pool, Actor::User(me), &form(me, "Energia", d(2026, 9, 1)))
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    let entries: Vec<(String, String, Option<i64>, Option<String>)> = sqlx::query_as(
        "SELECT action, source, member_id, period FROM change
          WHERE entity = 'transaction' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, "materialize");
    assert_eq!(entries[0].1, "materializer");
    assert_eq!(
        entries[0].2, None,
        "attributing this to a person would be a lie"
    );
    assert_eq!(entries[0].3.as_deref(), Some("2026-09"));
}

// ---------------------------------------------------------------------------
// seasonal months — the allow-list, and the ONE place it is read
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_seasonal_rule_never_materializes_the_months_it_skips() {
    let pool = pool().await;
    let me = household(&pool).await;
    // IPTU: ten installments, January to October, then nothing until the next
    // January. The mock demonstrated this in seed data; this is the rule.
    let mut f = form(me, "IPTU", d(2025, 1, 1));
    f.months = Some(Months::parse("1-10").unwrap());
    rules::create(&pool, Actor::User(me), &f).await.unwrap();

    materializer::reconcile(&pool, d(2026, 3, 15))
        .await
        .unwrap();

    let got = periods(&pool).await;
    // Twelve months of 2025 minus November and December, plus Jan-Mar 2026.
    assert!(
        !got.contains(&"2025-11".to_string()),
        "November was skipped"
    );
    assert!(
        !got.contains(&"2025-12".to_string()),
        "December was skipped"
    );
    assert!(got.contains(&"2025-10".to_string()));
    assert!(got.contains(&"2026-01".to_string()));
    assert_eq!(got.len(), 13);
}

#[tokio::test]
async fn a_skipped_month_is_not_a_gap_to_be_filled_on_the_next_tick() {
    // The reconciler has no watermark: it looks for periods with no row. A
    // skipped month IS a period with no row, so without the check it would be
    // materialized on every single tick, forever.
    let pool = pool().await;
    let me = household(&pool).await;
    let mut f = form(me, "IPTU", d(2026, 1, 1));
    f.months = Some(Months::parse("1-10").unwrap());
    rules::create(&pool, Actor::User(me), &f).await.unwrap();

    let first = materializer::reconcile(&pool, d(2026, 12, 20))
        .await
        .unwrap();
    assert_eq!(first.created, 10);
    let again = materializer::reconcile(&pool, d(2026, 12, 20))
        .await
        .unwrap();
    assert_eq!(again.created, 0, "a skipped month must not be re-created");
}

#[tokio::test]
async fn narrowing_the_months_leaves_what_was_already_recorded() {
    // The same rule `due_day` follows: editing a rule changes FUTURE
    // materializations only. A form field must never trigger a silent bulk
    // delete across the ledger.
    let pool = pool().await;
    let me = household(&pool).await;
    let mut f = form(me, "IPTU", d(2026, 1, 1));
    f.months = Some(Months::parse("1-10").unwrap());
    let id = rules::create(&pool, Actor::User(me), &f).await.unwrap();
    materializer::reconcile(&pool, d(2026, 10, 20))
        .await
        .unwrap();
    assert_eq!(periods(&pool).await.len(), 10);

    f.months = Some(Months::parse("1-8").unwrap());
    rules::edit(&pool, Actor::User(me), id, &f).await.unwrap();
    materializer::reconcile(&pool, d(2026, 10, 20))
        .await
        .unwrap();

    let got = periods(&pool).await;
    assert_eq!(got.len(), 10, "September and October stay as recorded");
    assert!(got.contains(&"2026-09".to_string()));
    assert!(got.contains(&"2026-10".to_string()));
}

#[tokio::test]
async fn a_skipped_month_does_not_consume_an_installment() {
    // `total_installments` counts PAYMENTS. A month the rule does not run in
    // is not one, so "12 installments" has to mean twelve actual rows.
    let pool = pool().await;
    let me = household(&pool).await;
    let mut f = form(me, "Parcelado", d(2026, 1, 1));
    f.months = Some(Months::parse("^7,^8").unwrap());
    f.total_installments = Some(4);
    rules::create(&pool, Actor::User(me), &f).await.unwrap();

    materializer::reconcile(&pool, d(2026, 12, 20))
        .await
        .unwrap();
    assert_eq!(
        periods(&pool).await,
        ["2026-01", "2026-02", "2026-03", "2026-04"]
    );
}

#[tokio::test]
async fn months_survive_a_save_and_come_back_as_a_canonical_expression() {
    let pool = pool().await;
    let me = household(&pool).await;
    let mut f = form(me, "IPTU", d(2026, 1, 1));
    // Typed as an exclusion; stored as the list; read back as a range.
    f.months = Some(Months::parse("^11,^12").unwrap());
    let id = rules::create(&pool, Actor::User(me), &f).await.unwrap();

    let stored: String = sqlx::query_scalar("SELECT months FROM recurring_rules WHERE id = ?1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, "1,2,3,4,5,6,7,8,9,10");

    let back = rules::get(&pool, id).await.unwrap().unwrap();
    assert_eq!(back.months.unwrap().to_expression(), "1-10");
}

#[tokio::test]
async fn a_rule_with_no_months_runs_every_month() {
    // NULL is "no opinion", which is a different fact from a list of all
    // twelve — and is why the column is nullable rather than defaulted.
    let pool = pool().await;
    let me = household(&pool).await;
    let id = rules::create(&pool, Actor::User(me), &form(me, "Energia", d(2026, 1, 1)))
        .await
        .unwrap();
    let stored: Option<String> =
        sqlx::query_scalar("SELECT months FROM recurring_rules WHERE id = ?1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, None);

    materializer::reconcile(&pool, d(2026, 12, 20))
        .await
        .unwrap();
    assert_eq!(periods(&pool).await.len(), 12);
}

// ---------------------------------------------------------------------------
// a new month arrives in the order the last one was left in
// ---------------------------------------------------------------------------

/// A pane's titles, in its manual order.
async fn order_of(pool: &SqlitePool, period: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT title FROM transactions WHERE period = ? AND deleted_at IS NULL
         ORDER BY sort_key, id",
    )
    .bind(period)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn row_in(pool: &SqlitePool, period: &str, title: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM transactions WHERE period = ? AND title = ?")
        .bind(period)
        .bind(title)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_new_month_arrives_in_the_order_the_last_one_was_left_in() {
    let pool = pool().await;
    let me = household(&pool).await;
    for title in ["Aluguel", "Luz", "Internet"] {
        rules::create(&pool, Actor::User(me), &form(me, title, d(2026, 8, 1)))
            .await
            .unwrap();
    }
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    assert_eq!(
        order_of(&pool, "2026-09").await,
        ["Aluguel", "Luz", "Internet"]
    );

    // September arranged by hand: Internet first.
    let internet = row_in(&pool, "2026-09", "Internet").await;
    tx::reorder(&pool, Actor::User(me), internet, -2, None)
        .await
        .unwrap();

    materializer::reconcile(&pool, d(2026, 10, 5))
        .await
        .unwrap();
    assert_eq!(
        order_of(&pool, "2026-10").await,
        ["Internet", "Aluguel", "Luz"],
        "October takes September's order, not the rules' order"
    );
}

#[tokio::test]
async fn catching_up_several_months_keeps_the_order_in_each() {
    let pool = pool().await;
    let me = household(&pool).await;
    for title in ["Aluguel", "Luz", "Internet"] {
        rules::create(&pool, Actor::User(me), &form(me, title, d(2026, 8, 1)))
            .await
            .unwrap();
    }
    materializer::reconcile(&pool, d(2026, 8, 21))
        .await
        .unwrap();
    let internet = row_in(&pool, "2026-08", "Internet").await;
    tx::reorder(&pool, Actor::User(me), internet, -2, None)
        .await
        .unwrap();

    // Down across two month boundaries: both arrive in August's order.
    materializer::reconcile(&pool, d(2026, 10, 5))
        .await
        .unwrap();
    for month in ["2026-09", "2026-10"] {
        assert_eq!(
            order_of(&pool, month).await,
            ["Internet", "Aluguel", "Luz"],
            "{month}"
        );
    }
}

#[tokio::test]
async fn a_rule_that_skipped_a_month_takes_its_place_from_the_last_it_ran() {
    let pool = pool().await;
    let me = household(&pool).await;
    rules::create(&pool, Actor::User(me), &form(me, "Aluguel", d(2026, 8, 1)))
        .await
        .unwrap();
    rules::create(&pool, Actor::User(me), &form(me, "Luz", d(2026, 8, 1)))
        .await
        .unwrap();
    // Not in September.
    let mut school = form(me, "Escola", d(2026, 8, 1));
    school.months = Some(Months::parse("^9").unwrap());
    rules::create(&pool, Actor::User(me), &school)
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    assert_eq!(order_of(&pool, "2026-09").await, ["Aluguel", "Luz"]);

    // August arranged by hand: Escola first.
    let escola = row_in(&pool, "2026-08", "Escola").await;
    tx::reorder(&pool, Actor::User(me), escola, -2, None)
        .await
        .unwrap();

    materializer::reconcile(&pool, d(2026, 10, 5))
        .await
        .unwrap();
    // Escola's last month was August, where it sat above Aluguel.
    assert_eq!(
        order_of(&pool, "2026-10").await,
        ["Escola", "Aluguel", "Luz"]
    );
}
