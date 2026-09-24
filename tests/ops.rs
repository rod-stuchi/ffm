//! The unresolved badge, and the ops that keep the file safe.

mod common;

use chrono::NaiveDate;
use common::pool;
use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::db::ledger;
use ffm::db::rules::{self, RuleForm};
use ffm::db::tx;
use ffm::domain::money::{Cents, Currency};
use ffm::domain::period::Period;
use ffm::domain::receipt_path::Kind;
use ffm::domain::status::{TxKind, Visibility};
use ffm::domain::viewer::Viewer;
use ffm::services::{backup, materializer, receipts};
use sqlx::SqlitePool;
use std::path::PathBuf;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ffm-ops-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

fn tracked(member_id: i64, title: &str, start: NaiveDate) -> RuleForm {
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
    }
}

async fn months(
    pool: &SqlitePool,
    viewer: Viewer,
    today: NaiveDate,
    root: Option<&std::path::Path>,
) -> Vec<String> {
    ledger::unresolved_months(pool, viewer, today, root)
        .await
        .unwrap()
        .into_iter()
        .map(|u| u.period.to_string())
        .collect()
}

#[tokio::test]
async fn an_overdue_tracked_row_with_no_receipt_flags_its_month() {
    let pool = pool().await;
    let vault = scratch("vault");
    let me = household(&pool).await;
    rules::create(
        &pool,
        Actor::User(me),
        &tracked(me, "Energia", d(2026, 6, 1)),
    )
    .await
    .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    // June, July and August are overdue with nothing on disk. September is
    // where the person already is — the badge points at the months they are
    // NOT looking at.
    assert_eq!(
        months(&pool, Viewer::new(me, true), d(2026, 9, 21), Some(&vault)).await,
        ["2026-06", "2026-07", "2026-08"]
    );
}

#[tokio::test]
async fn a_receipt_on_disk_clears_the_month() {
    let pool = pool().await;
    let vault = scratch("clears");
    let me = household(&pool).await;
    rules::create(
        &pool,
        Actor::User(me),
        &tracked(me, "Energia", d(2026, 7, 1)),
    )
    .await
    .unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    let (id, slug): (i64, String) = sqlx::query_as(
        "SELECT t.id, m.slug FROM transactions t JOIN members m ON m.id = t.member_id
          WHERE t.period = '2026-07'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let slot = receipts::Slot {
        transaction_id: id,
        vault: ffm::domain::receipt_path::vault(&slug),
        bucket: "Consumo".into(),
        bill_name: "Energia".into(),
        instance: None,
        period: Period::parse("2026-07").unwrap(),
        amount: Some((Cents(34362), Currency::Brl)),
        amount_in_filename: false,
    };
    receipts::store(
        &pool,
        &vault,
        Actor::User(me),
        &slot,
        Kind::Receipt,
        "r.pdf",
        b"r",
    )
    .await
    .unwrap();

    // The disk decides here as everywhere else.
    assert_eq!(
        months(&pool, Viewer::new(me, true), d(2026, 9, 21), Some(&vault)).await,
        ["2026-08"]
    );
}

#[tokio::test]
async fn an_untracked_bill_is_never_an_issue() {
    let pool = pool().await;
    let vault = scratch("untracked");
    let me = household(&pool).await;
    let mut form = tracked(me, "Internet", d(2026, 7, 1));
    form.tracked = false;
    rules::create(&pool, Actor::User(me), &form).await.unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    // There is no filesystem evidence to act on, so flagging it would be
    // nagging based on nothing.
    assert!(
        months(&pool, Viewer::new(me, true), d(2026, 9, 21), Some(&vault))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn the_lookback_is_the_calendar_year_and_no_further() {
    let pool = pool().await;
    let vault = scratch("year");
    let me = household(&pool).await;
    rules::create(
        &pool,
        Actor::User(me),
        &tracked(me, "Energia", d(2025, 10, 1)),
    )
    .await
    .unwrap();
    materializer::reconcile(&pool, d(2026, 3, 15))
        .await
        .unwrap();

    // A badge that reaches back forever is a badge nobody can ever clear.
    let flagged = months(&pool, Viewer::new(me, true), d(2026, 3, 15), Some(&vault)).await;
    assert_eq!(flagged, ["2026-01", "2026-02"]);
    assert!(!flagged.iter().any(|m| m.starts_with("2025")));
}

#[tokio::test]
async fn a_private_row_only_counts_for_who_can_see_it() {
    let pool = pool().await;
    let vault = scratch("private");
    let me = household(&pool).await;
    let token = auth::issue_link(&pool, Actor::User(me), None, None, 1)
        .await
        .unwrap();
    let sid = auth::accept_invite(&pool, &token, "ana", "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    let them = auth::session(&pool, &sid).await.unwrap().unwrap().member.id;

    let mut form = tracked(me, "Terapia", d(2026, 7, 1));
    form.visibility = Visibility::Private;
    rules::create(&pool, Actor::User(me), &form).await.unwrap();
    materializer::reconcile(&pool, d(2026, 9, 21))
        .await
        .unwrap();

    // Otherwise the dot would announce the existence of something the ledger
    // carefully hides.
    assert!(
        !months(&pool, Viewer::new(me, true), d(2026, 9, 21), Some(&vault))
            .await
            .is_empty()
    );
    assert!(
        months(
            &pool,
            Viewer::new(them, false),
            d(2026, 9, 21),
            Some(&vault)
        )
        .await
        .is_empty()
    );
}

#[tokio::test]
async fn a_deleted_row_stops_being_an_issue() {
    let pool = pool().await;
    let vault = scratch("deleted");
    let me = household(&pool).await;
    rules::create(
        &pool,
        Actor::User(me),
        &tracked(me, "Energia", d(2026, 8, 1)),
    )
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

    assert!(
        months(&pool, Viewer::new(me, true), d(2026, 9, 21), Some(&vault))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_backup_is_a_real_database_not_a_copy_of_a_half_written_one() {
    let pool = pool().await;
    let dir = scratch("backups");
    let me = household(&pool).await;
    rules::create(
        &pool,
        Actor::User(me),
        &tracked(me, "Energia", d(2026, 9, 1)),
    )
    .await
    .unwrap();

    let path = backup::run(&pool, &dir, d(2026, 9, 22)).await.unwrap();
    assert!(path.exists());

    // WAL mode keeps recent commits in a sidecar, so a plain file copy can
    // miss them. VACUUM INTO asks SQLite for a consistent snapshot — and the
    // proof is that the household is actually in it.
    let restored = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT name FROM household_settings WHERE id = 1")
        .fetch_one(&restored)
        .await
        .unwrap();
    assert_eq!(name, "Casa Stuchi");
    let rules_kept: i64 = sqlx::query_scalar("SELECT count(*) FROM recurring_rules")
        .fetch_one(&restored)
        .await
        .unwrap();
    assert_eq!(rules_kept, 1);

    // Twice in a day replaces rather than accumulates, so a restart loop does
    // not fill the disk.
    backup::run(&pool, &dir, d(2026, 9, 22)).await.unwrap();
    let files = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(files, 1);
}
