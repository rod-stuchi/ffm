//! Who is in the household, and the two account actions an admin can take.
//!
//! Issuing a reset hands the admin the ability to write to the ledger as
//! somebody else — the one thing preview-as-non-admin was carefully built *not*
//! to allow. So it is made accountable rather than quietly permitted: every
//! session revoked, and two entries in the append-only log. Neither is
//! optional, and both live in `db::auth::issue_link` where the link is made.

use crate::db::changes::{Actor, Change, MemberAction};
use crate::ui::fmt;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::SqlitePool;

/// A member as `:members` lists them.
#[derive(Clone, Debug)]
pub struct MemberRow {
    pub id: i64,
    pub name: String,
    pub is_admin: bool,
    /// So `:profile` — and the admin — can see what would be signed out.
    pub sessions: i64,
    pub devices: Vec<String>,
}

/// A link that has been issued and not yet used.
#[derive(Clone, Debug)]
pub struct PendingInvite {
    pub id: i64,
    pub label: Option<String>,
    /// Set when the link is a **password reset** for an existing member.
    pub for_member: Option<String>,
    pub expires: String,
    pub expired: bool,
}

pub async fn list(pool: &SqlitePool) -> sqlx::Result<Vec<MemberRow>> {
    let rows = sqlx::query!(
        r#"SELECT m.id AS "id!: i64", m.name AS "name!: String", m.role AS "role!: i64",
                  (SELECT count(*) FROM sessions s WHERE s.member_id = m.id) AS "sessions!: i64"
           FROM members m WHERE m.deleted_at IS NULL ORDER BY m.id"#
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::new();
    for r in rows {
        let devices = sqlx::query_scalar!(
            r#"SELECT label AS "label: String" FROM sessions
               WHERE member_id = ?1 ORDER BY last_seen_at DESC"#,
            r.id
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .flatten()
        .collect();

        out.push(MemberRow {
            id: r.id,
            name: r.name,
            is_admin: r.role == 0,
            sessions: r.sessions,
            devices,
        });
    }
    Ok(out)
}

/// Open links, invites and resets alike. An expired one is still listed —
/// knowing a link went stale is why somebody would issue another.
pub async fn pending_invites(pool: &SqlitePool) -> sqlx::Result<Vec<PendingInvite>> {
    let rows = sqlx::query!(
        r#"SELECT i.id AS "id!: i64", i.label AS "label: String",
                  i.expires_at AS "expires_at!: String",
                  m.name AS "for_member: String"
           FROM member_invites i
           LEFT JOIN members m ON m.id = i.member_id
           WHERE i.accepted_at IS NULL AND i.revoked_at IS NULL
           ORDER BY i.created_at DESC"#
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| PendingInvite {
            id: r.id,
            label: r.label,
            for_member: r.for_member,
            expired: DateTime::parse_from_rfc3339(&r.expires_at)
                .is_ok_and(|d| d.with_timezone(&Utc) < Utc::now()),
            expires: NaiveDate::parse_from_str(&r.expires_at[..10], "%Y-%m-%d")
                .map(fmt::date)
                .unwrap_or(r.expires_at),
        })
        .collect())
}

pub async fn revoke_invite(pool: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    let at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let n = sqlx::query!(
        r#"UPDATE member_invites SET revoked_at = ?2
           WHERE id = ?1 AND accepted_at IS NULL AND revoked_at IS NULL"#,
        id,
        at
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n > 0)
}

/// Signs one member out everywhere, and records it.
///
/// Separate from a reset, which does this *and* hands over a link. An admin
/// may want only this — a lost phone — and it is the one place the `sessions`
/// table earns its keep twice over.
pub async fn sign_out_everywhere(
    pool: &SqlitePool,
    admin_id: i64,
    member_id: i64,
) -> sqlx::Result<u64> {
    let mut tx = pool.begin().await?;
    let n = crate::db::auth::revoke_all_sessions(&mut tx, member_id).await?;
    if n > 0 {
        Change::member(
            Actor::User(admin_id),
            MemberAction::SessionsRevoked,
            member_id,
        )
        .write(&mut tx)
        .await?;
    }
    tx.commit().await?;
    Ok(n)
}
