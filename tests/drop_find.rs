//! A bill dropped anywhere on the page: which rows it is recognized for
//! (`web::import::candidates` + `recognize`). Recognized by a rule's existing
//! binding only — never bound here, never guessed.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::recipes;
use ffm::db::rules::{self, RuleForm};
use ffm::domain::money::Cents;
use ffm::domain::period::Period;
use ffm::domain::status::{TxKind, Visibility};
use ffm::domain::viewer::Viewer;
use ffm::services::assistant::Recipe;
use ffm::services::materializer;
use ffm::web::import::{Recognized, candidates, recognize};
use sqlx::SqlitePool;

const EXEMPLO: &str = "11222333000181";
const OUTRA: &str = "18236120000158";

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn p(s: &str) -> Period {
    Period::parse(s).unwrap()
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

async fn join(pool: &SqlitePool, me: i64, name: &str) -> i64 {
    let token = auth::issue_link(pool, Actor::User(me), None, None, 1)
        .await
        .unwrap();
    let sid = auth::accept_invite(pool, &token, name, "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    auth::session(pool, &sid).await.unwrap().unwrap().member.id
}

/// A tracked spend rule from September 2026, taught to read bills from the
/// company with `cnpj` — which also binds it to that CNPJ.
async fn rule(pool: &SqlitePool, owner: i64, title: &str, cnpj: Option<&str>) -> i64 {
    let id = rules::create(
        pool,
        Actor::User(owner),
        &RuleForm {
            member_id: owner,
            kind: TxKind::Out,
            title: title.into(),
            amount: Some(Cents(10_000)),
            cadence_months: 1,
            due_day: Some(9),
            start_date: d(2026, 9, 1),
            end_date: None,
            total_installments: None,
            carry_forward_amount: false,
            tracked: true,
            bucket: Some("Consumo".into()),
            bill_name: Some(title.into()),
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
        },
    )
    .await
    .unwrap();
    if let Some(cnpj) = cnpj {
        recipes::store(
            pool,
            Actor::User(owner),
            id,
            "manual",
            &Recipe {
                issuer_name: Some(title.into()),
                cnpj: Some(cnpj.into()),
                amount_regex: Some(r"Total a pagar\s+R\$\s*([\d.]+,\d{2})".into()),
                due_regex: Some(r"Vencimento\s+(\d{2}/\d{2}/\d{4})".into()),
                client_regex: None,
                card_regex: None,
            },
        )
        .await
        .unwrap();
    }
    id
}

/// A text-only bill from the company with `cnpj` (formatted as printed).
fn bill(cnpj: &str, due: Option<&str>) -> String {
    let printed = format!(
        "{}.{}.{}/{}-{}",
        &cnpj[0..2],
        &cnpj[2..5],
        &cnpj[5..8],
        &cnpj[8..12],
        &cnpj[12..14]
    );
    let due = due.map(|d| format!("Vencimento {d}\n")).unwrap_or_default();
    format!("Conta de exemplo\nCNPJ {printed}\nTotal a pagar R$ 108,40\n{due}")
}

async fn row_of(pool: &SqlitePool, rule: i64, period: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM transactions WHERE rule_id = ?1 AND period = ?2")
        .bind(rule)
        .bind(period)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn find(pool: &SqlitePool, viewer: Viewer, text: &str, on_screen: &str) -> Recognized {
    let candidates = candidates(pool, viewer).await;
    recognize(pool, &candidates, text, p(on_screen), d(2026, 10, 6)).await
}

#[tokio::test]
async fn a_bill_goes_to_the_rule_its_issuer_is_bound_to_in_its_due_month() {
    let pool = pool().await;
    let me = household(&pool).await;
    let agua = rule(&pool, me, "Água", Some(EXEMPLO)).await;
    let _cartao = rule(&pool, me, "Cartão", Some(OUTRA)).await;
    materializer::reconcile(&pool, d(2026, 10, 6))
        .await
        .unwrap();
    let admin = Viewer::new(me, true);

    // Due in October, dropped while September is on screen: October's row.
    let found = find(&pool, admin, &bill(EXEMPLO, Some("09/10/2026")), "2026-09").await;
    assert_eq!(found.matched.len(), 1);
    assert_eq!(found.matched[0].rule_id, agua);
    assert_eq!(found.matched[0].tx_id, row_of(&pool, agua, "2026-10").await);

    // Due in September: September's.
    let found = find(&pool, admin, &bill(EXEMPLO, Some("09/09/2026")), "2026-10").await;
    assert_eq!(found.matched[0].tx_id, row_of(&pool, agua, "2026-09").await);
}

#[tokio::test]
async fn a_bill_with_no_due_date_goes_to_the_month_on_screen() {
    let pool = pool().await;
    let me = household(&pool).await;
    let agua = rule(&pool, me, "Água", Some(EXEMPLO)).await;
    materializer::reconcile(&pool, d(2026, 10, 6))
        .await
        .unwrap();
    let found = find(
        &pool,
        Viewer::new(me, true),
        &bill(EXEMPLO, None),
        "2026-09",
    )
    .await;
    assert_eq!(found.matched.len(), 1);
    assert_eq!(found.matched[0].tx_id, row_of(&pool, agua, "2026-09").await);
}

#[tokio::test]
async fn two_rules_bound_to_the_same_issuer_are_both_offered() {
    let pool = pool().await;
    let me = household(&pool).await;
    let casa = rule(&pool, me, "Água · casa", Some(EXEMPLO)).await;
    let praia = rule(&pool, me, "Água · praia", Some(EXEMPLO)).await;
    materializer::reconcile(&pool, d(2026, 10, 6))
        .await
        .unwrap();
    let found = find(
        &pool,
        Viewer::new(me, true),
        &bill(EXEMPLO, Some("09/10/2026")),
        "2026-10",
    )
    .await;
    let rules: Vec<i64> = found.matched.iter().map(|m| m.rule_id).collect();
    assert_eq!(rules, vec![casa, praia], "a tie is asked, never guessed");
}

#[tokio::test]
async fn an_unbound_rule_is_never_a_candidate() {
    let pool = pool().await;
    let me = household(&pool).await;
    rule(&pool, me, "Água", None).await;
    materializer::reconcile(&pool, d(2026, 10, 6))
        .await
        .unwrap();
    assert!(candidates(&pool, Viewer::new(me, true)).await.is_empty());
    let found = find(
        &pool,
        Viewer::new(me, true),
        &bill(EXEMPLO, Some("09/10/2026")),
        "2026-10",
    )
    .await;
    assert!(found.matched.is_empty());
    assert!(found.no_row.is_none());
}

#[tokio::test]
async fn a_member_is_offered_only_their_own_rules() {
    let pool = pool().await;
    let me = household(&pool).await;
    let ana = join(&pool, me, "ana").await;
    rule(&pool, me, "Água", Some(EXEMPLO)).await;
    materializer::reconcile(&pool, d(2026, 10, 6))
        .await
        .unwrap();
    let found = find(
        &pool,
        Viewer::new(ana, false),
        &bill(EXEMPLO, Some("09/10/2026")),
        "2026-10",
    )
    .await;
    assert!(
        found.matched.is_empty(),
        "the admin's ledger is read-only to Ana"
    );
}

#[tokio::test]
async fn a_bill_for_a_month_with_no_row_says_so() {
    let pool = pool().await;
    let me = household(&pool).await;
    rule(&pool, me, "Água", Some(EXEMPLO)).await;
    materializer::reconcile(&pool, d(2026, 10, 6))
        .await
        .unwrap();
    let found = find(
        &pool,
        Viewer::new(me, true),
        &bill(EXEMPLO, Some("09/03/2027")),
        "2026-10",
    )
    .await;
    assert!(found.matched.is_empty());
    assert_eq!(found.no_row, Some(("Água".to_string(), p("2027-03"))));
}

#[tokio::test]
async fn another_issuers_bill_is_recognized_by_nobody() {
    let pool = pool().await;
    let me = household(&pool).await;
    rule(&pool, me, "Água", Some(EXEMPLO)).await;
    materializer::reconcile(&pool, d(2026, 10, 6))
        .await
        .unwrap();
    let found = find(
        &pool,
        Viewer::new(me, true),
        &bill(OUTRA, Some("09/10/2026")),
        "2026-10",
    )
    .await;
    assert!(found.matched.is_empty());
}
