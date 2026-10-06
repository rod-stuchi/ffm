//! A rule's recipe — how its bills are read — and the startup step that
//! retired `config/issuers.toml` (migration 0016).

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::recipes;
use ffm::db::rules::{self, RuleForm};
use ffm::domain::money::Cents;
use ffm::domain::status::{TxKind, Visibility};
use ffm::services::assistant::Recipe;
use sqlx::SqlitePool;

async fn card_rule(pool: &SqlitePool) -> (i64, i64) {
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
    let id = rules::create(
        pool,
        Actor::User(me),
        &RuleForm {
            member_id: me,
            kind: TxKind::Out,
            title: "Nubank".into(),
            amount: Some(Cents(100_000)),
            cadence_months: 1,
            due_day: Some(9),
            start_date: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            end_date: None,
            total_installments: None,
            carry_forward_amount: false,
            tracked: true,
            bucket: Some("Cartão".into()),
            bill_name: Some("Nubank".into()),
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
    (me, id)
}

fn recipe(amount: &str) -> Recipe {
    Recipe {
        issuer_name: Some("Exemplo Pagamentos S.A.".into()),
        cnpj: Some("11222333000181".into()),
        amount_regex: Some(amount.into()),
        due_regex: None,
        client_regex: None,
        card_regex: None,
    }
}

#[tokio::test]
async fn a_recipe_is_kept_and_a_fix_retires_it_rather_than_overwriting() {
    let pool = pool().await;
    let (me, rule) = card_rule(&pool).await;
    assert!(recipes::live(&pool, rule).await.is_none());

    recipes::store(
        &pool,
        Actor::User(me),
        rule,
        "codex",
        &recipe("first ([\\d,]+)"),
    )
    .await
    .unwrap();
    let live = recipes::live(&pool, rule).await.unwrap();
    assert_eq!(live.source, "codex");
    assert_eq!(live.amount_regex.as_deref(), Some("first ([\\d,]+)"));
    // Storing it binds the rule to the issuer's CNPJ, where nothing was.
    let bound = rules::get(&pool, rule).await.unwrap().unwrap();
    assert_eq!(bound.issuer_id.as_deref(), Some("11222333000181"));

    recipes::store(
        &pool,
        Actor::User(me),
        rule,
        "claude",
        &recipe("second ([\\d,]+)"),
    )
    .await
    .unwrap();
    let live = recipes::live(&pool, rule).await.unwrap();
    assert_eq!(live.source, "claude");
    let all: i64 = sqlx::query_scalar("SELECT count(*) FROM rule_recipe WHERE rule_id = ?1")
        .bind(rule)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(all, 2, "the first is retired, not gone");

    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM change WHERE entity = 'rule' AND entity_id = ?1
            AND note LIKE 'bill recipe taught by %'",
    )
    .bind(rule)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(logged, 2);
}

#[tokio::test]
async fn one_live_recipe_per_rule() {
    let pool = pool().await;
    let (me, rule) = card_rule(&pool).await;
    recipes::store(
        &pool,
        Actor::User(me),
        rule,
        "codex",
        &recipe("a ([\\d,]+)"),
    )
    .await
    .unwrap();
    let second = sqlx::query(
        "INSERT INTO rule_recipe (rule_id, created_at, source) VALUES (?1, 'now', 'manual')",
    )
    .bind(rule)
    .execute(&pool)
    .await;
    assert!(second.is_err(), "the partial unique index");
}

#[tokio::test]
async fn the_retired_profiles_become_the_rules_own_recipes() {
    let pool = pool().await;
    let (_, rule) = card_rule(&pool).await;
    sqlx::query("UPDATE recurring_rules SET issuer_id = 'nubank-cartao' WHERE id = ?1")
        .bind(rule)
        .execute(&pool)
        .await
        .unwrap();
    let dir = std::env::temp_dir().join(format!("ffm-recipes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("issuers.toml");
    std::fs::write(
        &file,
        r#"
[[issuer]]
id      = "nubank-cartao"
name    = "Cartão · Nubank"
detect  = ['\b18\.?236\.?120/?0001-?58\b']
kind    = "cobranca"
barcode = "optional"

[issuer.amount]
when  = "open"
regex = 'no valor de\s+R\$\s*([\d.]+,\d{2})'

[[issuer]]
id     = "placeholder"
name   = "Água"
detect = ['\bnothing\b']
"#,
    )
    .unwrap();

    let done = recipes::convert_profiles(&pool, &file).await.unwrap();
    assert_eq!(done.recipes, 1);
    let r = rules::get(&pool, rule).await.unwrap().unwrap();
    assert_eq!(
        r.issuer_id.as_deref(),
        Some("18236120000158"),
        "bound by CNPJ now"
    );
    let live = recipes::live(&pool, rule).await.unwrap();
    assert_eq!(live.source, "migrated");
    assert_eq!(live.issuer_name.as_deref(), Some("Cartão · Nubank"));
    assert!(live.amount_regex.is_some());

    // A no-op the second time.
    let again = recipes::convert_profiles(&pool, &file).await.unwrap();
    assert_eq!(again.recipes, 0);
}

/// A checkout whose `config/issuers.toml` is already gone still converts —
/// from the frozen copy of the last one shipped.
#[tokio::test]
async fn with_the_file_gone_the_frozen_copy_converts() {
    let pool = pool().await;
    let (_, rule) = card_rule(&pool).await;
    sqlx::query("UPDATE recurring_rules SET issuer_id = 'inter-cartao' WHERE id = ?1")
        .bind(rule)
        .execute(&pool)
        .await
        .unwrap();
    let done = recipes::convert_profiles(&pool, std::path::Path::new("/nonexistent/issuers.toml"))
        .await
        .unwrap();
    assert_eq!(done.recipes, 1);
    let r = rules::get(&pool, rule).await.unwrap().unwrap();
    assert_eq!(r.issuer_id.as_deref(), Some("00416968000101"));
    let live = recipes::live(&pool, rule).await.unwrap();
    assert!(live.card_regex.is_some(), "Inter's card pattern came along");
}
