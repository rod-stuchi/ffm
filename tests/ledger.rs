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
use ffm::domain::viewer::Viewer;
use ffm::ui::row::{Gutter, LineNumbers, RowCtx};

fn ctx() -> RowCtx {
    RowCtx {
        sort: Default::default(),
        // Pinned, never `Local::now()`: half the row states are functions of
        // the date, so a real clock would make this test pass in September and
        // fail in October.
        today: chrono::NaiveDate::from_ymd_opt(2026, 9, 21).unwrap(),
        currency: Currency::Brl,
        show_recurring: true,
        show_delta: true,
        narrow: false,
        line_numbers: Default::default(),
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
    let pane = db::ledger::pane(
        &pool,
        &members[0],
        sep,
        ctx(),
        Viewer::new(members[0].id, true),
    )
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

    // The gutter numbers the rows as rendered, so `3j` lands where it looks —
    // in whichever of the three modes the member chose. This used to be
    // hardcoded to Absolute in the query, which made `relative` and `none`
    // settings you could pick with nothing happening.
    assert_eq!(pane.rows[2].gutter, Gutter::Relative(2));

    let absolute = db::ledger::pane(
        &pool,
        &members[0],
        sep,
        RowCtx {
            line_numbers: LineNumbers::Absolute,
            ..ctx()
        },
        Viewer::new(members[0].id, true),
    )
    .await
    .unwrap();
    assert_eq!(absolute.rows[2].gutter, Gutter::Absolute(3));

    let off = db::ledger::pane(
        &pool,
        &members[0],
        sep,
        RowCtx {
            line_numbers: LineNumbers::Off,
            ..ctx()
        },
        Viewer::new(members[0].id, true),
    )
    .await
    .unwrap();
    // The cell stays in the grid and only its track narrows — `display: none`
    // on a grid child pulls the row's content into the first track.
    assert_eq!(off.rows[2].gutter, Gutter::Hidden);
}

#[tokio::test]
async fn the_delta_matches_the_same_line_in_the_previous_period() {
    let pool = seeded().await;
    let me = &db::ledger::members(&pool).await.unwrap()[0];
    let pane = db::ledger::pane(
        &pool,
        me,
        Period::parse("2026-09").unwrap(),
        ctx(),
        Viewer::new(me.id, true),
    )
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

/// The row with this title in that month's pane for the first member.
async fn row_in(
    pool: &sqlx::SqlitePool,
    period: &str,
    title: &str,
) -> Option<ffm::ui::row::LedgerRow> {
    let me = &db::ledger::members(pool).await.unwrap()[0];
    let pane = db::ledger::pane(
        pool,
        me,
        Period::parse(period).unwrap(),
        ctx(),
        Viewer::new(me.id, true),
    )
    .await
    .unwrap();
    pane.rows.into_iter().find(|r| r.title == title)
}

#[tokio::test]
async fn installments_count_what_is_paid_up_to_this_period() {
    let pool = seeded().await;

    // Twelve installments from March. Only August and September exist in the
    // seed, both ticked: paid so far is 1, then 2.
    for (period, want) in [("2026-08", 1), ("2026-09", 2)] {
        let car = row_in(&pool, period, "Financiamento · Carro")
            .await
            .unwrap();
        assert_eq!(car.installments(), Some((want, 12)), "{period}");
        let p = car.progress.unwrap();
        assert_eq!(p.paid_cents, Cents(89700 * want as i64));
        // March to July have no rows, so their share of what is left is the
        // rule's amount — a projection, and marked as one.
        assert_eq!(p.to_go_cents, Some(Cents(89700 * (12 - want as i64))));
        assert!(p.estimated);
    }

    // An unpaid month is a gap on every row after it.
    sqlx::query("UPDATE transactions SET manual_paid = 0 WHERE rule_id = 4 AND period = '2026-08'")
        .execute(&pool)
        .await
        .unwrap();
    let car = row_in(&pool, "2026-09", "Financiamento · Carro")
        .await
        .unwrap();
    assert_eq!(car.installments(), Some((1, 12)));
}

#[tokio::test]
async fn a_seasonal_rule_counts_its_months_a_year() {
    let pool = seeded().await;
    sqlx::query(
        "INSERT INTO recurring_rules
            (id, member_id, title, kind, amount_cents, cadence_months, due_day,
             start_date, end_date, total_installments, carry_forward_amount, tracked,
             bucket, bill_name, instance, no_barcode, varies, visibility, months, created_at)
         VALUES (9, 1, 'IPTU', 'out', 12162, 1, 10, '2026-01-01', NULL, NULL, 1, 0,
                 'Moradia', 'IPTU', NULL, 0, 0, 'household', '1,2,3,4,5,6,7,8,9,10',
                 '2026-01-01T09:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let may = chrono::NaiveDate::from_ymd_opt(2026, 5, 20).unwrap();
    ffm::services::materializer::reconcile(&pool, may)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE transactions SET manual_paid = 1
          WHERE rule_id = 9 AND period IN ('2026-01', '2026-02', '2026-04')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Ten a year, three of them paid by May; March is the gap.
    let iptu = row_in(&pool, "2026-05", "IPTU").await.unwrap();
    assert_eq!(iptu.installments(), Some((3, 10)));
    let p = iptu.progress.clone().unwrap();
    assert_eq!(p.paid_cents, Cents(12162 * 3));
    // March and May are rows of their own; June to October are not yet.
    assert_eq!(p.to_go_cents, Some(Cents(12162 * 7)));
    assert!(p.estimated, "June to October have only the rule's amount");

    let v = iptu.view(&ffm::ui::i18n::Catalog::new(ffm::ui::i18n::Lang::En));
    assert_eq!(v.installments.as_deref(), Some("3/10"));
    assert_eq!(
        v.details.progress.as_deref(),
        Some("3 of 10 · R$ 364,86 paid · ~R$ 851,34 to go")
    );
}

#[tokio::test]
async fn an_end_date_gives_an_open_ended_bill_a_count() {
    let pool = seeded().await;
    // Energia has no end: nothing to count to.
    let energia = row_in(&pool, "2026-09", "Energia · Enel SP").await.unwrap();
    assert_eq!(energia.installments(), None);
    assert!(energia.progress.is_none());

    // Canceled after December: January to December is twelve. It is tracked
    // and these tests have no vault, so nothing is confirmed.
    sqlx::query("UPDATE recurring_rules SET end_date = '2026-12-31' WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    let energia = row_in(&pool, "2026-09", "Energia · Enel SP").await.unwrap();
    assert_eq!(energia.installments(), Some((0, 12)));
    // The months with no row are projected from the rule's amount.
    let p = energia.progress.clone().unwrap();
    assert_eq!(p.to_go_cents, Some(Cents(28140 + 34362 + 34362 * 10)));
    assert!(p.estimated);

    // With no amount anywhere — not on the rule, not on any row — what is
    // left is unknown, and the popup says nothing about it rather than
    // "R$ 0,00": those months are not free.
    sqlx::query("UPDATE recurring_rules SET amount_cents = NULL WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE transactions SET amount_cents = NULL WHERE rule_id = 1")
        .execute(&pool)
        .await
        .unwrap();
    let energia = row_in(&pool, "2026-09", "Energia · Enel SP").await.unwrap();
    assert_eq!(energia.progress.as_ref().unwrap().to_go_cents, None);
    let v = energia.view(&ffm::ui::i18n::Catalog::new(ffm::ui::i18n::Lang::En));
    assert_eq!(
        v.details.progress.as_deref(),
        Some("0 of 12 · R$ 0,00 paid")
    );
}

#[tokio::test]
async fn a_tracked_row_with_no_file_reads_pending_not_paid() {
    // The filesystem is authoritative for `paid`, and the vault does not exist
    // yet — so a tracked occurrence must say "nothing has happened" rather than
    // inheriting a claim from the database.
    let pool = seeded().await;
    let me = &db::ledger::members(&pool).await.unwrap()[0];
    let pane = db::ledger::pane(
        &pool,
        me,
        Period::parse("2026-09").unwrap(),
        ctx(),
        Viewer::new(me.id, true),
    )
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
    let her = db::ledger::members(&pool).await.unwrap()[1].clone();
    let consult = db::ledger::pane(
        &pool,
        &her,
        Period::parse("2026-09").unwrap(),
        ctx(),
        Viewer::new(her.id, false),
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
    let pane = db::ledger::pane(
        &pool,
        me,
        Period::parse("2026-09").unwrap(),
        ctx(),
        Viewer::new(me.id, true),
    )
    .await
    .unwrap();
    assert!(pane.rows.iter().any(|r| r.flags.private));
}
