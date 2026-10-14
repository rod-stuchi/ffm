//! The trash: what has been deleted and can still be brought back.
//!
//! `u` is a stack — it walks back through *your* last actions in order — so
//! getting back a row deleted three actions ago meant undoing the two newer,
//! unrelated ones first. The trash is the other shape: everything that is gone,
//! each one independently restorable. Only deletions belong here, because only
//! they are independent of one another — undoing an older *edit* out of order
//! would silently overwrite a newer one, and that stays `u`'s job.
//!
//! **Scoped by ledger, not by who deleted it.** A member sees everything gone
//! from a ledger they may write — their own, or every ledger for an admin — so
//! a row an admin deleted from Ana's ledger is Ana's to bring back. That is
//! `Viewer::may_write`, and it also settles privacy: a non-admin only ever sees
//! their own ledger, where every private row is theirs.

use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::viewer::Viewer;
use sqlx::SqlitePool;

/// What is in the trash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    /// A deleted row. `files` is how many of its files went with it, and come
    /// back with it — they are not listed on their own.
    Row { files: i64 },
    /// A removed file on a row that is still live. `kind` is the storage code
    /// (`barcode` · `scheduled` · `receipt`); `replaced` when a newer file
    /// took its place rather than somebody removing it.
    File { kind: String, replaced: bool },
}

#[derive(Clone, Debug)]
pub struct Item {
    /// The row's id, or the `receipts` id for a file.
    pub id: i64,
    pub what: What,
    /// The row the item is, or the row the file belongs to.
    pub tx_id: i64,
    pub member_id: i64,
    pub member: String,
    pub title: String,
    pub period: Period,
    pub amount: Option<Cents>,
    pub is_income: bool,
    /// ISO timestamp, UTC.
    pub deleted_at: String,
    /// Nobody, when the app did it on its own (a startup tidy, a supersede).
    pub deleted_by: Option<String>,
    /// For a file, where it lived — which is also how its copy in the trash
    /// is found (`services::receipts::deleted_copy`).
    pub path: Option<String>,
}

/// The trash as this viewer may see it, newest first, at most `limit` items.
/// The flag says whether there were more.
pub async fn list(
    pool: &SqlitePool,
    viewer: Viewer,
    limit: i64,
) -> sqlx::Result<(Vec<Item>, bool)> {
    let me = viewer.member_id;
    let all = viewer.is_admin();
    // One more than asked, so the page can say it is not the whole trash.
    let fetch = limit + 1;

    let rows = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.period AS "period!: String",
                  t.title AS "title!: String", t.kind AS "kind!: String",
                  t.amount_cents AS "amount_cents: i64",
                  t.member_id AS "member_id!: i64", m.name AS "member!: String",
                  t.deleted_at AS "deleted_at!: String", d.name AS "deleted_by?: String",
                  (SELECT count(*) FROM change c JOIN receipts r ON r.id = c.entity_id
                   WHERE c.entity = 'file' AND c.action = 'file_delete'
                     AND c.note = 'moved with its row'
                     AND r.transaction_id = t.id AND r.deleted_at IS NOT NULL
                     AND NOT EXISTS (SELECT 1 FROM change u WHERE u.undo_of = c.id)
                     -- A retired copy is not coming back with the row.
                     AND NOT EXISTS (SELECT 1 FROM change x
                                     WHERE x.entity = 'file' AND x.entity_id = r.id
                                       AND x.action = 'file_unlink' AND x.note IN (?4, ?5, ?6))
                  ) AS "files!: i64"
           FROM transactions t
           JOIN members m ON m.id = t.member_id
           LEFT JOIN members d ON d.id = t.deleted_by
           WHERE t.deleted_at IS NOT NULL
             AND (t.member_id = ?1 OR ?2)
           ORDER BY t.deleted_at DESC, t.id DESC
           LIMIT ?3"#,
        me,
        all,
        fetch,
        crate::services::receipts::RETIRED[0],
        crate::services::receipts::RETIRED[1],
        crate::services::receipts::RETIRED[2],
    )
    .fetch_all(pool)
    .await?;

    // A file whose row is deleted is not listed: the row stands for it, and
    // restoring the row is what brings it back.
    let files = sqlx::query!(
        r#"SELECT r.id AS "id!: i64", r.kind AS "kind!: String",
                  r.delete_reason AS "reason: String",
                  r.deleted_from_path AS "path: String",
                  r.deleted_at AS "deleted_at!: String", d.name AS "deleted_by?: String",
                  t.id AS "tx_id!: i64", t.period AS "period!: String",
                  t.title AS "title!: String", t.kind AS "tx_kind!: String",
                  t.amount_cents AS "amount_cents: i64",
                  t.member_id AS "member_id!: i64", m.name AS "member!: String"
           FROM receipts r
           JOIN transactions t ON t.id = r.transaction_id
           JOIN members m ON m.id = t.member_id
           LEFT JOIN members d ON d.id = r.deleted_by
           WHERE r.deleted_at IS NOT NULL
             AND t.deleted_at IS NULL
             AND (t.member_id = ?1 OR ?2)
             -- A retired copy can never come back here (`receipts::RETIRED`):
             -- a bill now on another row, whose file stays in `.delete/` and is
             -- just no longer offered, or a file `:prune` removed — a
             -- duplicate, or one kept past the household's time.
             AND NOT EXISTS (SELECT 1 FROM change x
                             WHERE x.entity = 'file' AND x.entity_id = r.id
                               AND x.action = 'file_unlink' AND x.note IN (?4, ?6, ?7))
             -- Nor a file deleted by hand: it left the vault without
             -- passing through the trash, so there is nothing to restore.
             AND NOT EXISTS (SELECT 1 FROM change x
                             WHERE x.entity = 'file' AND x.entity_id = r.id
                               AND x.action = 'file_delete' AND x.note = ?5)
           ORDER BY r.deleted_at DESC, r.id DESC
           LIMIT ?3"#,
        me,
        all,
        fetch,
        crate::services::receipts::RETIRED[0],
        crate::services::receipts::REMOVED_BY_HAND,
        crate::services::receipts::RETIRED[1],
        crate::services::receipts::RETIRED[2],
    )
    .fetch_all(pool)
    .await?;

    let mut items: Vec<Item> = rows
        .into_iter()
        .filter_map(|r| {
            Some(Item {
                id: r.id,
                what: What::Row { files: r.files },
                tx_id: r.id,
                member_id: r.member_id,
                member: crate::ui::fmt::name(&r.member),
                title: r.title,
                period: Period::parse(&r.period)?,
                amount: r.amount_cents.map(Cents),
                is_income: r.kind == "in",
                deleted_at: r.deleted_at,
                deleted_by: r.deleted_by.as_deref().map(crate::ui::fmt::name),
                path: None,
            })
        })
        .chain(files.into_iter().filter_map(|r| {
            Some(Item {
                id: r.id,
                what: What::File {
                    kind: r.kind,
                    replaced: r.reason.as_deref() == Some("superseded"),
                },
                tx_id: r.tx_id,
                member_id: r.member_id,
                member: crate::ui::fmt::name(&r.member),
                title: r.title,
                period: Period::parse(&r.period)?,
                amount: r.amount_cents.map(Cents),
                is_income: r.tx_kind == "in",
                deleted_at: r.deleted_at,
                deleted_by: r.deleted_by.as_deref().map(crate::ui::fmt::name),
                path: r.path,
            })
        }))
        .collect();

    // Both timestamps are the same fixed-width UTC format, so the strings
    // order the way the instants do.
    items.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    let more = items.len() as i64 > limit;
    items.truncate(limit.max(0) as usize);
    Ok((items, more))
}

/// Who owns the row a pick is about, and whether it is still in the trash —
/// the two things a restore has to know before it touches anything.
pub struct Pick {
    pub tx_id: i64,
    pub member_id: i64,
    pub visibility: String,
    pub period: Period,
    /// For a file: its kind, so two picks for one slot can be refused.
    pub kind: Option<String>,
}

/// A deleted row, or `None` when it is not (or no longer) in the trash.
pub async fn deleted_row(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<Pick>> {
    let r = sqlx::query!(
        r#"SELECT member_id AS "member_id!: i64", visibility AS "visibility!: String",
                  period AS "period!: String"
           FROM transactions WHERE id = ?1 AND deleted_at IS NOT NULL"#,
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(r.and_then(|r| {
        Some(Pick {
            tx_id: id,
            member_id: r.member_id,
            visibility: r.visibility,
            period: Period::parse(&r.period)?,
            kind: None,
        })
    }))
}

/// A removed file on a live row, or `None` when it is not in the trash.
pub async fn deleted_file(pool: &SqlitePool, receipt_id: i64) -> sqlx::Result<Option<Pick>> {
    let r = sqlx::query!(
        r#"SELECT t.id AS "tx_id!: i64", t.member_id AS "member_id!: i64",
                  t.visibility AS "visibility!: String", t.period AS "period!: String",
                  r.kind AS "kind!: String"
           FROM receipts r JOIN transactions t ON t.id = r.transaction_id
           WHERE r.id = ?1 AND r.deleted_at IS NOT NULL AND t.deleted_at IS NULL"#,
        receipt_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(r.and_then(|r| {
        Some(Pick {
            tx_id: r.tx_id,
            member_id: r.member_id,
            visibility: r.visibility,
            period: Period::parse(&r.period)?,
            kind: Some(r.kind),
        })
    }))
}

/// The live row already holding this deleted row's bill, if any.
///
/// The same boleto can only ever be one live row (`idx_tx_barcode`), so a
/// deleted row whose bill has since been attached to another row cannot come
/// back while that one holds it. Asked before restoring, so the answer can
/// name the row in the way rather than failing on the index.
///
/// A card's payment slip with no date and no amount names an account, not a
/// bill, and any number of rows may carry it (migration 0013,
/// `boleto::names_an_account`) — so it never holds a restore up.
pub async fn bill_held_by(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar!(
        r#"SELECT o.title AS "title!: String"
           FROM transactions t
           JOIN transactions o ON o.barcode_raw = t.barcode_raw AND o.id <> t.id
           WHERE t.id = ?1 AND t.barcode_raw IS NOT NULL AND o.deleted_at IS NULL
             AND NOT (t.barcode_kind = 'cobranca'
                      AND substr(t.barcode_raw, 6, 14) = '00000000000000')
           LIMIT 1"#,
        id
    )
    .fetch_optional(pool)
    .await
}
