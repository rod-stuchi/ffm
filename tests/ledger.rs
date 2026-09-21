//! The pane query, against a real SQLite file and the real seed.
//!
//! Unit tests cover what `LedgerRow` *derives*; this covers what the query is
//! obliged to *carry* — the three facts a single row cannot know about itself.
//! Both halves have to hold for a rendered row to be true.

mod common;

use common::seeded;
use ffm::db;
use ffm::domain::money::{Cents, Currency};
use ffm::domain::period::Period;
use ffm::domain::status::RowStatus;
use ffm::ui::row::{Gutter, RowCtx};

fn ctx() -> RowCtx {
    RowCtx {
        // Pinned, never `Local::now()`: half the row states are functions of
        // the date, so a real clock would make this test pass in September and
        // fail in October.
        today: chrono::NaiveDate::from_ymd_opt(2026, 9, 21).unwrap(),
        currency: Currency::Brl,
        show_recurring: true,
        show_delta: true,
        narrow: false,
    }
}

#[tokio::test]
async fn a_month_reads_back_as_the_household_typed_it() {
    let pool = seeded().await;

    let h = db::ledger::household(&pool).await.unwrap().unwrap();
    assert_eq!(h.name, "Casa Stuchi");
    assert_eq!(h.currency, Currency::Brl);

    let members = db::ledger::members(&pool).await.unwrap();
    assert_eq!(
        members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        ["Rodrigo", "Natália", "Miguel"]
    );
    assert!(members[0].is_admin, "role 0 is the admin");

    let sep = Period::parse("2026-09").unwrap();
    let pane = db::ledger::pane(&pool, &members[0], sep, ctx())
        .await
        .unwrap();

    // Manual order, not insertion order: ORDER BY sort_key is what J/K moves.
    assert_eq!(
        pane.rows
            .iter()
            .map(|r| r.title.as_str())
            .collect::<Vec<_>>(),
        [
            "Salário",
            "Energia · Enel SP",
            "Água · Sabesp",
            "Internet · Vivo Fibra",
            "Financiamento · Carro",
            "Mercado · Pão de Açúcar",
            "Presente · aniversário",
        ]
    );

    // Totals are a sum of the rows, and direction lives in `kind` — never in
    // the sign of a stored amount.
    assert_eq!(pane.ins, Cents(980_000));
    assert_eq!(
        pane.outs,
        Cents(34362 + 10840 + 12990 + 89700 + 65890 + 24000)
    );
    assert_eq!(pane.balance(), pane.ins - pane.outs);

    // The gutter numbers the rows as rendered, so `3j` lands where it looks.
    assert_eq!(pane.rows[2].gutter, Gutter::Absolute(3));
}

#[tokio::test]
async fn the_delta_matches_the_same_line_in_the_previous_period() {
    let pool = seeded().await;
    let me = &db::ledger::members(&pool).await.unwrap()[0];
    let pane = db::ledger::pane(&pool, me, Period::parse("2026-09").unwrap(), ctx())
        .await
        .unwrap();

    let by = |t: &str| pane.rows.iter().find(|r| r.title == t).unwrap();

    // A recurring row: the same rule, last month.
    assert_eq!(
        by("Energia · Enel SP").prev_amount_cents,
        Some(Cents(28140))
    );
    // And a one-off, which has no rule at all — matched on (member, title),
    // which is what a household means by "the same line".
    assert_eq!(
        by("Mercado · Pão de Açúcar").prev_amount_cents,
        Some(Cents(70090))
    );
    // A line with no previous occurrence gets no delta rather than a fake 0%.
    assert_eq!(by("Presente · aniversário").prev_amount_cents, None);

    // The delta is derived from that, never stored.
    assert_eq!(by("Energia · Enel SP").delta().map(|d| d.pct), Some(22));
    let mercado = by("Mercado · Pão de Açúcar").delta().unwrap();
    assert_eq!((mercado.pct, mercado.up), (-6, false));
}

#[tokio::test]
async fn installments_count_occurrences_up_to_this_period() {
    let pool = seeded().await;
    let me = &db::ledger::members(&pool).await.unwrap()[0];

    for (period, want) in [("2026-08", 1), ("2026-09", 2)] {
        let pane = db::ledger::pane(&pool, me, Period::parse(period).unwrap(), ctx())
            .await
            .unwrap();
        let car = pane
            .rows
            .iter()
            .find(|r| r.title == "Financiamento · Carro")
            .unwrap();
        assert_eq!(car.installments_paid, Some(want), "{period}");
        assert_eq!(car.total_installments, Some(12));
    }
}

#[tokio::test]
async fn a_tracked_row_with_no_file_reads_pending_not_paid() {
    // The filesystem is authoritative for `paid`, and the vault does not exist
    // yet — so a tracked occurrence must say "nothing has happened" rather than
    // inheriting a claim from the database.
    let pool = seeded().await;
    let me = &db::ledger::members(&pool).await.unwrap()[0];
    let pane = db::ledger::pane(&pool, me, Period::parse("2026-09").unwrap(), ctx())
        .await
        .unwrap();

    let energia = pane
        .rows
        .iter()
        .find(|r| r.title == "Energia · Enel SP")
        .unwrap();
    assert!(energia.tracked);
    assert_eq!(energia.status(), RowStatus::Pending);

    // Untracked and ticked is the one manual assertion the model allows.
    let internet = pane
        .rows
        .iter()
        .find(|r| r.title == "Internet · Vivo Fibra")
        .unwrap();
    assert!(!internet.tracked);
    assert_eq!(internet.status(), RowStatus::ManualPaid);

    // Income runs on its own axis and never borrows a spend word.
    let consult = db::ledger::pane(
        &pool,
        &db::ledger::members(&pool).await.unwrap()[1],
        Period::parse("2026-09").unwrap(),
        ctx(),
    )
    .await
    .unwrap();
    let c = consult
        .rows
        .iter()
        .find(|r| r.title == "Consultoria")
        .unwrap();
    assert_eq!(c.status(), RowStatus::Expected);
}

#[tokio::test]
async fn a_private_row_is_still_read_here_and_filtered_later() {
    // Written down as a test rather than a comment: privacy is phase 4, and
    // this query deliberately does NOT filter. If somebody adds the WHERE
    // clause here instead of in the viewer layer, this fails and says why —
    // the filter has to sit where the totals are computed too, or a hidden row
    // leaks by making the numbers not add up.
    let pool = seeded().await;
    let me = &db::ledger::members(&pool).await.unwrap()[0];
    let pane = db::ledger::pane(&pool, me, Period::parse("2026-09").unwrap(), ctx())
        .await
        .unwrap();
    assert!(pane.rows.iter().any(|r| r.flags.private));
}
