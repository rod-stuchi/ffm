//! Copying a row with a few columns changed, whatever columns the table has.
//!
//! The column list is read from the table itself, so a migration that adds a
//! column is copied along without this tool having to learn about it.

use sqlx::SqliteConnection;

pub enum V {
    I(i64),
    T(String),
    N,
}

impl From<Option<String>> for V {
    fn from(v: Option<String>) -> V {
        v.map(V::T).unwrap_or(V::N)
    }
}

impl From<Option<i64>> for V {
    fn from(v: Option<i64>) -> V {
        v.map(V::I).unwrap_or(V::N)
    }
}

pub async fn columns(conn: &mut SqliteConnection, table: &str) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info(?1)")
        .bind(table)
        .fetch_all(conn)
        .await?)
}

/// Inserts a copy of `table`'s row `id` with `set` applied, and returns the
/// copy's rowid.
pub async fn clone_row(conn: &mut SqliteConnection, table: &str, id: i64, set: Vec<(&str, V)>) -> anyhow::Result<i64> {
    clone_by(conn, table, "id", id, set).await
}

/// The same, for a table keyed by another column (`member_settings` is keyed
/// by its member) — which `set` must then give a new value.
pub async fn clone_by(
    conn: &mut SqliteConnection,
    table: &str,
    key: &str,
    id: i64,
    set: Vec<(&str, V)>,
) -> anyhow::Result<i64> {
    let cols: Vec<String> = columns(conn, table).await?.into_iter().filter(|c| c != "id").collect();
    let mut binds = Vec::new();
    let exprs: Vec<String> = cols
        .iter()
        .map(|c| match set.iter().position(|(k, _)| k == c) {
            Some(i) => {
                binds.push(i);
                "?".to_string()
            }
            None => format!("\"{c}\""),
        })
        .collect();
    for (k, _) in &set {
        anyhow::ensure!(cols.iter().any(|c| c == k), "{table} has no column {k}");
    }
    let sql = format!(
        "INSERT INTO \"{table}\" ({}) SELECT {} FROM \"{table}\" WHERE \"{key}\" = ?",
        cols.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", "),
        exprs.join(", ")
    );
    let mut q = sqlx::query(&sql);
    let mut set = set;
    // Bind in column order, which is the order the placeholders appear in.
    let mut values: Vec<Option<V>> = set.drain(..).map(|(_, v)| Some(v)).collect();
    for i in binds {
        q = match values[i].take().expect("each value bound once") {
            V::I(x) => q.bind(x),
            V::T(x) => q.bind(x),
            V::N => q.bind(None::<String>),
        };
    }
    let r = q.bind(id).execute(conn).await?;
    Ok(r.last_insert_rowid())
}
