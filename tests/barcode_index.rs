//! `idx_tx_barcode`: one barcode, one live row — except a card's payment slip,
//! which names an account rather than a bill (migration 0013).

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::rules::{self, RuleForm};
use ffm::domain::money::Cents;
use ffm::domain::status::{TxKind, Visibility};
use ffm::services::{boleto, materializer};
use sqlx::SqlitePool;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

/// A card rule with two months already materialized; their row ids.
async fn two_months(pool: &SqlitePool) -> (i64, i64) {
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
    rules::create(
        pool,
        Actor::User(me),
        &RuleForm {
            member_id: me,
            kind: TxKind::Out,
            title: "Inter".into(),
            amount: Some(Cents(100_000)),
            cadence_months: 1,
            due_day: Some(9),
            start_date: d(2026, 8, 1),
            end_date: None,
            total_installments: None,
            carry_forward_amount: false,
            tracked: true,
            bucket: Some("Cartão".into()),
            bill_name: Some("Inter".into()),
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
    materializer::reconcile(pool, d(2026, 9, 21)).await.unwrap();
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM transactions ORDER BY period")
        .fetch_all(pool)
        .await
        .unwrap();
    assert_eq!(ids.len(), 2, "August and September");
    (ids[0], ids[1])
}

async fn put(pool: &SqlitePool, id: i64, raw: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE transactions SET barcode_raw = ?2, barcode_kind = 'cobranca' WHERE id = ?1")
        .bind(id)
        .bind(raw)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Synthetic, with valid check digits — the shape Inter prints identically on
/// every month's fatura: factor 0000, value zero.
const CARD_SLIP: &str = "26095000000000000001234500012345000123450000";
/// A dated bill with a real amount.
const ONE_BILL: &str = "34191790010104351004779102015009815700000034362";

#[tokio::test]
async fn a_card_slip_may_sit_on_every_month() {
    let pool = pool().await;
    let (aug, sep) = two_months(&pool).await;
    put(&pool, aug, CARD_SLIP).await.unwrap();
    put(&pool, sep, CARD_SLIP)
        .await
        .expect("the same account-level barcode on another month is not a duplicate");
}

#[tokio::test]
async fn a_real_bill_still_cannot_be_on_two_rows() {
    let pool = pool().await;
    let (aug, sep) = two_months(&pool).await;
    let raw = boleto::parse(ONE_BILL, d(2026, 9, 21)).unwrap().raw;
    put(&pool, aug, &raw).await.unwrap();
    assert!(
        put(&pool, sep, &raw).await.is_err(),
        "a dated, valued barcode is one bill, and the index still says so"
    );
}

#[test]
fn the_sql_and_the_rust_agree_on_which_is_which() {
    // The index's test is `substr(raw, 6, 14)` all zeros on a cobrança;
    // `names_an_account` reads the same digits.
    assert!(boleto::names_an_account(boleto::Kind::Cobranca, CARD_SLIP));
    let one = boleto::parse(ONE_BILL, d(2026, 9, 21)).unwrap();
    assert!(!boleto::names_an_account(one.kind, &one.raw));
    assert_ne!(&one.raw[5..19], "00000000000000");
}
