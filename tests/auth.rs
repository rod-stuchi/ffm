//! Auth, sessions, invites, resets — and the privacy rule they exist to serve.
//!
//! The privacy tests at the bottom are the reason this phase came before any
//! mutation work: a hidden row has to be absent from the row list *and* from
//! the totals, and the cheapest way to get that wrong is to filter in one place
//! and sum in another.

mod common;

use common::{pool, seeded};
use ffm::db::auth::{self, AcceptError, Link};
use ffm::db::changes::Actor;
use ffm::db::ledger;
use ffm::domain::money::{Cents, Currency};
use ffm::domain::period::Period;
use ffm::domain::viewer::Viewer;
use ffm::ui::row::RowCtx;
use sqlx::SqlitePool;

fn ctx() -> RowCtx {
    RowCtx {
        today: chrono::NaiveDate::from_ymd_opt(2026, 9, 21).unwrap(),
        currency: Currency::Brl,
        show_recurring: true,
        show_delta: true,
        narrow: false,
    }
}

async fn first_household(pool: &SqlitePool) -> String {
    auth::setup(
        pool,
        auth::NewHousehold {
            household_name: "Casa Stuchi",
            currency: "BRL",
            language: "pt-BR",
            admin_name: "rodrigo",
            password: "abcd1234",
            theme: "rosepine",
            line_numbers: "relative",
            device_label: "Safari on Mac",
        },
    )
    .await
    .expect("setup")
}

// ---------------------------------------------------------------------------
// first run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn setup_creates_the_whole_household_in_one_go_and_only_once() {
    let pool = pool().await;
    assert!(auth::is_fresh(&pool).await.unwrap());

    let sid = first_household(&pool).await;
    assert!(!auth::is_fresh(&pool).await.unwrap());

    // One POST, one transaction: household + member + settings + session, or
    // nothing at all. Anything less leaves a half-configured app with no way
    // back in.
    let s = auth::session(&pool, &sid)
        .await
        .unwrap()
        .expect("signed in");
    assert_eq!(s.member.name, "rodrigo");
    assert!(
        s.role_admin,
        "the first member is the admin by construction"
    );
    assert_eq!(s.member.slug, "rodrigo", "the receipt vault's folder name");

    let h = ledger::household(&pool).await.unwrap().unwrap();
    assert_eq!(h.name, "Casa Stuchi");

    let settings = ffm::db::settings::member_settings(&pool, s.member.id)
        .await
        .unwrap();
    assert_eq!(settings.theme.key(), "rosepine");

    // Reachable by two browsers racing the form, so it is checked inside the
    // transaction rather than before it.
    let again = auth::setup(
        &pool,
        auth::NewHousehold {
            household_name: "Another",
            currency: "USD",
            language: "en",
            admin_name: "someone",
            password: "abcd1234",
            theme: "ffm",
            line_numbers: "relative",
            device_label: "x",
        },
    )
    .await;
    assert!(again.is_err(), "setup is first run ONLY");
}

// ---------------------------------------------------------------------------
// signing in
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_identifier_is_the_name_and_a_wrong_password_is_indistinguishable() {
    let pool = pool().await;
    first_household(&pool).await;

    assert!(
        auth::authenticate(&pool, "rodrigo", "abcd1234")
            .await
            .unwrap()
            .is_some()
    );
    // Both of these return None, on purpose: they lead to the same next action,
    // and the copy tells people what that is.
    assert!(
        auth::authenticate(&pool, "rodrigo", "nope")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        auth::authenticate(&pool, "nobody", "abcd1234")
            .await
            .unwrap()
            .is_none()
    );
    // Whitespace around a typed name is not a different person.
    assert!(
        auth::authenticate(&pool, "  rodrigo  ", "abcd1234")
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn the_seeded_household_cannot_be_signed_into() {
    // seeds/household.sql stores a deliberately invalid PHC string, so the
    // sample data is something to look at rather than a way in.
    let pool = seeded().await;
    for name in ["Rodrigo", "Natália", "Miguel"] {
        for pw in ["", "password", "!seed-cannot-sign-in"] {
            assert!(
                auth::authenticate(&pool, name, pw).await.unwrap().is_none(),
                "{name} / {pw}"
            );
        }
    }
}

#[tokio::test]
async fn one_device_can_be_signed_out_without_touching_the_others() {
    let pool = pool().await;
    let phone = first_household(&pool).await;
    let member_id = auth::session(&pool, &phone)
        .await
        .unwrap()
        .unwrap()
        .member
        .id;

    let mut tx = pool.begin().await.unwrap();
    let laptop = auth::start_session(&mut tx, member_id, "Firefox on Linux")
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // This is the entire reason sessions is a table rather than a signed
    // cookie: losing a phone revokes one row, not a secret that would sign out
    // the whole household.
    auth::end_session(&pool, &phone).await.unwrap();
    assert!(auth::session(&pool, &phone).await.unwrap().is_none());
    assert!(auth::session(&pool, &laptop).await.unwrap().is_some());
}

#[tokio::test]
async fn preview_is_session_scoped_not_account_scoped() {
    let pool = pool().await;
    let a = first_household(&pool).await;
    let member_id = auth::session(&pool, &a).await.unwrap().unwrap().member.id;
    let mut tx = pool.begin().await.unwrap();
    let b = auth::start_session(&mut tx, member_id, "other")
        .await
        .unwrap();
    tx.commit().await.unwrap();

    auth::set_preview(&pool, &a, true).await.unwrap();

    assert!(
        !auth::session(&pool, &a)
            .await
            .unwrap()
            .unwrap()
            .viewer()
            .is_admin()
    );
    assert!(
        auth::session(&pool, &b)
            .await
            .unwrap()
            .unwrap()
            .viewer()
            .is_admin(),
        "one browser previewing must not disarm the other"
    );
    // Still the same person throughout — this is a view, not a session swap.
    assert!(auth::session(&pool, &a).await.unwrap().unwrap().role_admin);
}

// ---------------------------------------------------------------------------
// invites
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_invite_creates_the_member_when_it_is_accepted_not_when_it_is_issued() {
    let pool = pool().await;
    let admin_sid = first_household(&pool).await;
    let admin = auth::session(&pool, &admin_sid)
        .await
        .unwrap()
        .unwrap()
        .member
        .id;

    let token = auth::issue_link(&pool, Actor::User(admin), None, Some("for Natália"), 1)
        .await
        .unwrap();

    // Nothing exists yet. This is why password_hash stays NOT NULL: a
    // pre-created, password-less member would make "a NULL hash cannot
    // authenticate" a rule enforced by one WHERE clause somebody forgets.
    assert_eq!(ledger::members(&pool).await.unwrap().len(), 1);

    match auth::read_link(&pool, &token).await.unwrap() {
        Link::Invite { inviter, .. } => assert_eq!(inviter, "rodrigo"),
        other => panic!("{other:?}"),
    }

    let sid = auth::accept_invite(
        &pool,
        &token,
        "natália",
        "hunter2hunter2",
        "pt-BR",
        "kanagawa",
        1,
        "iPhone",
    )
    .await
    .expect("accept");

    let s = auth::session(&pool, &sid).await.unwrap().unwrap();
    assert_eq!(s.member.name, "natália");
    assert!(!s.role_admin);
    // The invitee picked their own name, so they picked their own vault
    // folder — accents and all, per the slug rule: a household filing under
    // "Água" should see `água` when they open the folder themselves.
    assert_eq!(s.member.slug, "natália");
    assert!(
        auth::authenticate(&pool, "natália", "hunter2hunter2")
            .await
            .unwrap()
            .is_some()
    );

    // Single use — and a used link renders a page, never an error.
    assert_eq!(auth::read_link(&pool, &token).await.unwrap(), Link::Used);
    let twice = auth::accept_invite(
        &pool,
        &token,
        "outro",
        "hunter2hunter2",
        "en",
        "ffm",
        1,
        "x",
    )
    .await;
    assert_eq!(twice.unwrap_err(), AcceptError::Gone);
}

#[tokio::test]
async fn a_taken_name_is_refused_with_something_to_do_about_it() {
    let pool = pool().await;
    let admin_sid = first_household(&pool).await;
    let admin = auth::session(&pool, &admin_sid)
        .await
        .unwrap()
        .unwrap()
        .member
        .id;
    let token = auth::issue_link(&pool, Actor::User(admin), None, None, 1)
        .await
        .unwrap();

    let err = auth::accept_invite(
        &pool,
        &token,
        "rodrigo",
        "hunter2hunter2",
        "en",
        "ffm",
        1,
        "x",
    )
    .await
    .unwrap_err();
    assert_eq!(err, AcceptError::NameTaken);

    // And the link survives, so the person can just pick another name.
    assert!(matches!(
        auth::read_link(&pool, &token).await.unwrap(),
        Link::Invite { .. }
    ));
}

#[tokio::test]
async fn an_unknown_or_expired_token_is_a_page_not_an_error() {
    let pool = pool().await;
    let admin_sid = first_household(&pool).await;
    let admin = auth::session(&pool, &admin_sid)
        .await
        .unwrap()
        .unwrap()
        .member
        .id;

    assert_eq!(
        auth::read_link(&pool, "not-a-token").await.unwrap(),
        Link::Unknown
    );

    let token = auth::issue_link(&pool, Actor::User(admin), None, None, 1)
        .await
        .unwrap();
    sqlx::query("UPDATE member_invites SET expires_at = '2020-01-01T00:00:00Z'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        auth::read_link(&pool, &token).await.unwrap(),
        Link::Expired { .. }
    ));
    // And it cannot be accepted anyway, not merely hidden behind a page.
    let err = auth::accept_invite(&pool, &token, "x", "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap_err();
    assert_eq!(err, AcceptError::Gone);
}

// ---------------------------------------------------------------------------
// resets
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_reset_sets_a_password_without_renaming_anybody() {
    let pool = pool().await;
    let admin_sid = first_household(&pool).await;
    let admin = auth::session(&pool, &admin_sid)
        .await
        .unwrap()
        .unwrap()
        .member
        .id;
    let invite = auth::issue_link(&pool, Actor::User(admin), None, None, 1)
        .await
        .unwrap();
    let their_sid = auth::accept_invite(
        &pool,
        &invite,
        "natália",
        "hunter2hunter2",
        "pt-BR",
        "ffm",
        1,
        "iPhone",
    )
    .await
    .unwrap();
    let them = auth::session(&pool, &their_sid)
        .await
        .unwrap()
        .unwrap()
        .member
        .id;

    let token = auth::issue_link(&pool, Actor::User(admin), Some(them), None, 1)
        .await
        .unwrap();
    assert!(matches!(
        auth::read_link(&pool, &token).await.unwrap(),
        Link::Reset { .. }
    ));

    // Issuing it signs them out everywhere: a takeover is something they
    // NOTICE rather than something they could only infer.
    assert!(
        auth::session(&pool, &their_sid).await.unwrap().is_none(),
        "issuing a reset must revoke that member's sessions"
    );

    auth::accept_reset(&pool, &token, "brandnewpass", "Firefox")
        .await
        .unwrap();

    assert!(
        auth::authenticate(&pool, "natália", "brandnewpass")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        auth::authenticate(&pool, "natália", "hunter2hunter2")
            .await
            .unwrap()
            .is_none()
    );

    // The name — and therefore the receipt-vault folder — is untouched. This is
    // why a reset is not an invite pointed at somebody who already exists:
    // recovering through one would force a different name and orphan their
    // filing.
    let m = ledger::members(&pool).await.unwrap();
    let her = m.iter().find(|m| m.id == them).unwrap();
    assert_eq!(
        (her.name.as_str(), her.slug.as_str()),
        ("natália", "natália")
    );
}

#[tokio::test]
async fn a_reset_is_written_to_the_append_only_log() {
    let pool = pool().await;
    let admin_sid = first_household(&pool).await;
    let admin = auth::session(&pool, &admin_sid)
        .await
        .unwrap()
        .unwrap()
        .member
        .id;
    let invite = auth::issue_link(&pool, Actor::User(admin), None, None, 1)
        .await
        .unwrap();
    let sid = auth::accept_invite(&pool, &invite, "ana", "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    let them = auth::session(&pool, &sid).await.unwrap().unwrap().member.id;

    auth::issue_link(&pool, Actor::User(admin), Some(them), None, 1)
        .await
        .unwrap();

    // A reset hands the admin the ability to write to the ledger as somebody
    // else — the one thing preview-as-non-admin was built NOT to allow. So it
    // is recorded rather than quietly permitted, in the log that cannot be
    // tidied away afterwards.
    let rows: Vec<(String, String, i64, Option<i64>)> =
        sqlx::query_as("SELECT action, entity, entity_id, member_id FROM change ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();

    assert_eq!(
        rows,
        vec![
            ("password_reset".into(), "member".into(), them, Some(admin)),
            (
                "sessions_revoked".into(),
                "member".into(),
                them,
                Some(admin)
            ),
        ]
    );
}

#[tokio::test]
async fn a_second_reset_click_replaces_the_first_link() {
    let pool = pool().await;
    let admin_sid = first_household(&pool).await;
    let admin = auth::session(&pool, &admin_sid)
        .await
        .unwrap()
        .unwrap()
        .member
        .id;
    let invite = auth::issue_link(&pool, Actor::User(admin), None, None, 1)
        .await
        .unwrap();
    let sid = auth::accept_invite(&pool, &invite, "ana", "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    let them = auth::session(&pool, &sid).await.unwrap().unwrap().member.id;

    let first = auth::issue_link(&pool, Actor::User(admin), Some(them), None, 1)
        .await
        .unwrap();
    let second = auth::issue_link(&pool, Actor::User(admin), Some(them), None, 1)
        .await
        .unwrap();

    // At most one live reset per member — a partial unique index enforces it,
    // so two working links for one account is not a state that can exist.
    assert_eq!(auth::read_link(&pool, &first).await.unwrap(), Link::Used);
    assert!(matches!(
        auth::read_link(&pool, &second).await.unwrap(),
        Link::Reset { .. }
    ));
}

// ---------------------------------------------------------------------------
// privacy — the expensive rule, and the reason this phase is early
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_private_row_is_absent_from_the_list_and_from_the_totals() {
    let pool = seeded().await;
    let members = ledger::members(&pool).await.unwrap();
    let rodrigo = &members[0]; // role 0, and the owner of the private row
    let natalia = &members[1];
    let sep = Period::parse("2026-09").unwrap();

    let owner = Viewer::new(rodrigo.id, true);
    let other = Viewer::new(natalia.id, false);

    let mine = ledger::pane(&pool, rodrigo, sep, ctx(), owner)
        .await
        .unwrap();
    let theirs = ledger::pane(&pool, rodrigo, sep, ctx(), other)
        .await
        .unwrap();

    assert!(
        mine.rows
            .iter()
            .any(|r| r.title == "Presente · aniversário")
    );
    assert!(
        !theirs
            .rows
            .iter()
            .any(|r| r.title == "Presente · aniversário"),
        "a private row is absent, not greyed out"
    );

    // And the numbers add up WITHOUT it, which is the half that is easy to get
    // wrong: a row filtered out of the list but still counted in the balance
    // announces its own existence.
    assert_eq!(mine.outs - theirs.outs, Cents(24000));
    assert_eq!(
        theirs.outs,
        theirs
            .rows
            .iter()
            .filter_map(|r| r.amount_cents)
            .sum::<Cents>()
            - theirs
                .rows
                .iter()
                .filter(|r| r.kind.is_income())
                .filter_map(|r| r.amount_cents)
                .sum::<Cents>()
    );
}

#[tokio::test]
async fn an_admin_sees_every_private_row_until_they_preview_as_a_member() {
    let pool = seeded().await;
    let members = ledger::members(&pool).await.unwrap();
    let rodrigo = &members[0];
    let natalia = &members[1];
    let sep = Period::parse("2026-09").unwrap();

    // Give Natália a private row of her own, so "the admin sees somebody
    // else's" is actually what is being tested.
    sqlx::query(
        "INSERT INTO transactions
             (member_id, period, kind, title, amount_cents, date, manual_paid,
              visibility, sort_key, created_at)
         VALUES (?1, '2026-09', 'out', 'Presente · Rodrigo', 19900, '2026-09-14', 0,
                 'private', 'z0', '2026-09-14T10:00:00Z')",
    )
    .bind(natalia.id)
    .execute(&pool)
    .await
    .unwrap();

    let admin = Viewer::new(rodrigo.id, true);
    let previewing = Viewer {
        preview: true,
        ..admin
    };

    let as_admin = ledger::pane(&pool, natalia, sep, ctx(), admin)
        .await
        .unwrap();
    let as_member = ledger::pane(&pool, natalia, sep, ctx(), previewing)
        .await
        .unwrap();

    assert!(
        as_admin
            .rows
            .iter()
            .any(|r| r.title == "Presente · Rodrigo")
    );
    assert!(
        !as_member
            .rows
            .iter()
            .any(|r| r.title == "Presente · Rodrigo"),
        "preview really hides it — that is the whole point of the mode"
    );
    assert_eq!(as_admin.outs - as_member.outs, Cents(19900));

    // But previewing never hides the admin's OWN private row: ownership grants
    // visibility independent of role, which is what makes this a view rather
    // than an impersonation.
    let own = ledger::pane(&pool, rodrigo, sep, ctx(), previewing)
        .await
        .unwrap();
    assert!(own.rows.iter().any(|r| r.title == "Presente · aniversário"));
}

// ---------------------------------------------------------------------------
// the console: the sole admin, locked out of the one account nobody can reset
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_console_can_reset_the_only_admin_and_the_log_says_who_did_it() {
    let pool = pool().await;
    let admin_sid = first_household(&pool).await;
    let admin = auth::session(&pool, &admin_sid)
        .await
        .unwrap()
        .unwrap()
        .member
        .id;

    // The name is exact, not a search: the console resets an account, and
    // resolving "rod" to the nearest match would be the wrong kind of helpful.
    assert!(auth::member_by_name(&pool, "rod").await.unwrap().is_none());
    let me = auth::member_by_name(&pool, "rodrigo")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(me.id, admin);

    let token = auth::issue_link(&pool, Actor::System, Some(me.id), None, 0)
        .await
        .unwrap();
    assert!(matches!(
        auth::read_link(&pool, &token).await.unwrap(),
        Link::Reset { .. }
    ));

    // It costs exactly what the web path costs. A console reset that skipped
    // either of these would be the same power, unrecorded.
    assert!(
        auth::session(&pool, &admin_sid).await.unwrap().is_none(),
        "a console reset must revoke sessions like any other"
    );

    // `source = 'system'` with no member id is the truth: nobody was signed
    // in. Naming an admin would be a lie in the one record built to catch
    // exactly this action — and the note is what separates "a person with the
    // database file in their hands" from the app acting on its own.
    let entries: Vec<(String, String, Option<i64>, Option<String>)> = sqlx::query_as(
        "SELECT action, source, member_id, note
         FROM change WHERE entity = 'member' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let note = Some("reset link issued from the console".to_string());
    assert_eq!(
        entries,
        vec![
            ("password_reset".into(), "system".into(), None, note.clone()),
            ("sessions_revoked".into(), "system".into(), None, note),
        ]
    );

    // And the way back in is the same page every other reset lands on.
    auth::accept_reset(&pool, &token, "newpass12345", "Firefox")
        .await
        .unwrap();
    assert!(
        auth::authenticate(&pool, "rodrigo", "newpass12345")
            .await
            .unwrap()
            .is_some()
    );

    // The account they got back is the one they left: same name, so the same
    // slug, so the same receipt-vault folder — and still an admin.
    let after = auth::member_by_name(&pool, "rodrigo")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (after.id, after.slug.as_str(), after.is_admin),
        (admin, "rodrigo", true)
    );
}
