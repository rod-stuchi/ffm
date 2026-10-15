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
    let token = auth::issue_link(&pool, Actor::User(me), None, None, 1)
        .await
        .unwrap();
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

// ---------------------------------------------------------------------------
// a new due day reaches this month's row while it is unpaid
// ---------------------------------------------------------------------------

/// This month's row of the rule, and its stamped due date.
async fn this_month(pool: &SqlitePool, rule: i64) -> (i64, Option<String>) {
    let period = ffm::domain::period::Period::of(ffm::clock::today()).to_string();
    sqlx::query_as("SELECT id, due_date FROM transactions WHERE rule_id = ? AND period = ?")
        .bind(rule)
        .bind(period)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_new_due_day_moves_this_months_unpaid_row_and_u_puts_it_back() {
    use ffm::db::tx::{self, Direction, Scope};
    let pool = pool().await;
    let me = household(&pool).await;
    let today = ffm::clock::today();
    let mut f = form(me);
    f.start_date = d(2026, 1, 1);
    let rule = rules::create(&pool, Actor::User(me), &f).await.unwrap();
    ffm::services::materializer::reconcile(&pool, today)
        .await
        .unwrap();
    let (row, before) = this_month(&pool, rule).await;

    f.due_day = Some(31); // clamped to the month's length
    let edited = rules::edit_reporting(&pool, Actor::User(me), rule, &f)
        .await
        .unwrap()
        .unwrap();
    let period = ffm::domain::period::Period::of(today);
    let expected = period.due_date(31).unwrap();
    assert_eq!(
        edited.moved_due,
        Some(expected),
        "the row a person is looking at follows"
    );
    let (_, after) = this_month(&pool, rule).await;
    assert_eq!(after, Some(expected.format("%Y-%m-%d").to_string()));

    // `u` on the row puts the old date back.
    let next = tx::next(
        &pool,
        Scope {
            me,
            owner: Some(me),
            period: Some(period),
        },
        Direction::Undo,
    )
    .await
    .unwrap();
    let u = next.pick.expect("the restamp is undoable");
    assert_eq!((u.action.as_str(), u.entity_id), ("edit", row));
    assert!(tx::undo(&pool, Actor::User(me), &u).await.unwrap());
    assert_eq!(this_month(&pool, rule).await.1, before);
}

#[tokio::test]
async fn a_paid_row_keeps_the_date_it_was_paid_against() {
    let pool = pool().await;
    let me = household(&pool).await;
    let mut f = form(me);
    f.start_date = d(2026, 1, 1);
    f.tracked = false; // the manual tick is what confirms an untracked row
    let rule = rules::create(&pool, Actor::User(me), &f).await.unwrap();
    ffm::services::materializer::reconcile(&pool, ffm::clock::today())
        .await
        .unwrap();
    let (row, before) = this_month(&pool, rule).await;
    sqlx::query("UPDATE transactions SET manual_paid = 1 WHERE id = ?")
        .bind(row)
        .execute(&pool)
        .await
        .unwrap();

    f.due_day = Some(5);
    let edited = rules::edit_reporting(&pool, Actor::User(me), rule, &f)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(edited.moved_due, None);
    assert_eq!(this_month(&pool, rule).await.1, before);
}

/// This month's row of the rule, and its title.
async fn title_this_month(pool: &SqlitePool, rule: i64) -> (i64, String) {
    let period = ffm::domain::period::Period::of(ffm::clock::today()).to_string();
    sqlx::query_as("SELECT id, title FROM transactions WHERE rule_id = ? AND period = ?")
        .bind(rule)
        .bind(period)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_new_title_renames_this_months_unpaid_row_and_u_puts_it_back() {
    use ffm::db::tx::{self, Direction, Scope};
    let pool = pool().await;
    let me = household(&pool).await;
    let today = ffm::clock::today();
    let mut f = form(me);
    f.start_date = d(2026, 1, 1);
    let rule = rules::create(&pool, Actor::User(me), &f).await.unwrap();
    ffm::services::materializer::reconcile(&pool, today)
        .await
        .unwrap();
    let (row, before) = title_this_month(&pool, rule).await;
    let earlier: String = sqlx::query_scalar(
        "SELECT title FROM transactions WHERE rule_id = ? AND period = '2026-01'",
    )
    .bind(rule)
    .fetch_one(&pool)
    .await
    .unwrap();

    f.title = "Energia · CPFL".into();
    let edited = rules::edit_reporting(&pool, Actor::User(me), rule, &f)
        .await
        .unwrap()
        .unwrap();
    assert!(edited.renamed_row);
    assert_eq!(edited.moved_due, None, "the due day did not change");
    assert_eq!(title_this_month(&pool, rule).await.1, "Energia · CPFL");
    let january: String = sqlx::query_scalar(
        "SELECT title FROM transactions WHERE rule_id = ? AND period = '2026-01'",
    )
    .bind(rule)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(january, earlier, "earlier months keep what they recorded");

    let period = ffm::domain::period::Period::of(today);
    let next = tx::next(
        &pool,
        Scope {
            me,
            owner: Some(me),
            period: Some(period),
        },
        Direction::Undo,
    )
    .await
    .unwrap();
    let u = next.pick.expect("the rename is undoable");
    assert_eq!((u.action.as_str(), u.entity_id), ("edit", row));
    assert!(tx::undo(&pool, Actor::User(me), &u).await.unwrap());
    assert_eq!(title_this_month(&pool, rule).await.1, before);
}

#[tokio::test]
async fn a_rename_leaves_a_paid_row_and_a_row_renamed_by_hand() {
    let pool = pool().await;
    let me = household(&pool).await;
    let mut f = form(me);
    f.start_date = d(2026, 1, 1);
    f.tracked = false;
    let rule = rules::create(&pool, Actor::User(me), &f).await.unwrap();
    ffm::services::materializer::reconcile(&pool, ffm::clock::today())
        .await
        .unwrap();
    let (row, before) = title_this_month(&pool, rule).await;

    // Paid: it keeps the name it was paid under.
    sqlx::query("UPDATE transactions SET manual_paid = 1 WHERE id = ?")
        .bind(row)
        .execute(&pool)
        .await
        .unwrap();
    f.title = "Energia · CPFL".into();
    let edited = rules::edit_reporting(&pool, Actor::User(me), rule, &f)
        .await
        .unwrap()
        .unwrap();
    assert!(!edited.renamed_row);
    assert_eq!(title_this_month(&pool, rule).await.1, before);

    // Unpaid but renamed by hand: that name was about this month.
    sqlx::query(
        "UPDATE transactions SET manual_paid = 0, title = 'Energia · 2 meses' WHERE id = ?",
    )
    .bind(row)
    .execute(&pool)
    .await
    .unwrap();
    f.title = "Energia · Neoenergia".into();
    let edited = rules::edit_reporting(&pool, Actor::User(me), rule, &f)
        .await
        .unwrap()
        .unwrap();
    assert!(!edited.renamed_row);
    assert_eq!(title_this_month(&pool, rule).await.1, "Energia · 2 meses");
}

/// The PDF password opens files, so it is stored as typed — and never written
/// to the log, which only says whether one is set, and that it moved.
#[tokio::test]
async fn the_pdf_password_is_kept_but_never_logged() {
    let pool = pool().await;
    let me = household(&pool).await;
    let mut first = form(me);
    first.pdf_password = Some("123.456".into());
    let id = rules::create(&pool, Actor::User(me), &first).await.unwrap();
    assert_eq!(
        rules::get(&pool, id)
            .await
            .unwrap()
            .unwrap()
            .pdf_password
            .as_deref(),
        Some("123.456")
    );

    // Replaced by another: still on → on, but it is recorded.
    let mut second = form(me);
    second.pdf_password = Some("654.321".into());
    rules::edit(&pool, Actor::User(me), id, &second)
        .await
        .unwrap();
    // And taken away.
    rules::edit(&pool, Actor::User(me), id, &form(me))
        .await
        .unwrap();
    assert_eq!(
        rules::get(&pool, id).await.unwrap().unwrap().pdf_password,
        None
    );

    let fields: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT c.action, f.field, f.old_value, f.new_value FROM change_field f
           JOIN change c ON c.id = f.change_id
          WHERE f.field = 'pdf_password' ORDER BY f.id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let on = Some("1".to_string());
    let off = Some("0".to_string());
    assert_eq!(
        fields,
        vec![
            (
                "rule_create".into(),
                "pdf_password".into(),
                off.clone(),
                on.clone()
            ),
            (
                "rule_edit".into(),
                "pdf_password".into(),
                on.clone(),
                on.clone()
            ),
            ("rule_edit".into(), "pdf_password".into(), on, off),
        ]
    );

    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM change_field
          WHERE old_value LIKE '%456%' OR new_value LIKE '%456%'
             OR old_value LIKE '%321%' OR new_value LIKE '%321%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leaked, 0, "the password itself is never in the log");
}
