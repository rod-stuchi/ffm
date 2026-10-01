//! Who is in the household, and the account actions an admin can take:
//! a reset link, signing somebody out, turning them off, and the trash.
//!
//! Issuing a reset hands the admin the ability to write to the ledger as
//! somebody else — the one thing preview-as-non-admin was carefully built *not*
//! to allow. So it is made accountable rather than quietly permitted: every
//! session revoked, and two entries in the append-only log. Neither is
//! optional, and both live in `db::auth::issue_link` where the link is made.

use crate::db::changes::{Actor, Change, MemberAction, Value};
use crate::domain::period::Period;
use crate::ui::fmt;
use chrono::NaiveDate;
use sqlx::{Sqlite, SqlitePool, Transaction};

/// Where a member stands. Only an **active** member can sign in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Active,
    /// Turned off: cannot sign in, and their pane is hidden from the month
    /// they were turned off (`member_inactive`), while the months before
    /// still show them.
    Inactive,
    /// In the trash. Only a member with no rows can be put there — it is for
    /// a mistake, not for somebody who has left; that is `Inactive`.
    Trashed,
}

/// A member as `:members` lists them.
#[derive(Clone, Debug)]
pub struct MemberRow {
    pub id: i64,
    pub name: String,
    pub is_admin: bool,
    /// So `:profile` — and the admin — can see what would be signed out.
    pub sessions: i64,
    pub devices: Vec<String>,
    pub status: Status,
    /// `09/2026` — the month an inactive member's current stretch began.
    pub inactive_since: Option<String>,
    /// Any transaction or rule, deleted or not. A member with some cannot go
    /// to the trash: their history is the household's.
    pub has_rows: bool,
}

/// A link that has been issued and not yet used.
#[derive(Clone, Debug)]
pub struct PendingInvite {
    pub id: i64,
    pub label: Option<String>,
    /// Set when the link is a **password reset** for an existing member.
    pub for_member: Option<String>,
    pub expires: String,
    /// The raw token, so the pane can show and copy the link again. Pending
    /// links always carry one; a row issued before migration 0009 does not,
    /// and can only be revoked and issued again.
    pub token: Option<String>,
}

/// Every member, the trash included — `:members` sorts them into its tabs.
pub async fn list(pool: &SqlitePool) -> sqlx::Result<Vec<MemberRow>> {
    let rows = sqlx::query!(
        r#"SELECT m.id AS "id!: i64", m.name AS "name!: String", m.role AS "role!: i64",
                  m.deleted_at AS "deleted_at: String",
                  (SELECT count(*) FROM sessions s WHERE s.member_id = m.id) AS "sessions!: i64",
                  (SELECT i.from_period FROM member_inactive i
                   WHERE i.member_id = m.id AND i.until_period IS NULL) AS "inactive_from: String",
                  (EXISTS (SELECT 1 FROM transactions t WHERE t.member_id = m.id)
                   OR EXISTS (SELECT 1 FROM recurring_rules r WHERE r.member_id = m.id))
                      AS "has_rows!: bool"
           FROM members m ORDER BY m.id"#
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

        let status = if r.deleted_at.is_some() {
            Status::Trashed
        } else if r.inactive_from.is_some() {
            Status::Inactive
        } else {
            Status::Active
        };
        out.push(MemberRow {
            id: r.id,
            name: r.name,
            is_admin: r.role == 0,
            sessions: r.sessions,
            devices,
            status,
            inactive_since: r
                .inactive_from
                .as_deref()
                .and_then(Period::parse)
                .map(fmt::period),
            has_rows: r.has_rows,
        });
    }
    Ok(out)
}

/// Open links, invites and resets alike, each with its token.
///
/// An expired link is **revoked on its own** first — its token wiped, its
/// `revoked_at` the moment it expired — and so never listed: a link that no
/// longer works is not pending anything. `:members` says beside the list how
/// long a link lasts, which is the whole of what an admin needs to know.
pub async fn pending_invites(pool: &SqlitePool) -> sqlx::Result<Vec<PendingInvite>> {
    let now = crate::clock::stamp();
    sqlx::query!(
        r#"UPDATE member_invites SET revoked_at = expires_at, token = NULL
           WHERE accepted_at IS NULL AND revoked_at IS NULL AND expires_at < ?1"#,
        now
    )
    .execute(pool)
    .await?;

    let rows = sqlx::query!(
        r#"SELECT i.id AS "id!: i64", i.label AS "label: String",
                  i.expires_at AS "expires_at!: String",
                  i.token AS "token: String",
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
            expires: NaiveDate::parse_from_str(&r.expires_at[..10], "%Y-%m-%d")
                .map(fmt::date)
                .unwrap_or(r.expires_at),
            token: r.token,
        })
        .collect())
}

pub async fn revoke_invite(pool: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    let at = crate::clock::stamp();
    let n = sqlx::query!(
        r#"UPDATE member_invites SET revoked_at = ?2, token = NULL
           WHERE id = ?1 AND accepted_at IS NULL AND revoked_at IS NULL"#,
        id,
        at
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n > 0)
}

// ---------------------------------------------------------------------------
// turning a member off, and the trash
// ---------------------------------------------------------------------------

/// Why an admin may not do something to a member. Checked here, in one place,
/// for every account action — the pane hides what does not apply, but the
/// server is the rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not on yourself: you would lock yourself out, and your own password
    /// is recovered from the console, not by an admin link you issue.
    Yourself,
    /// The household would be left with no admin to undo it.
    LastAdmin,
    /// Nobody by that id — a stale page.
    Unknown,
    /// Their history is the household's; turn them off instead.
    HasRows,
}

/// Whether `admin` may act on member `id`, and who that is.
pub async fn may_change(
    pool: &SqlitePool,
    admin: i64,
    id: i64,
) -> sqlx::Result<Result<MemberRow, Refusal>> {
    if admin == id {
        return Ok(Err(Refusal::Yourself));
    }
    let Some(target) = list(pool).await?.into_iter().find(|m| m.id == id) else {
        return Ok(Err(Refusal::Unknown));
    };
    if target.is_admin && target.status == Status::Active {
        let others: i64 = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM members m
               WHERE m.role = 0 AND m.deleted_at IS NULL AND m.id <> ?1
                 AND NOT EXISTS (SELECT 1 FROM member_inactive i
                                 WHERE i.member_id = m.id AND i.until_period IS NULL)"#,
            id
        )
        .fetch_one(pool)
        .await?;
        if others == 0 {
            return Ok(Err(Refusal::LastAdmin));
        }
    }
    Ok(Ok(target))
}

/// Whether a member is turned off right now — asked by the login page, which
/// says so rather than starting a session.
pub async fn is_inactive(pool: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    let n: i64 = sqlx::query_scalar!(
        r#"SELECT count(*) AS "n!: i64" FROM member_inactive
           WHERE member_id = ?1 AND until_period IS NULL"#,
        id
    )
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}

/// Revokes a member's open reset link — somebody who cannot sign in has no
/// use for a way to set a password, and it must not be a way back in.
async fn revoke_reset(
    tx: &mut Transaction<'_, Sqlite>,
    member_id: i64,
    at: &str,
) -> sqlx::Result<()> {
    sqlx::query!(
        r#"UPDATE member_invites SET revoked_at = ?2, token = NULL
           WHERE member_id = ?1 AND accepted_at IS NULL AND revoked_at IS NULL"#,
        member_id,
        at
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Signs them out and records it — part of turning somebody off or trashing
/// them, so a session cannot outlive the decision.
async fn sign_out_in(
    tx: &mut Transaction<'_, Sqlite>,
    admin: i64,
    member_id: i64,
) -> sqlx::Result<()> {
    if crate::db::auth::revoke_all_sessions(tx, member_id).await? > 0 {
        Change::member(Actor::User(admin), MemberAction::SessionsRevoked, member_id)
            .write(tx)
            .await?;
    }
    Ok(())
}

fn status_change(admin: i64, id: i64, from: &str, to: &str) -> Change {
    Change::member(Actor::User(admin), MemberAction::Status, id).diff(
        "status",
        Value::text(from),
        Value::text(to),
    )
}

/// Turn a member off from `from` on — the month on the clock, from the web:
/// they cannot sign in, every session and any open reset link go, and their
/// pane is hidden from that month until they are turned back on. `false`
/// when they already were.
pub async fn disable(pool: &SqlitePool, admin: i64, id: i64, from: Period) -> sqlx::Result<bool> {
    let at = crate::clock::stamp();
    let from = from.to_string();
    let mut tx = pool.begin().await?;
    let n = sqlx::query!(
        r#"INSERT INTO member_inactive (member_id, from_period, created_by, created_at)
           SELECT ?1, ?2, ?3, ?4
           WHERE NOT EXISTS (SELECT 1 FROM member_inactive
                             WHERE member_id = ?1 AND until_period IS NULL)"#,
        id,
        from,
        admin,
        at
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n == 0 {
        return Ok(false);
    }
    sign_out_in(&mut tx, admin, id).await?;
    revoke_reset(&mut tx, id, &at).await?;
    status_change(admin, id, "active", "inactive")
        .write(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// Turn a member back on from this month on. The months they were away stay
/// hidden — the stretch is closed, not deleted — and the materializer never
/// backfills them. Turned back on in the month they were turned off, the
/// stretch is empty and hides nothing.
pub async fn enable(pool: &SqlitePool, admin: i64, id: i64, until: Period) -> sqlx::Result<bool> {
    let until = until.to_string();
    let mut tx = pool.begin().await?;
    let n = sqlx::query!(
        r#"UPDATE member_inactive SET until_period = max(from_period, ?2)
           WHERE member_id = ?1 AND until_period IS NULL"#,
        id,
        until
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n == 0 {
        return Ok(false);
    }
    status_change(admin, id, "inactive", "active")
        .write(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// Into the trash — only a member with no rows at all. Their name stays taken
/// while they are there (`members.name` is UNIQUE), which is what makes
/// [`restore`] safe.
pub async fn trash(pool: &SqlitePool, admin: i64, id: i64) -> sqlx::Result<Result<(), Refusal>> {
    let at = crate::clock::stamp();
    let mut tx = pool.begin().await?;
    let rows: i64 = sqlx::query_scalar!(
        r#"SELECT (SELECT count(*) FROM transactions WHERE member_id = ?1)
                + (SELECT count(*) FROM recurring_rules WHERE member_id = ?1) AS "n!: i64""#,
        id
    )
    .fetch_one(&mut *tx)
    .await?;
    if rows > 0 {
        return Ok(Err(Refusal::HasRows));
    }
    let n = sqlx::query!(
        "UPDATE members SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
        id,
        at
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n > 0 {
        sign_out_in(&mut tx, admin, id).await?;
        revoke_reset(&mut tx, id, &at).await?;
        Change::member(Actor::User(admin), MemberAction::Remove, id)
            .write(&mut tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Ok(()))
}

/// Out of the trash, as they were.
pub async fn restore(pool: &SqlitePool, admin: i64, id: i64) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    let n = sqlx::query!(
        "UPDATE members SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
        id
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n > 0 {
        Change::member(Actor::User(admin), MemberAction::Restore, id)
            .write(&mut tx)
            .await?;
    }
    tx.commit().await?;
    Ok(n > 0)
}

/// Signs a member out of every device but the one asking — the "everywhere"
/// an admin can do to their own account without locking themselves out of
/// the page they are on. Recorded like any other sign-out.
pub async fn sign_out_others(
    pool: &SqlitePool,
    member_id: i64,
    keep_session: &str,
) -> sqlx::Result<u64> {
    let mut tx = pool.begin().await?;
    let n = sqlx::query!(
        "DELETE FROM sessions WHERE member_id = ?1 AND id <> ?2",
        member_id,
        keep_session
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n > 0 {
        Change::member(
            Actor::User(member_id),
            MemberAction::SessionsRevoked,
            member_id,
        )
        .note("signed out of every other device")
        .write(&mut tx)
        .await?;
    }
    tx.commit().await?;
    Ok(n)
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
