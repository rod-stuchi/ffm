//! What a rule's bill is recognized by beyond its barcode, and the text
//! fingerprint for bills no barcode can tell apart (migration 0014).

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::rules::{self, RuleForm};
use ffm::domain::money::Cents;
use ffm::domain::status::{TxKind, Visibility};
use ffm::services::materializer;
use sqlx::SqlitePool;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn member(pool: &SqlitePool) -> i64 {
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

fn card_rule(me: i64, kind: TxKind) -> RuleForm {
    RuleForm {
        member_id: me,
        kind,
        title: "Cartão".into(),
        amount: Some(Cents(100_000)),
        cadence_months: 1,
        due_day: Some(9),
        start_date: d(2026, 9, 1),
        end_date: None,
        total_installments: None,
        carry_forward_amount: false,
        tracked: true,
        bucket: Some("Cartão".into()),
        bill_name: Some("Cartão".into()),
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

#[tokio::test]
async fn a_rule_remembers_its_card_and_issuer() {
    let pool = pool().await;
    let me = member(&pool).await;
    let id = rules::create(&pool, Actor::User(me), &card_rule(me, TxKind::Out))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE recurring_rules SET card_last4 = '7766', issuer_id = 'inter-cartao' WHERE id = ?1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let rule = rules::get(&pool, id).await.unwrap().unwrap();
    assert_eq!(rule.card_last4.as_deref(), Some("7766"));
    assert_eq!(rule.issuer_id.as_deref(), Some("inter-cartao"));
}

#[tokio::test]
async fn a_card_is_four_digits_or_nothing() {
    let pool = pool().await;
    let me = member(&pool).await;
    let id = rules::create(&pool, Actor::User(me), &card_rule(me, TxKind::Out))
        .await
        .unwrap();
    for bad in ["77a6", "123", "12345"] {
        let r = sqlx::query("UPDATE recurring_rules SET card_last4 = ?2 WHERE id = ?1")
            .bind(id)
            .bind(bad)
            .execute(&pool)
            .await;
        assert!(r.is_err(), "{bad} must be refused");
    }
}

#[tokio::test]
async fn a_card_belongs_to_a_spend_rule_with_a_barcode() {
    // The same rule 0007's trigger states for the rest of the binding.
    let pool = pool().await;
    let me = member(&pool).await;
    let income = rules::create(&pool, Actor::User(me), &card_rule(me, TxKind::In))
        .await
        .unwrap();
    let r = sqlx::query("UPDATE recurring_rules SET card_last4 = '7766' WHERE id = ?1")
        .bind(income)
        .execute(&pool)
        .await;
    assert!(r.is_err(), "an income rule has no card to bind");

    let mut form = card_rule(me, TxKind::Out);
    form.no_barcode = true;
    let wire = rules::create(&pool, Actor::User(me), &form).await.unwrap();
    let r = sqlx::query("UPDATE recurring_rules SET card_last4 = '7766' WHERE id = ?1")
        .bind(wire)
        .execute(&pool)
        .await;
    assert!(r.is_err(), "nor does a bill paid by transfer");
}

/// A bill with no barcode replaces the row's last one, and the sources that
/// claimed that barcode must go with it — the CASE the attach writes.
#[tokio::test]
async fn clearing_a_barcode_clears_what_claimed_it() {
    let pool = pool().await;
    let me = member(&pool).await;
    rules::create(&pool, Actor::User(me), &card_rule(me, TxKind::Out))
        .await
        .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();
    let tx: i64 = sqlx::query_scalar("SELECT id FROM transactions LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE transactions SET barcode_raw = '34197001000000034362043510047791020150098157',
                barcode_kind = 'cobranca', amount_source = 'barcode', due_date_source = 'fator'
         WHERE id = ?1",
    )
    .bind(tx)
    .execute(&pool)
    .await
    .unwrap();

    // Naively, the schema refuses: a source claims a barcode that is gone.
    let naive = sqlx::query(
        "UPDATE transactions SET barcode_raw = NULL, barcode_kind = NULL WHERE id = ?1",
    )
    .bind(tx)
    .execute(&pool)
    .await;
    assert!(naive.is_err());

    // What `attach_to` writes for a text-only bill that read nothing new.
    sqlx::query(
        "UPDATE transactions SET barcode_raw = ?2, barcode_kind = ?2,
               amount_source = coalesce(?3, CASE WHEN ?2 IS NULL AND amount_source = 'barcode'
                                                 THEN NULL ELSE amount_source END),
               due_date_source = coalesce(?3, CASE WHEN ?2 IS NULL
                                                    AND due_date_source IN ('fator', 'free_field')
                                                   THEN NULL ELSE due_date_source END)
         WHERE id = ?1",
    )
    .bind(tx)
    .bind(None::<String>)
    .bind(None::<String>)
    .execute(&pool)
    .await
    .expect("the sources that claimed the barcode go with it");
}

/// The fingerprint column is indexed for live files only.
#[tokio::test]
async fn a_fingerprint_lives_on_the_file() {
    let pool = pool().await;
    let has: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pragma_table_info('receipts') WHERE name = 'text_hash'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(has, 1);
    let index: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = 'idx_receipts_text_hash'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(index.contains("deleted_at IS NULL"));
}
