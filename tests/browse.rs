//! Reading a month a different way: sort, folds, `/` and `:grep`.
//!
//! The interesting rules here are all about what a *derived view* is. The
//! moment rows are read in an order the household did not put them in,
//! reordering stops meaning anything — and the app says so rather than
//! pretending.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::ledger::{self, Sort};
use ffm::db::tx::{self, NewTransaction};
use ffm::domain::money::{Cents, Currency};
use ffm::domain::period::Period;
use ffm::domain::status::{TxKind, Visibility};
use ffm::domain::viewer::Viewer;
use ffm::ui::i18n::{Catalog, Lang};
use ffm::ui::row::RowCtx;
use sqlx::SqlitePool;

fn ctx(sort: Sort) -> RowCtx {
    RowCtx {
        today: NaiveDate::from_ymd_opt(2026, 9, 21).unwrap(),
        sort,
        currency: Currency::Brl,
        show_recurring: true,
        show_delta: true,
        narrow: false,
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

async fn add(
    pool: &SqlitePool,
    me: i64,
    title: &str,
    cents: i64,
    bucket: Option<&str>,
    date: Option<NaiveDate>,
) -> i64 {
    tx::add(
        pool,
        Actor::User(me),
        NewTransaction {
            member_id: me,
            period: Period::parse("2026-09").unwrap(),
            kind: TxKind::Out,
            title,
            amount: Some(Cents(cents)),
            date,
            bucket,
            visibility: Visibility::Household,
        },
    )
    .await
    .unwrap()
}

async fn titles(pool: &SqlitePool, me: i64, sort: Sort) -> Vec<String> {
    let member = ledger::members(pool)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == me)
        .unwrap();
    ledger::pane(
        pool,
        &member,
        Period::parse("2026-09").unwrap(),
        ctx(sort),
        Viewer::new(me, true),
    )
    .await
    .unwrap()
    .rows
    .into_iter()
    .map(|r| r.title)
    .collect()
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn three_rows(pool: &SqlitePool) -> i64 {
    let me = household(pool).await;
    add(
        pool,
        me,
        "Energia",
        34362,
        Some("Consumo"),
        Some(d(2026, 9, 11)),
    )
    .await;
    add(
        pool,
        me,
        "Água",
        10840,
        Some("Consumo"),
        Some(d(2026, 9, 18)),
    )
    .await;
    add(
        pool,
        me,
        "Mercado",
        65890,
        Some("Mercado"),
        Some(d(2026, 9, 3)),
    )
    .await;
    me
}

#[tokio::test]
async fn the_three_orders_read_the_same_rows_three_ways() {
    let pool = pool().await;
    let me = three_rows(&pool).await;

    // Manual is the default because this replaces a spreadsheet where rows
    // WERE arranged by hand, and `a` appends.
    assert_eq!(
        titles(&pool, me, Sort::Manual).await,
        ["Energia", "Água", "Mercado"]
    );
    assert_eq!(
        titles(&pool, me, Sort::Date).await,
        ["Mercado", "Energia", "Água"]
    );
    // Biggest first: sorting a ledger by amount is asking what costs the most.
    assert_eq!(
        titles(&pool, me, Sort::Amount).await,
        ["Mercado", "Energia", "Água"]
    );
}

#[tokio::test]
async fn an_unpaid_row_sorts_by_the_day_it_is_due() {
    let pool = pool().await;
    let me = household(&pool).await;
    // Paid on the 20th.
    add(&pool, me, "Paid late", 100, None, Some(d(2026, 9, 20))).await;
    // Not paid at all, but due on the 2nd — it has no paid-on date to sort by,
    // and bunching every unpaid row at one end is the opposite of what
    // sorting by date is for.
    let unpaid = add(&pool, me, "Due early", 100, None, None).await;
    sqlx::query("UPDATE transactions SET due_date = '2026-09-02' WHERE id = ?")
        .bind(unpaid)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        titles(&pool, me, Sort::Date).await,
        ["Due early", "Paid late"]
    );
}

#[tokio::test]
async fn a_move_writes_one_row_and_records_where_it_went() {
    let pool = pool().await;
    let me = three_rows(&pool).await;

    let keys_before: Vec<String> =
        sqlx::query_scalar("SELECT sort_key FROM transactions ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();

    let first: i64 = sqlx::query_scalar("SELECT id FROM transactions ORDER BY sort_key LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        tx::reorder(&pool, Actor::User(me), first, 2, None)
            .await
            .unwrap()
    );

    assert_eq!(
        titles(&pool, me, Sort::Manual).await,
        ["Água", "Mercado", "Energia"]
    );

    // ONE write. That is the whole reason the sort key is fractional: with an
    // integer position this would renumber every row after it, and two people
    // doing that at once is a conflict SQLite cannot help with.
    let keys_after: Vec<String> =
        sqlx::query_scalar("SELECT sort_key FROM transactions ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    let changed = keys_before
        .iter()
        .zip(&keys_after)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(changed, 1);

    let moved: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT f.field, f.old_value, f.new_value FROM change_field f
           JOIN change c ON c.id = f.change_id WHERE c.action = 'reorder'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        moved,
        vec![("position".into(), Some("1".into()), Some("3".into()))]
    );
}

#[tokio::test]
async fn moving_past_either_end_is_a_quiet_no_op() {
    let pool = pool().await;
    let me = three_rows(&pool).await;
    let first: i64 = sqlx::query_scalar("SELECT id FROM transactions ORDER BY sort_key LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    // `K` on the top row should do nothing, quietly — not fail.
    assert!(
        !tx::reorder(&pool, Actor::User(me), first, -1, None)
            .await
            .unwrap()
    );
    assert!(
        !tx::reorder(&pool, Actor::User(me), first, 0, None)
            .await
            .unwrap()
    );
    assert_eq!(titles(&pool, me, Sort::Manual).await[0], "Energia");

    let reorders: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE action = 'reorder'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(reorders, 0, "a no-op writes nothing to the log either");
}

#[tokio::test]
async fn a_derived_view_cannot_be_reordered() {
    // Not a capability check — a statement about what the gesture could even
    // mean. Under `sort = date` the rows are in an order nobody put them in.
    assert!(!Sort::Manual.is_derived());
    assert!(Sort::Date.is_derived());
    assert!(Sort::Amount.is_derived());
    assert_eq!(
        Sort::parse("sideways"),
        Sort::Manual,
        "and an unknown one is manual"
    );
}

#[tokio::test]
async fn grouping_subtotals_are_the_report() {
    let pool = pool().await;
    let me = three_rows(&pool).await;
    // No bucket at all — grouping never silently drops a row.
    add(&pool, me, "Presente", 24000, None, None).await;

    let member = ledger::members(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == me)
        .unwrap();
    let pane = ledger::pane(
        &pool,
        &member,
        Period::parse("2026-09").unwrap(),
        ctx(Sort::Manual),
        Viewer::new(me, true),
    )
    .await
    .unwrap();

    let t = Catalog::new(Lang::En);
    let views = pane.rows.iter().map(|r| r.view(&t)).collect();
    let groups = ffm::web::ledger::group_rows(&pane.rows, views, &t, Currency::Brl);

    assert_eq!(
        groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
        ["Consumo", "Mercado", "Uncategorized"],
        "in the order the buckets first appear, and nothing is dropped"
    );
    // Consumo: 343,62 + 108,40. The subtotal is what the bucket COSTS, which
    // is why `zM` turns the ledger into a per-category summary with no
    // separate screen built for it.
    assert_eq!(groups[0].subtotal, "R$ 452,02");
    assert!(groups[0].negative);
    assert_eq!(groups[0].count, "2 rows");
    assert_eq!(groups[2].count, "1 row");
}

#[tokio::test]
async fn grep_reaches_every_month_and_respects_privacy() {
    let pool = pool().await;
    let me = household(&pool).await;
    let token = auth::issue_link(&pool, Actor::User(me), None, None, 1)
        .await
        .unwrap();
    let sid = auth::accept_invite(&pool, &token, "ana", "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    let them = auth::session(&pool, &sid).await.unwrap().unwrap().member.id;

    add(&pool, me, "Energia · Enel SP", 34362, Some("Consumo"), None).await;
    // Another month entirely, which is the whole point of the verb.
    tx::add(
        &pool,
        Actor::User(me),
        NewTransaction {
            member_id: me,
            period: Period::parse("2026-03").unwrap(),
            kind: TxKind::Out,
            title: "Energia · Enel SP",
            amount: Some(Cents(28000)),
            date: None,
            bucket: None,
            visibility: Visibility::Household,
        },
    )
    .await
    .unwrap();
    tx::add(
        &pool,
        Actor::User(me),
        NewTransaction {
            member_id: me,
            period: Period::parse("2026-09").unwrap(),
            kind: TxKind::Out,
            title: "Energia secreta",
            amount: Some(Cents(100)),
            date: None,
            bucket: None,
            visibility: Visibility::Private,
        },
    )
    .await
    .unwrap();

    let mine = ledger::grep(&pool, Viewer::new(me, true), "energia")
        .await
        .unwrap();
    assert_eq!(mine.len(), 3, "every month, and my own private row");
    assert_eq!(mine[0].period.to_string(), "2026-09", "newest first");
    assert!(mine.iter().any(|h| h.period.to_string() == "2026-03"));

    // A row you cannot see is a row you cannot find, or the search leaks the
    // existence of something the ledger carefully hides.
    let theirs = ledger::grep(&pool, Viewer::new(them, false), "energia")
        .await
        .unwrap();
    assert_eq!(theirs.len(), 2);
    assert!(!theirs.iter().any(|h| h.title == "Energia secreta"));
}

#[tokio::test]
async fn grep_survives_what_people_actually_type() {
    let pool = pool().await;
    let me = household(&pool).await;
    add(&pool, me, "Mercado · Pão de Açúcar", 65890, None, None).await;

    // FTS5 reads a bare term as a query language, so a stray quote or star is
    // a syntax error rather than a search. None of these may fail.
    for term in ["pão", "Açúcar", "merc", "\"", "*", "AND", "a OR b", ""] {
        let hits = ledger::grep(&pool, Viewer::new(me, true), term).await;
        assert!(hits.is_ok(), "{term:?}");
    }
    assert_eq!(
        ledger::grep(&pool, Viewer::new(me, true), "merc")
            .await
            .unwrap()
            .len(),
        1,
        "a prefix finds the row"
    );
    assert!(
        ledger::grep(&pool, Viewer::new(me, true), "")
            .await
            .unwrap()
            .is_empty(),
        "an empty search is not a search for everything"
    );
}

#[tokio::test]
async fn a_deleted_row_is_not_found_but_stays_indexed_for_its_undo() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = add(&pool, me, "Mercado", 65890, None, None).await;
    tx::soft_delete(&pool, Actor::User(me), id).await.unwrap();

    assert!(
        ledger::grep(&pool, Viewer::new(me, true), "mercado")
            .await
            .unwrap()
            .is_empty(),
        "the deleted filter lives in the QUERY, where every other read has it"
    );

    let u = tx::last_undoable(&pool, me).await.unwrap().unwrap();
    tx::undo(&pool, Actor::User(me), &u).await.unwrap();
    assert_eq!(
        ledger::grep(&pool, Viewer::new(me, true), "mercado")
            .await
            .unwrap()
            .len(),
        1,
        "and it comes straight back, because it was never un-indexed"
    );
}
