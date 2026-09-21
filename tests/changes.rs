//! The change log, against the real schema.
//!
//! Most of what makes this table trustworthy is enforced by SQLite, not by
//! Rust — the append-only triggers, the action/entity pairing, the "a user
//! action has an actor" rule. So these tests go through a real database: a unit
//! test of the builder would pass happily while the schema rejected everything
//! it produced.

mod common;

use common::seeded;
use ffm::db::changes::{Actor, Change, FileAction, MemberAction, RuleAction, TxAction, Value};
use ffm::domain::money::Cents;
use ffm::domain::period::Period;
use sqlx::SqlitePool;

async fn entry(pool: &SqlitePool, id: i64) -> (String, String, Option<i64>, Option<String>) {
    sqlx::query_as("SELECT action, entity, member_id, period FROM change WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_edit_records_one_row_per_field_that_actually_moved() {
    let pool = seeded().await;
    let mut tx = pool.begin().await.unwrap();

    let id = Change::transaction(Actor::User(1), TxAction::Edit, 8)
        .from_rule(Some(1))
        .in_period(Period::parse("2026-09").unwrap())
        .diff(
            "amount",
            Value::Money(Some(Cents(34062))),
            Value::Money(Some(Cents(35210))),
        )
        .diff("bucket", Value::text("Consumo"), Value::text("Utilidades"))
        // Unchanged: must not appear, or every save becomes noise the pane has
        // to filter back out.
        .diff("title", Value::text("Energia"), Value::text("Energia"))
        .write(&mut tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let fields: Vec<(String, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT field, old_value, new_value, kind FROM change_field
         WHERE change_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(fields.len(), 2, "only the fields that moved");
    assert_eq!(
        fields[0],
        (
            "amount".into(),
            Some("34062".into()),
            Some("35210".into()),
            "money".into()
        ),
        "money is stored in minor units, never in the household's format"
    );
    assert_eq!(fields[1].0, "bucket");
    assert_eq!(fields[1].3, "text");

    // rule_id is stamped on a transaction change so "this row plus the rule
    // behind it" stays one query.
    let (action, entity, member, period) = entry(&pool, id).await;
    assert_eq!((action.as_str(), entity.as_str()), ("edit", "transaction"));
    assert_eq!(member, Some(1));
    assert_eq!(period.as_deref(), Some("2026-09"));
}

#[tokio::test]
async fn nothing_in_the_log_can_be_rewritten_or_removed() {
    let pool = seeded().await;
    let mut tx = pool.begin().await.unwrap();
    let id = Change::transaction(Actor::User(1), TxAction::Delete, 8)
        .write(&mut tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // The entire value of an audit log is that it cannot be tidied up
    // afterwards, so this is a trigger in the schema rather than a rule in
    // prose — one careless UPDATE away from being untrue otherwise.
    let e = sqlx::query("UPDATE change SET note = 'oops' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("append-only"), "{e}");

    let e = sqlx::query("DELETE FROM change WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("append-only"), "{e}");

    let mut tx = pool.begin().await.unwrap();
    let with_field = Change::transaction(Actor::User(1), TxAction::Edit, 8)
        .diff("title", Value::text("a"), Value::text("b"))
        .write(&mut tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let e = sqlx::query("DELETE FROM change_field WHERE change_id = ?")
        .bind(with_field)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("append-only"), "{e}");
}

#[tokio::test]
async fn an_undo_is_an_entry_pointing_at_what_it_reversed() {
    let pool = seeded().await;
    let mut tx = pool.begin().await.unwrap();

    let deleted = Change::transaction(Actor::User(1), TxAction::Delete, 8)
        .in_period(Period::parse("2026-09").unwrap())
        .write(&mut tx)
        .await
        .unwrap();
    let restored = Change::transaction(Actor::User(1), TxAction::Restore, 8)
        .in_period(Period::parse("2026-09").unwrap())
        .undoing(deleted)
        .write(&mut tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // The log reads "deleted", then "restored · undo of 18:39" — the restore
    // never rewrites the delete out of existence.
    let points_at: Option<i64> = sqlx::query_scalar("SELECT undo_of FROM change WHERE id = ?")
        .bind(restored)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(points_at, Some(deleted));

    let still_there: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE id = ?")
        .bind(deleted)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still_there, 1);
}

#[tokio::test]
async fn an_entry_and_the_row_it_describes_commit_together_or_not_at_all() {
    let pool = seeded().await;

    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE transactions SET amount_cents = 1 WHERE id = 8")
        .execute(&mut *tx)
        .await
        .unwrap();
    Change::transaction(Actor::User(1), TxAction::Edit, 8)
        .diff(
            "amount",
            Value::Money(Some(Cents(34362))),
            Value::Money(Some(Cents(1))),
        )
        .write(&mut tx)
        .await
        .unwrap();
    // The mutation failed after the log was written — both must vanish. This is
    // the whole reason `write` takes the caller's transaction rather than the
    // pool: there is no way to record something that did not happen.
    tx.rollback().await.unwrap();

    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM change")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(logged, 0);
    let amount: Option<i64> =
        sqlx::query_scalar("SELECT amount_cents FROM transactions WHERE id = 8")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(amount, Some(34362), "the row is untouched too");
}

#[tokio::test]
async fn every_action_the_api_can_spell_is_one_the_schema_accepts() {
    // The schema pins action↔entity pairs so `password_reset` on a ledger row
    // is impossible rather than merely wrong. The constructors here take an
    // action enum per entity, so those pairs cannot be spelled — this asserts
    // the two agree, in both directions.
    let pool = seeded().await;
    let mut tx = pool.begin().await.unwrap();

    for a in [
        TxAction::Create,
        TxAction::Edit,
        TxAction::Delete,
        TxAction::Restore,
        TxAction::Reattribute,
        TxAction::Reorder,
        TxAction::Privacy,
        TxAction::Import,
        TxAction::Materialize,
    ] {
        Change::transaction(Actor::User(1), a, 8)
            .write(&mut tx)
            .await
            .unwrap_or_else(|e| panic!("{a:?}: {e}"));
    }
    for a in [
        RuleAction::Create,
        RuleAction::Edit,
        RuleAction::Delete,
        RuleAction::Restore,
    ] {
        Change::rule(Actor::User(1), a, 1)
            .write(&mut tx)
            .await
            .unwrap_or_else(|e| panic!("{a:?}: {e}"));
    }
    for a in [
        FileAction::Attach,
        FileAction::Unlink,
        FileAction::Upload,
        FileAction::Delete,
    ] {
        Change::file(Actor::User(1), a, 1)
            .write(&mut tx)
            .await
            .unwrap_or_else(|e| panic!("{a:?}: {e}"));
    }
    for a in [MemberAction::PasswordReset, MemberAction::SessionsRevoked] {
        Change::member(Actor::User(1), a, 2)
            .write(&mut tx)
            .await
            .unwrap_or_else(|e| panic!("{a:?}: {e}"));
    }

    // And the pairing really is enforced, rather than every action being legal
    // on every entity — which would make the test above meaningless.
    let bad = sqlx::query(
        "INSERT INTO change (at, member_id, source, action, entity, entity_id)
         VALUES ('2026-09-21T10:00:00Z', 1, 'user', 'password_reset', 'transaction', 8)",
    )
    .execute(&mut *tx)
    .await;
    assert!(
        bad.is_err(),
        "password_reset on a transaction must be refused"
    );
}

#[tokio::test]
async fn a_materializer_entry_has_no_actor_and_that_is_allowed() {
    let pool = seeded().await;
    let mut tx = pool.begin().await.unwrap();
    let id = Change::transaction(Actor::Materializer, TxAction::Materialize, 8)
        .from_rule(Some(1))
        .in_period(Period::parse("2026-09").unwrap())
        .write(&mut tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (_, _, member, _) = entry(&pool, id).await;
    assert_eq!(member, None, "nobody typed this");

    // A *user* action without one, though, is refused by the schema.
    let bad = sqlx::query(
        "INSERT INTO change (at, member_id, source, action, entity, entity_id)
         VALUES ('2026-09-21T10:00:00Z', NULL, 'user', 'edit', 'transaction', 8)",
    )
    .execute(&pool)
    .await;
    assert!(bad.is_err(), "a user action must name its actor");
}
