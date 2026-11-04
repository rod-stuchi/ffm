//! `seeds/e2e.sql` is only applied by `just e2e`, which most commits do not run.
//! A migration that stops it applying should fail here, on the way to a push,
//! rather than at the next browser run.

mod common;

#[tokio::test]
async fn the_e2e_seed_applies_to_the_current_schema() {
    let pool = common::pool().await;
    let sql = include_str!("../seeds/e2e.sql");
    for stmt in sql.split(";\n") {
        if stmt.trim().is_empty() {
            continue;
        }
        sqlx::query(stmt).execute(&pool).await.expect(stmt);
    }

    let (members, tx): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM members), (SELECT count(*) FROM transactions)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((members, tx), (3, 37));
}
