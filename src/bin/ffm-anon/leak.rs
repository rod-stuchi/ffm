//! The gate before the copy is put in place: no original string anywhere.
//!
//! Three looks, because each catches what the others cannot name: every
//! column that holds household content (so a hit says where), every path in
//! the generated vault, and the raw bytes of the vacuumed database file —
//! which is the one that would catch a column this tool never heard of.
//! Any hit and the copy is thrown away.

use crate::shuffle::Originals;
use sqlx::SqliteConnection;
use std::path::Path;

/// Where household content lives. Anything else is identifiers, dates and
/// settings — and the raw-bytes pass covers it anyway.
const COLUMNS: &[(&str, &[&str])] = &[
    ("transactions", &["title", "bucket", "bill_name", "barcode_raw", "issuer_id"]),
    (
        "recurring_rules",
        &["title", "bucket", "bill_name", "instance", "client_code", "issuer_id", "pdf_password"],
    ),
    ("receipts", &["path", "original_filename", "deleted_from_path"]),
    ("change", &["note"]),
    ("change_field", &["old_value", "new_value"]),
    ("rule_recipe", &["cnpj", "issuer_name", "amount_regex", "due_regex", "client_regex", "card_regex"]),
    ("rule_bill_code", &["barcode_raw"]),
    ("rule_barcode_window", &["file_path"]),
    ("members", &["name", "slug", "email"]),
    ("household_settings", &["name"]),
    ("sessions", &["label"]),
    ("member_invites", &["label"]),
];

pub async fn check(conn: &mut SqliteConnection, o: &Originals, db: &Path, vault: &Path) -> anyhow::Result<()> {
    // A string the schema itself contains (a column comment quoting sample
    // data) is in every copy of the database, shuffled or not.
    let schema: String = sqlx::query_scalar("SELECT coalesce(group_concat(sql, ' '), '') FROM sqlite_master")
        .fetch_one(&mut *conn)
        .await?;
    let schema = schema.to_lowercase();

    let text: Vec<String> = o
        .text
        .iter()
        .map(|s| s.to_lowercase())
        .filter(|s| s.chars().count() >= 4)
        .collect();
    let numbers: Vec<&String> = o.numbers.iter().filter(|s| s.len() >= 7).collect();

    let mut hits = Vec::new();
    let look = |where_: String, value: &str, hits: &mut Vec<String>| {
        let lower = value.to_lowercase();
        for t in &text {
            if lower.contains(t.as_str()) {
                hits.push(format!("{where_}: “{t}”"));
            }
        }
        for n in &numbers {
            if contains_number(value, n) {
                hits.push(format!("{where_}: {n}"));
            }
        }
    };

    for (table, cols) in COLUMNS {
        for col in *cols {
            let sql = format!("SELECT rowid, \"{col}\" FROM \"{table}\" WHERE \"{col}\" IS NOT NULL");
            let rows: Vec<(i64, String)> = sqlx::query_as(&sql).fetch_all(&mut *conn).await?;
            for (rowid, v) in rows {
                look(format!("{table}.{col} row {rowid}"), &v, &mut hits);
            }
        }
    }

    for entry in walk(vault)? {
        let rel = entry.strip_prefix(vault).unwrap_or(&entry).to_string_lossy().to_string();
        look(format!("vault file {rel}"), &rel, &mut hits);
    }

    let bytes = std::fs::read(db)?;
    for t in o.text.iter().filter(|s| s.chars().count() >= 4) {
        if schema.contains(&t.to_lowercase()) {
            continue;
        }
        for needle in [t.clone(), t.to_lowercase()] {
            if find(&bytes, needle.as_bytes()) {
                hits.push(format!("database file bytes: “{needle}”"));
            }
        }
    }
    for n in &numbers {
        if !schema.contains(n.as_str()) && find(&bytes, n.as_bytes()) {
            hits.push(format!("database file bytes: {n}"));
        }
    }

    hits.sort();
    hits.dedup();
    if !hits.is_empty() {
        let shown: Vec<&String> = hits.iter().take(25).collect();
        anyhow::bail!(
            "the copy still holds {} original value(s), so it was thrown away:\n  {}{}",
            hits.len(),
            shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  "),
            if hits.len() > 25 { "\n  …" } else { "" }
        );
    }
    println!("· leak check: nothing original left");
    Ok(())
}

/// `n` in `s`, not as part of a longer run of digits.
fn contains_number(s: &str, n: &str) -> bool {
    let mut from = 0;
    while let Some(at) = s[from..].find(n) {
        let start = from + at;
        let end = start + n.len();
        let before = s[..start].chars().next_back().is_some_and(|c| c.is_ascii_digit());
        let after = s[end..].chars().next().is_some_and(|c| c.is_ascii_digit());
        if !before && !after {
            return true;
        }
        from = start + 1;
        while !s.is_char_boundary(from) {
            from += 1;
        }
    }
    false
}

/// `needle` in raw bytes as a word of its own. Records sit back to back in a
/// page, so a bare substring match reads `receipt` + the next record's `u`
/// as `iptu`; a letter, digit or UTF-8 byte on either side means it is part
/// of something else. The column pass, which knows where values end, still
/// matches inside words.
fn find(haystack: &[u8], needle: &[u8]) -> bool {
    let word = |b: u8| b.is_ascii_alphanumeric() || b >= 0x80;
    !needle.is_empty()
        && haystack.windows(needle.len()).enumerate().any(|(i, w)| {
            w == needle
                && (i == 0 || !word(haystack[i - 1]))
                && haystack.get(i + needle.len()).is_none_or(|b| !word(*b))
        })
}

fn walk(dir: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            out.push(p.clone());
            out.extend(walk(&p)?);
        } else {
            out.push(p);
        }
    }
    Ok(out)
}
