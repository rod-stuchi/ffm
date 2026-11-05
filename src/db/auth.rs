//! Accounts, sessions, invites and resets.
//!
//! Sized deliberately for a LAN/Tailscale deployment: the network boundary does
//! most of the real work, so there is no lockout, no rate limiting and no mail
//! sender. Revisit every line of that the day this is exposed to the open
//! internet — it is not designed for that today.
//!
//! Two decisions here look inconsistent and are not:
//!
//! * **Passwords are Argon2.** They are chosen by people, so they are
//!   guessable, so hashing has to be slow.
//! * **Invite tokens are SHA-256.** There is nothing to brute-force in 256 bits
//!   of randomness, and a slow hash on every `GET /invite/:token` is a
//!   self-inflicted denial of service. Recorded because the obvious "fix" is to
//!   make it match the passwords.

use crate::db::changes::{Actor, Change, MemberAction};
use crate::db::ledger::Member;
use crate::domain::slug::slugify;
use argon2::Argon2;

/// The log's note on a reset issued from the console (`ffm reset-link`).
/// Stored in English, like every note; `ui::log::note_text` words it.
pub const CONSOLE_RESET: &str = "reset link issued from the console";
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, SqlitePool, Transaction};

/// Below this, a password is not worth hashing. There is no complexity rule on
/// top: length is the only requirement that survives contact with a person who
/// has to type it on a phone.
pub const MIN_PASSWORD: usize = 8;

/// How long an invite or reset link stays good. It is handed over in person or
/// over a chat app, so the number is short and named rather than configurable.
/// Past it the link is revoked on its own (`members::pending_invites`) and
/// leaves the list — `:members` says so beside the list.
pub const LINK_DAYS: i64 = 5;

fn now() -> String {
    crate::clock::stamp()
}

// ---------------------------------------------------------------------------
// passwords
// ---------------------------------------------------------------------------

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    // argon2's password-hash still rides rand_core 0.6 while the rest of the
    // crate is on rand 0.9, so the salt is filled here and encoded rather than
    // routed through two incompatible RNG traits.
    let mut raw = [0u8; 16];
    rand::rng().fill_bytes(&mut raw);
    let salt = SaltString::encode_b64(&raw).map_err(|e| anyhow::anyhow!("salt: {e}"))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hashing failed: {e}"))
}

/// False for anything that is not a valid PHC string, which is what makes the
/// seed's `!seed-cannot-sign-in` placeholder unusable rather than merely
/// unlikely.
pub fn verify_password(password: &str, stored: &str) -> bool {
    match PasswordHash::new(stored) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// sessions
// ---------------------------------------------------------------------------

/// Who is signed in, and with what powers *right now*.
#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub member: Member,
    /// The role on the row. Whether admin powers are actually in effect is
    /// `Viewer::is_admin`, which also accounts for preview mode.
    pub role_admin: bool,
    /// This admin has switched their own admin powers off to see what a member
    /// sees. Session-scoped, so one browser can preview while another does not.
    pub preview: bool,
    /// This lookup moved `last_seen_at` — at most once a minute. The cookie's
    /// lifetime is renewed on exactly these requests (`web::session::renew`),
    /// so it counts from the last visit, like the row, not from sign-in.
    pub slid: bool,
}

impl Session {
    pub fn viewer(&self) -> crate::domain::viewer::Viewer {
        crate::domain::viewer::Viewer {
            member_id: self.member.id,
            role_admin: self.role_admin,
            preview: self.preview,
        }
    }
}

/// 32 random bytes, base64url. Opaque: it carries no member id, so a stolen
/// cookie reveals nothing and revoking it is deleting one row rather than
/// rotating a secret that would sign out the whole household.
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A device label derived from the User-Agent, so `:profile` can name the thing
/// it is offering to sign out rather than showing an opaque id.
pub fn device_label(user_agent: Option<&str>) -> String {
    let ua = user_agent.unwrap_or("");
    let os = [
        ("iPhone", "iPhone"),
        ("iPad", "iPad"),
        ("Android", "Android"),
        ("Macintosh", "Mac"),
        ("Windows", "Windows"),
        ("Linux", "Linux"),
    ]
    .iter()
    .find(|(needle, _)| ua.contains(needle))
    .map(|(_, name)| *name);

    let browser = [
        ("Edg/", "Edge"),
        ("OPR/", "Opera"),
        ("Firefox/", "Firefox"),
        ("Chrome/", "Chrome"),
        ("Safari/", "Safari"),
    ]
    .iter()
    .find(|(needle, _)| ua.contains(needle))
    .map(|(_, name)| *name);

    match (os, browser) {
        (Some(o), Some(b)) => format!("{b}{DEVICE_ON}{o}"),
        (Some(o), None) => o.to_string(),
        (None, Some(b)) => b.to_string(),
        (None, None) => UNKNOWN_DEVICE.into(),
    }
}

/// How a device label is stored: `Safari on iPhone`, or `unknown device`.
/// English in the table, since it is written once at sign-in and read by
/// whoever looks — `device_text` words it for them.
const DEVICE_ON: &str = " on ";
const UNKNOWN_DEVICE: &str = "unknown device";

/// A stored device label in the reader's language: *Safari no iPhone*. The
/// browser and system names are names, and stay as they are.
pub fn device_text(label: &str, t: &crate::ui::i18n::Catalog) -> String {
    if label == UNKNOWN_DEVICE {
        return t.device_unknown().to_string();
    }
    match label.split_once(DEVICE_ON) {
        Some((browser, os)) => t.device_on(browser, os),
        None => label.to_string(),
    }
}

pub async fn start_session(
    tx: &mut Transaction<'_, Sqlite>,
    member_id: i64,
    label: &str,
) -> sqlx::Result<String> {
    let id = random_token();
    let at = now();
    sqlx::query!(
        r#"INSERT INTO sessions (id, member_id, created_at, last_seen_at, label)
           VALUES (?1, ?2, ?3, ?3, ?4)"#,
        id,
        member_id,
        at,
        label,
    )
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

/// How stale `last_seen_at` may get before a request moves it.
const SEEN_EVERY: chrono::Duration = chrono::Duration::seconds(60);

/// Resolves the cookie, sliding the session's `last_seen_at` as it goes.
///
/// Long-lived and sliding rather than expiring: signing a household out of its
/// own ledger every fortnight buys nothing behind a LAN, and the revocation
/// path — one row — is the thing that actually matters. The cookie follows
/// the slide (`Session::slid`), so the device keeps holding it too.
pub async fn session(pool: &SqlitePool, id: &str) -> sqlx::Result<Option<Session>> {
    let row = sqlx::query!(
        r#"SELECT s.id AS "id!: String",
                  m.id AS "member_id!: i64",
                  m.name AS "name!: String",
                  m.uuid AS "uuid!: String",
                  m.slug AS "slug!: String",
                  m.role AS "role!: i64",
                  s.preview AS "preview!: i64",
                  s.last_seen_at AS "last_seen_at!: String"
           FROM sessions s
           JOIN members m ON m.id = s.member_id
           WHERE s.id = ?1 AND m.deleted_at IS NULL
             -- Turning a member off deletes their sessions too; this is the
             -- belt to that brace, so no session outlives the decision.
             AND NOT EXISTS (SELECT 1 FROM member_inactive i
                             WHERE i.member_id = m.id AND i.until_period IS NULL)"#,
        id
    )
    .fetch_optional(pool)
    .await?;

    let Some(r) = row else { return Ok(None) };

    // Slid once a minute, not on every request. A page asks for a dozen things
    // at once — the carousel's pages, the live feed's rows — and each one took
    // SQLite's single writer to move a timestamp by a second. Only `:members`
    // reads it, to order a member's devices.
    let fresh = chrono::DateTime::parse_from_rfc3339(&r.last_seen_at).is_ok_and(|seen| {
        let age = crate::clock::now() - seen.to_utc();
        age >= chrono::Duration::zero() && age < SEEN_EVERY
    });
    if !fresh {
        let at = now();
        sqlx::query!(
            "UPDATE sessions SET last_seen_at = ?2 WHERE id = ?1",
            id,
            at
        )
        .execute(pool)
        .await?;
    }

    Ok(Some(Session {
        id: r.id,
        member: Member {
            id: r.member_id,
            name: r.name,
            uuid: r.uuid,
            slug: r.slug,
            is_admin: r.role == 0,
        },
        role_admin: r.role == 0,
        preview: r.preview != 0,
        slid: !fresh,
    }))
}

/// Toggles preview-as-non-admin for this session. Reversible any time and
/// nothing about it is destructive, so it needs no confirmation.
pub async fn set_preview(pool: &SqlitePool, session_id: &str, on: bool) -> sqlx::Result<()> {
    let on = i64::from(on);
    sqlx::query!(
        "UPDATE sessions SET preview = ?2 WHERE id = ?1",
        session_id,
        on
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn end_session(pool: &SqlitePool, id: &str) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM sessions WHERE id = ?1", id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Every device, for one member. This is the reason the table exists.
pub async fn revoke_all_sessions(
    tx: &mut Transaction<'_, Sqlite>,
    member_id: i64,
) -> sqlx::Result<u64> {
    let n = sqlx::query!("DELETE FROM sessions WHERE member_id = ?1", member_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(n)
}

// ---------------------------------------------------------------------------
// signing in
// ---------------------------------------------------------------------------

/// A name as it is stored: lowercase, trimmed, inner runs of spaces made one.
/// Everybody signs in with their name, and `Rodrigo` and `rodrigo` must not be
/// two accounts — nor the same person failing to sign in over a Shift key.
/// Shown through `ui::fmt::name`, which capitalizes it back for reading.
pub fn normal_name(typed: &str) -> String {
    typed
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The live member a typed name means, ignoring case — names stored before
/// they were lowercased (`Rodrigo`) still sign in as `rodrigo`. Compared in
/// Rust rather than with SQLite's `lower()`, which folds only ASCII and would
/// leave `Ângela` unmatched. A household is a handful of rows.
async fn member_named(pool: &SqlitePool, typed: &str) -> sqlx::Result<Option<i64>> {
    let want = normal_name(typed);
    let rows = sqlx::query!(
        r#"SELECT id AS "id!: i64", name AS "name!: String" FROM members
           WHERE deleted_at IS NULL"#
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .find(|r| normal_name(&r.name) == want)
        .map(|r| r.id))
}

/// The login identifier is `members.name`, never an email: there is no mail
/// sender in this app, and `email` plays no part in signing in.
///
/// Returns `None` for both a wrong name and a wrong password, deliberately —
/// not as security theater (this document already declines rate limiting) but
/// because both lead to the same next action, and the copy says what that is.
///
/// An **inactive** member with the right password still authenticates here;
/// the login handler asks [`crate::db::members::is_inactive`] and says so
/// instead of starting a session. Only somebody who knows the password learns
/// the account is off, and their next step really is different: ask an admin.
pub async fn authenticate(
    pool: &SqlitePool,
    name: &str,
    password: &str,
) -> sqlx::Result<Option<i64>> {
    let Some(id) = member_named(pool, name).await? else {
        return Ok(None);
    };
    let row = sqlx::query!(
        r#"SELECT id AS "id!: i64", password_hash AS "password_hash!: String"
           FROM members WHERE id = ?1"#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row
        .filter(|r| verify_password(password, &r.password_hash))
        .map(|r| r.id))
}

// ---------------------------------------------------------------------------
// first run
// ---------------------------------------------------------------------------

pub struct NewHousehold<'a> {
    pub household_name: &'a str,
    pub currency: &'a str,
    pub language: &'a str,
    pub admin_name: &'a str,
    pub password: &'a str,
    pub theme: &'a str,
    pub line_numbers: &'a str,
    pub device_label: &'a str,
}

/// `members` empty is the whole precondition for `/setup`, and it is checked
/// inside the same transaction that creates the first member — otherwise two
/// browsers racing the form both pass the check and both create an admin.
pub async fn is_fresh(pool: &SqlitePool) -> sqlx::Result<bool> {
    let n: i64 = sqlx::query_scalar!(r#"SELECT count(*) AS "n!: i64" FROM members"#)
        .fetch_one(pool)
        .await?;
    Ok(n == 0)
}

/// One POST, one transaction: `household_settings` + `members` +
/// `member_settings` + `sessions`, or nothing at all. A wizard would need
/// somewhere to park step one's answers, and there is nowhere — no household
/// row, no member, therefore no session.
pub async fn setup(pool: &SqlitePool, new: NewHousehold<'_>) -> anyhow::Result<String> {
    let hash = hash_password(new.password)?;
    let at = now();
    let uuid = uuid_v4();
    let admin_name = normal_name(new.admin_name);
    let slug = slugify(&admin_name);
    let mut tx = pool.begin().await?;

    let existing: i64 = sqlx::query_scalar!(r#"SELECT count(*) AS "n!: i64" FROM members"#)
        .fetch_one(&mut *tx)
        .await?;
    if existing > 0 {
        anyhow::bail!("this household is already set up");
    }

    sqlx::query!(
        // The household's palette for new members starts as its founder's —
        // the household begins looking like its first admin. The layout
        // defaults keep the schema's values.
        r#"INSERT INTO household_settings
               (id, name, locale_currency, default_language, default_visibility,
                ledger_show_recurring, created_at, updated_at, default_theme)
           VALUES (1, ?1, ?2, ?3, 'household', 1, ?4, ?4, ?5)"#,
        new.household_name,
        new.currency,
        new.language,
        at,
        new.theme,
    )
    .execute(&mut *tx)
    .await?;

    // role 0: the first member is the admin, by construction rather than by a
    // later grant somebody could forget.
    let member_id = sqlx::query_scalar!(
        r#"INSERT INTO members (uuid, name, slug, password_hash, role, created_at)
           VALUES (?1, ?2, ?3, ?4, 0, ?5)
           RETURNING id AS "id!: i64""#,
        uuid,
        admin_name,
        slug,
        hash,
        at,
    )
    .fetch_one(&mut *tx)
    .await?;

    insert_member_settings(
        &mut tx,
        member_id,
        new.language,
        new.theme,
        new.line_numbers,
    )
    .await?;
    let sid = start_session(&mut tx, member_id, new.device_label).await?;
    tx.commit().await?;
    Ok(sid)
}

/// A new member's own settings row, **seeded** from the household's defaults
/// for new members (`settings::MemberDefaults`) — read inside the same
/// transaction, so a default changed a moment ago is the one used. From here
/// on the row is the member's: the household never reads its defaults for
/// them again. The palette is the one they picked on the form, which the
/// form preselects from the same defaults.
async fn insert_member_settings(
    tx: &mut Transaction<'_, Sqlite>,
    member_id: i64,
    language: &str,
    theme: &str,
    line_numbers: &str,
) -> sqlx::Result<()> {
    let d = crate::db::settings::member_defaults(&mut **tx).await?;
    sqlx::query!(
        r#"INSERT INTO member_settings
               (member_id, language, theme_name, line_numbers, layout_panes,
                layout_min_split_width, layout_min_width, layout_max_width, layout_gap,
                density)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
        member_id,
        language,
        theme,
        line_numbers,
        d.panes,
        d.min_split,
        d.min_width,
        d.max_width,
        d.gap,
        d.density,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// invites and resets — one mechanism, two jobs
// ---------------------------------------------------------------------------

/// What a link turns out to be. `member_id IS NULL` means it creates a member;
/// set means it sets the password on one that already exists.
///
/// A reset is **not** an invite pointed at somebody who exists: the invite path
/// inserts, and `members.name` is UNIQUE, so recovering through one would force
/// a different name — which rewrites `slug`, which is that member's receipt
/// vault folder. Recovering a password must never orphan somebody's filing.
/// One member, by the name they sign in with. `members.name` is UNIQUE, so
/// this is exact rather than a search — the console resets an account, and
/// resolving "rod" to the closest match would be the wrong kind of helpful.
pub async fn member_by_name(pool: &SqlitePool, name: &str) -> sqlx::Result<Option<Member>> {
    // Exact apart from case: `rodrigo` is the member stored as `Rodrigo`.
    let Some(id) = member_named(pool, name).await? else {
        return Ok(None);
    };
    let row = sqlx::query!(
        r#"SELECT id AS "id!: i64", name, uuid, slug, role FROM members
           WHERE id = ?1"#,
        id,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| Member {
        id: r.id,
        name: r.name,
        uuid: r.uuid,
        slug: r.slug,
        is_admin: r.role == 0,
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// Create a member. Carries who issued it, for the page's copy.
    Invite {
        id: i64,
        inviter: String,
        expires: String,
        role: i64,
    },
    /// Set the password on this member.
    Reset {
        id: i64,
        member_id: i64,
        expires: String,
    },
    Used,
    Expired {
        on: String,
    },
    Unknown,
}

fn token_hash(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    format!("{:x}", h.finalize())
}

/// Issues a link. `member_id = None` invites somebody new; `Some(id)` resets
/// that member's password — which also **revokes every session they have** and
/// writes it to the change log, because it hands the issuer the ability to
/// sign in as them. That is the one thing preview-as-non-admin was built not
/// to allow, so it is made accountable rather than quietly permitted.
///
/// `actor` is who is doing it, not an admin id, because a reset has a second
/// issuer: `ffm reset-link` on the console, for the sole admin who is locked
/// out of the one account nobody can reset for them. That run has no member
/// behind it, so it stamps [`Actor::System`] and leaves `created_by` NULL.
/// Naming an admin would be a lie in the one record built to catch exactly
/// this action. An **invite** still requires a real member: deciding who joins
/// the household is the admin's call, not the operator's.
pub async fn issue_link(
    pool: &SqlitePool,
    actor: Actor,
    member_id: Option<i64>,
    label: Option<&str>,
    role: i64,
) -> anyhow::Result<String> {
    debug_assert!(
        actor.member_id().is_some() || member_id.is_some(),
        "an invite needs a real member behind it; only a reset may come from the console"
    );
    let token = random_token();
    let hash = token_hash(&token);
    let created = crate::clock::now();
    let created_s = created.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let expires_s = (created + Duration::days(LINK_DAYS))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let issuer = actor.member_id();

    let mut tx = pool.begin().await?;

    // At most one live reset per member is a partial unique index, so a second
    // click has to retire the first rather than leaving two working links.
    if let Some(mid) = member_id {
        sqlx::query!(
            r#"UPDATE member_invites SET revoked_at = ?2, token = NULL
               WHERE member_id = ?1 AND accepted_at IS NULL AND revoked_at IS NULL"#,
            mid,
            created_s,
        )
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query!(
        // The raw token too, while the link is pending, so `:members` can show
        // and copy it again (migration 0009). `token_hash` is still what a
        // link is looked up by; the token is wiped on accept, revoke or
        // expiry.
        r#"INSERT INTO member_invites
               (token_hash, member_id, label, role, created_by, created_at, expires_at, token)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
        hash,
        member_id,
        label,
        role,
        issuer,
        created_s,
        expires_s,
        token,
    )
    .execute(&mut *tx)
    .await?;

    if let Some(mid) = member_id {
        revoke_all_sessions(&mut tx, mid).await?;
        // A console run says so on both entries. The log already carries
        // `source = 'system'` with no member id, but that alone reads as the
        // app acting on its own — the note is what tells a household reading
        // this back that a person with the database file in their hands did
        // it, which is a different fact worth being able to see.
        let note = issuer.is_none().then_some(CONSOLE_RESET);
        for action in [MemberAction::PasswordReset, MemberAction::SessionsRevoked] {
            let entry = Change::member(actor, action, mid);
            match note {
                Some(n) => entry.note(n).write(&mut tx).await?,
                None => entry.write(&mut tx).await?,
            };
        }
    }

    tx.commit().await?;
    Ok(token)
}

/// Resolves a token to what the page should render. A used, expired or unknown
/// token is a **page**, never an error — with no form at all, because a
/// disabled form invites retrying.
pub async fn read_link(pool: &SqlitePool, token: &str) -> sqlx::Result<Link> {
    let hash = token_hash(token);
    let row = sqlx::query!(
        r#"SELECT i.id AS "id!: i64",
                  i.member_id AS "member_id: i64",
                  i.role AS "role!: i64",
                  i.expires_at AS "expires_at!: String",
                  i.accepted_at AS "accepted_at: String",
                  i.revoked_at AS "revoked_at: String",
                  a.name AS "inviter?: String"
           FROM member_invites i
           -- LEFT, because `ffm reset-link` on the console has no member to
           -- name in created_by. An inner join here silently made a
           -- console-issued link read as Unknown, which is the worst possible
           -- answer: a valid token, on a page telling you to ask an admin —
           -- when being unable to reach one is the whole reason you ran it.
           LEFT JOIN members a ON a.id = i.created_by
           WHERE i.token_hash = ?1"#,
        hash
    )
    .fetch_optional(pool)
    .await?;

    let Some(r) = row else {
        return Ok(Link::Unknown);
    };
    if r.accepted_at.is_some() {
        return Ok(Link::Used);
    }
    // A revoked link says "already used" rather than naming revocation: the
    // person holding it has exactly one thing to do either way, and "somebody
    // took this away from you" is not it.
    if r.revoked_at.is_some() {
        return Ok(Link::Used);
    }
    if expired(&r.expires_at) {
        return Ok(Link::Expired { on: r.expires_at });
    }

    Ok(match r.member_id {
        None => Link::Invite {
            id: r.id,
            // An invite always has a real issuer: deciding who joins the
            // household is the admin's call, not the operator's, and the
            // schema CHECKs it. A NULL can only be a console reset, which is
            // the arm below.
            inviter: r
                .inviter
                .as_deref()
                .map(crate::ui::fmt::name)
                .unwrap_or_default(),
            expires: r.expires_at,
            role: r.role,
        },
        Some(member_id) => Link::Reset {
            id: r.id,
            member_id,
            expires: r.expires_at,
        },
    })
}

fn expired(at: &str) -> bool {
    DateTime::parse_from_rfc3339(at).is_ok_and(|d| d.with_timezone(&Utc) < crate::clock::now())
}

/// Why an accept did not go through. Each one is copy the page already has.
#[derive(Debug, PartialEq, Eq)]
pub enum AcceptError {
    NameTaken,
    Gone,
    Db(String),
}

impl std::fmt::Display for AcceptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcceptError::NameTaken => write!(f, "name taken"),
            AcceptError::Gone => write!(f, "link no longer valid"),
            AcceptError::Db(e) => write!(f, "{e}"),
        }
    }
}

/// Accepting an **invite**: creates the member, their settings and a session,
/// all in one transaction, and marks the link used inside it. The link is
/// re-checked in that transaction, so two browsers racing it cannot both
/// create a member.
#[allow(clippy::too_many_arguments)]
pub async fn accept_invite(
    pool: &SqlitePool,
    token: &str,
    name: &str,
    password: &str,
    language: &str,
    theme: &str,
    role: i64,
    device: &str,
) -> Result<String, AcceptError> {
    let hash = hash_password(password).map_err(|e| AcceptError::Db(e.to_string()))?;
    let token_hash = token_hash(token);
    let at = now();
    let uuid = uuid_v4();
    let name = normal_name(name);
    let slug = slugify(&name);

    let mut tx = pool.begin().await.map_err(db)?;

    let invite = sqlx::query!(
        r#"SELECT id AS "id!: i64", expires_at AS "expires_at!: String"
           FROM member_invites
           WHERE token_hash = ?1 AND member_id IS NULL
             AND accepted_at IS NULL AND revoked_at IS NULL"#,
        token_hash
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;

    let invite = invite.ok_or(AcceptError::Gone)?;
    if expired(&invite.expires_at) {
        return Err(AcceptError::Gone);
    }

    // Taken ignoring case — and including the trash, whose names stay taken
    // so a restore is safe. `Rodrigo`, stored before names were lowercased,
    // takes `rodrigo` too.
    let names: Vec<String> = sqlx::query_scalar!(r#"SELECT name AS "n!: String" FROM members"#)
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
    if names.iter().any(|n| normal_name(n) == name) {
        return Err(AcceptError::NameTaken);
    }

    let member_id = sqlx::query_scalar!(
        r#"INSERT INTO members (uuid, name, slug, password_hash, role, created_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6)
           RETURNING id AS "id!: i64""#,
        uuid,
        name,
        slug,
        hash,
        role,
        at,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;

    insert_member_settings(&mut tx, member_id, language, theme, "relative")
        .await
        .map_err(db)?;

    sqlx::query!(
        r#"UPDATE member_invites SET accepted_at = ?2, accepted_member_id = ?3, token = NULL
           WHERE id = ?1"#,
        invite.id,
        at,
        member_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(db)?;

    let sid = start_session(&mut tx, member_id, device)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(sid)
}

/// Accepting a **reset**: sets `password_hash` on the member the link names.
/// No name, no language, no theme — they are not new, and a reset has no
/// business resetting taste.
pub async fn accept_reset(
    pool: &SqlitePool,
    token: &str,
    password: &str,
    device: &str,
) -> Result<String, AcceptError> {
    let hash = hash_password(password).map_err(|e| AcceptError::Db(e.to_string()))?;
    let token_hash = token_hash(token);
    let at = now();

    let mut tx = pool.begin().await.map_err(db)?;

    let link = sqlx::query!(
        // The member must still be one who can sign in: turning somebody off
        // or trashing them revokes their open reset, and this is the re-check
        // in case a link raced it.
        r#"SELECT i.id AS "id!: i64", i.member_id AS "member_id!: i64",
                  i.expires_at AS "expires_at!: String"
           FROM member_invites i
           JOIN members m ON m.id = i.member_id
           WHERE i.token_hash = ?1 AND i.member_id IS NOT NULL
             AND i.accepted_at IS NULL AND i.revoked_at IS NULL
             AND m.deleted_at IS NULL
             AND NOT EXISTS (SELECT 1 FROM member_inactive x
                             WHERE x.member_id = m.id AND x.until_period IS NULL)"#,
        token_hash
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;

    let link = link.ok_or(AcceptError::Gone)?;
    if expired(&link.expires_at) {
        return Err(AcceptError::Gone);
    }

    sqlx::query!(
        "UPDATE members SET password_hash = ?2 WHERE id = ?1",
        link.member_id,
        hash
    )
    .execute(&mut *tx)
    .await
    .map_err(db)?;

    sqlx::query!(
        r#"UPDATE member_invites SET accepted_at = ?2, accepted_member_id = ?3, token = NULL
           WHERE id = ?1"#,
        link.id,
        at,
        link.member_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(db)?;

    let sid = start_session(&mut tx, link.member_id, device)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(sid)
}

fn db(e: sqlx::Error) -> AcceptError {
    AcceptError::Db(e.to_string())
}

/// A v4 UUID, without pulling in a crate for sixteen random bytes and a format
/// string. It is a vault folder name, not a distributed identifier.
fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_is_named_in_the_readers_language() {
        let pt = crate::ui::i18n::Catalog::new(crate::ui::i18n::Lang::PtBr);
        assert_eq!(device_text("Safari on iPhone", &pt), "Safari no iPhone");
        assert_eq!(
            device_text("unknown device", &pt),
            "dispositivo desconhecido"
        );
        assert_eq!(device_text("Linux", &pt), "Linux", "a lone name is a name");
    }

    #[test]
    fn a_password_round_trips_and_a_wrong_one_does_not() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("Correct horse battery", &h));
    }

    #[test]
    fn the_seed_placeholder_can_never_authenticate() {
        // seeds/household.sql stores a deliberately invalid PHC string. This
        // asserts that is actually unusable rather than merely unlikely.
        assert!(!verify_password("", "!seed-cannot-sign-in"));
        assert!(!verify_password(
            "!seed-cannot-sign-in",
            "!seed-cannot-sign-in"
        ));
    }

    #[test]
    fn a_token_is_never_stored_only_its_hash() {
        let t = random_token();
        let h = token_hash(&t);
        assert_ne!(t, h);
        assert_eq!(h.len(), 64, "sha-256, hex");
        assert_eq!(h, token_hash(&t), "and it is deterministic");
    }

    #[test]
    fn tokens_do_not_repeat() {
        let a = random_token();
        let b = random_token();
        assert_ne!(a, b);
        assert!(a.len() >= 43, "32 bytes, base64url, unpadded");
    }

    #[test]
    fn a_uuid_looks_like_one() {
        let u = uuid_v4();
        assert_eq!(u.len(), 36);
        assert_eq!(u.as_bytes()[14], b'4', "version nibble");
        assert!(matches!(u.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
    }

    #[test]
    fn a_device_label_names_something_a_person_would_recognize() {
        let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
                  AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
        assert_eq!(device_label(Some(ua)), "Safari on iPhone");
        assert_eq!(device_label(None), "unknown device");
    }
}
