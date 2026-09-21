//! Recurring rules: what a save records, and what a rule does not own.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::rules::{self, RuleForm};
use ffm::domain::money::Cents;
use ffm::domain::status::{TxKind, Visibility};
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

fn form(member_id: i64) -> RuleForm {
    RuleForm {
        member_id,
        kind: TxKind::Out,
        title: "Energia · Enel SP".into(),
        amount: Some(Cents(34362)),
        cadence_months: 1,
        due_day: Some(11),
        start_date: d(2026, 9, 1),
        end_date: None,
        total_installments: None,
        carry_forward_amount: true,
        tracked: true,
        bucket: Some("Consumo".into()),
        bill_name: Some("Energia".into()),
        instance: None,
        no_barcode: false,
        varies: false,
        estimate_basis: "lowest".into(),
        estimate_window: 3,
        amount_in_filename: false,
        visibility: Visibility::Household,
    }
}

#[tokio::test]
async fn creating_and_editing_a_rule_are_their_own_verbs_in_the_log() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = rules::create(&pool, Actor::User(me), &form(me))
        .await
        .unwrap();

    let mut edited = form(me);
    edited.due_day = Some(18);
    edited.bucket = Some("Utilidades".into());
    rules::edit(&pool, Actor::User(me), id, &edited)
        .await
        .unwrap();

    let entries: Vec<(String, String, i64)> =
        sqlx::query_as("SELECT action, entity, entity_id FROM change ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        entries,
        vec![
            ("rule_create".into(), "rule".into(), id),
            ("rule_edit".into(), "rule".into(), id),
        ]
    );

    // A rule-level entry has no period: a rule does not live in a month.
    let period: Option<String> =
        sqlx::query_scalar("SELECT period FROM change WHERE action = 'rule_edit'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(period, None);

    let fields: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT f.field, f.old_value, f.new_value FROM change_field f
           JOIN change c ON c.id = f.change_id
          WHERE c.action = 'rule_edit' ORDER BY f.id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        fields,
        vec![
            ("due_day".into(), Some("11".into()), Some("18".into())),
            (
                "bucket".into(),
                Some("Consumo".into()),
                Some("Utilidades".into())
            ),
        ],
        "only what actually moved"
    );
}

#[tokio::test]
async fn a_save_that_changed_nothing_writes_nothing() {
    let pool = pool().await;
    let me = household(&pool).await;
    let id = rules::create(&pool, Actor::User(me), &form(me))
        .await
        .unwrap();
    rules::edit(&pool, Actor::User(me), id, &form(me))
        .await
        .unwrap();

    // A log full of empty edits is a log nobody reads.
    let edits: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE action = 'rule_edit'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(edits, 0);
}

#[tokio::test]
async fn the_bucket_cloud_is_the_members_own_and_spans_rules_and_rows() {
    let pool = pool().await;
    let me = household(&pool).await;
    rules::create(&pool, Actor::User(me), &form(me))
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO transactions (member_id, period, kind, title, bucket, sort_key, created_at)
         VALUES (?1, '2026-09', 'out', 'Mercado', 'Mercado', 'z', '2026-09-01T00:00:00Z')",
    )
    .bind(me)
    .execute(&pool)
    .await
    .unwrap();

    // One cloud, fed by both rules and one-offs — the same values the Bucket
    // combobox suggests wherever it appears.
    assert_eq!(
        rules::buckets(&pool, me).await.unwrap(),
        ["Consumo", "Mercado"]
    );

    // And scoped per member: a kid's school-cafeteria labels have no business
    // appearing in a parent's utility dropdown.
    let token = auth::issue_link(&pool, me, None, None, 1).await.unwrap();
    let sid = auth::accept_invite(&pool, &token, "ana", "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    let them = auth::session(&pool, &sid).await.unwrap().unwrap().member.id;
    assert!(rules::buckets(&pool, them).await.unwrap().is_empty());
}

#[tokio::test]
async fn the_schema_refuses_a_rule_that_contradicts_itself() {
    let pool = pool().await;
    let me = household(&pool).await;

    // The barcode apparatus is spend-only: money arriving has no boleto.
    let mut income_with_barcode = form(me);
    income_with_barcode.kind = TxKind::In;
    income_with_barcode.no_barcode = true;
    assert!(
        rules::create(&pool, Actor::User(me), &income_with_barcode)
            .await
            .is_err()
    );

    // And `varies` is the mirror image — a bill carries its amount forward, it
    // does not get projected.
    let mut spend_that_varies = form(me);
    spend_that_varies.varies = true;
    assert!(
        rules::create(&pool, Actor::User(me), &spend_that_varies)
            .await
            .is_err()
    );

    // A tracked rule must have the two path segments it files under.
    let mut tracked_without_taxonomy = form(me);
    tracked_without_taxonomy.bill_name = None;
    assert!(
        rules::create(&pool, Actor::User(me), &tracked_without_taxonomy)
            .await
            .is_err()
    );
}
