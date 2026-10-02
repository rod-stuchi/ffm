//! The settings tiers: what a write accepts, what it refuses, and what an
//! admin can do to somebody else's account.

mod common;

use common::pool;
use ffm::db::auth;
use ffm::db::ledger;
use ffm::db::members;
use ffm::db::settings::{self, HouseholdKey, MemberKey};
use ffm::domain::money::Currency;
use ffm::domain::viewer::Viewer;
use sqlx::SqlitePool;

async fn household(pool: &SqlitePool) -> i64 {
    let sid = auth::setup(
        pool,
        auth::NewHousehold {
            household_name: "Casa Stuchi",
            currency: "BRL",
            language: "en",
            admin_name: "rodrigo",
            password: "abcd1234",
            theme: "ffm",
            line_numbers: "relative",
            device_label: "Safari on Mac",
        },
    )
    .await
    .unwrap();
    auth::session(pool, &sid).await.unwrap().unwrap().member.id
}

async fn settings_of(pool: &SqlitePool, me: i64) -> settings::MemberSettings {
    settings::member_settings(pool, me).await.unwrap()
}

#[tokio::test]
async fn a_refused_setting_leaves_the_previous_value_alone() {
    let pool = pool().await;
    let me = household(&pool).await;

    settings::set_member(&pool, me, MemberKey::LayoutGap, "24")
        .await
        .unwrap();
    assert_eq!(settings_of(&pool, me).await.layout_gap, 24);

    // The field keeps its previous value on a refusal, which falls out of not
    // writing it — there is no half-applied state to clean up.
    let e = settings::set_member(&pool, me, MemberKey::LayoutGap, "900")
        .await
        .unwrap_err();
    assert!(e.to_string().starts_with("✗ layout.gap"), "{e}");
    assert_eq!(settings_of(&pool, me).await.layout_gap, 24);

    let e = settings::set_member(&pool, me, MemberKey::LedgerSort, "sideways")
        .await
        .unwrap_err();
    assert!(
        e.to_string().contains("manual"),
        "it says what WOULD work: {e}"
    );
    assert_eq!(settings_of(&pool, me).await.ledger_sort, "manual");
}

#[tokio::test]
async fn the_privacy_default_has_three_states_and_the_third_is_null() {
    let pool = pool().await;
    let me = household(&pool).await;

    // NULL means "follow the household", which is a choice rather than the
    // absence of one — and `:profile` labels it with what it resolves to.
    assert_eq!(settings_of(&pool, me).await.default_visibility, None);

    settings::set_member(&pool, me, MemberKey::DefaultVisibility, "private")
        .await
        .unwrap();
    assert_eq!(
        settings_of(&pool, me).await.default_visibility.as_deref(),
        Some("private")
    );

    settings::set_member(&pool, me, MemberKey::DefaultVisibility, "household_default")
        .await
        .unwrap();
    assert_eq!(settings_of(&pool, me).await.default_visibility, None);
}

#[tokio::test]
async fn a_command_and_a_form_control_write_the_same_row() {
    // `:theme nord`, the swatch in :profile and a clicked statusline cell are
    // three gestures writing ONE row. That is what the single-source-of-truth
    // rule asks for; what it forbids is a second, pending copy.
    let pool = pool().await;
    let me = household(&pool).await;

    settings::set_member(&pool, me, MemberKey::ThemeName, "nord")
        .await
        .unwrap();
    assert_eq!(settings_of(&pool, me).await.theme.key(), "nord");

    // And an unknown palette is refused rather than silently ignored.
    assert!(
        settings::set_member(&pool, me, MemberKey::ThemeName, "neon")
            .await
            .is_err()
    );
    assert_eq!(settings_of(&pool, me).await.theme.key(), "nord");
}

#[tokio::test]
async fn currency_is_household_wide_and_picks_a_whole_format() {
    let pool = pool().await;
    household(&pool).await;

    settings::set_household(&pool, HouseholdKey::Currency, "usd")
        .await
        .unwrap();
    let h = ledger::household(&pool).await.unwrap().unwrap();
    assert_eq!(h.currency, Currency::Usd);

    // Picking USD switches to dot-decimal and comma-thousands, not just the
    // glyph — there is no "keep comma-decimal but show $" combination.
    assert_eq!(
        ffm::ui::fmt::money(ffm::domain::money::Cents(123_456), h.currency),
        "$1,234.56"
    );

    assert!(
        settings::set_household(&pool, HouseholdKey::Currency, "btc")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn the_household_name_cannot_be_emptied() {
    let pool = pool().await;
    household(&pool).await;
    // /login's heading, the statusline's left cell and the invite page all
    // need a word for the household.
    assert!(
        settings::set_household(&pool, HouseholdKey::Name, "   ")
            .await
            .is_err()
    );
    assert_eq!(
        ledger::household(&pool).await.unwrap().unwrap().name,
        "Casa Stuchi"
    );
}

#[tokio::test]
async fn admin_only_is_a_question_about_the_session_not_the_value() {
    let pool = pool().await;
    let me = household(&pool).await;
    let token = auth::issue_link(&pool, ffm::db::changes::Actor::User(me), None, None, 1)
        .await
        .unwrap();
    let sid = auth::accept_invite(&pool, &token, "ana", "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    let them = auth::session(&pool, &sid).await.unwrap().unwrap();

    assert!(Viewer::new(me, true).is_admin());
    assert!(!them.viewer().is_admin());

    // And an admin previewing refuses exactly as a real non-admin would —
    // the Household button disappears rather than sitting there disabled.
    let previewing = Viewer {
        preview: true,
        ..Viewer::new(me, true)
    };
    assert!(!previewing.is_admin());
}

#[tokio::test]
async fn members_lists_who_is_signed_in_where() {
    let pool = pool().await;
    let me = household(&pool).await;

    let list = members::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].is_admin);
    assert_eq!(list[0].sessions, 1);
    assert_eq!(
        list[0].devices,
        ["Safari on Mac"],
        "named, not an opaque id"
    );

    // Signing out everywhere is its own action, separate from a reset: an
    // admin may want only this — a lost phone.
    let n = members::sign_out_everywhere(&pool, me, me).await.unwrap();
    assert_eq!(n, 1);
    assert_eq!(members::list(&pool).await.unwrap()[0].sessions, 0);

    let logged: Vec<(String, i64)> =
        sqlx::query_as("SELECT action, entity_id FROM change ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(logged, vec![("sessions_revoked".into(), me)]);
}

#[tokio::test]
async fn a_pending_link_can_be_revoked_and_then_does_not_work() {
    let pool = pool().await;
    let me = household(&pool).await;
    let token = auth::issue_link(
        &pool,
        ffm::db::changes::Actor::User(me),
        None,
        Some("for Natália"),
        1,
    )
    .await
    .unwrap();

    let pending = members::pending_invites(&pool).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].label.as_deref(), Some("for Natália"));
    // Pending links carry their token, so the pane can show them again.
    assert_eq!(pending[0].token.as_deref(), Some(token.as_str()));

    assert!(members::revoke_invite(&pool, pending[0].id).await.unwrap());
    assert!(members::pending_invites(&pool).await.unwrap().is_empty());

    // A revoked link says "already used" rather than naming revocation: the
    // person holding it has exactly one thing to do either way.
    assert_eq!(
        auth::read_link(&pool, &token).await.unwrap(),
        auth::Link::Used
    );
    // And revoking twice is not an error, it is just nothing to do.
    assert!(!members::revoke_invite(&pool, pending[0].id).await.unwrap());
}

#[tokio::test]
async fn a_reset_appears_in_the_pending_list_named_for_its_member() {
    let pool = pool().await;
    let me = household(&pool).await;
    let invite = auth::issue_link(&pool, ffm::db::changes::Actor::User(me), None, None, 1)
        .await
        .unwrap();
    let sid = auth::accept_invite(&pool, &invite, "ana", "hunter2hunter2", "en", "ffm", 1, "x")
        .await
        .unwrap();
    let them = auth::session(&pool, &sid).await.unwrap().unwrap().member.id;

    auth::issue_link(
        &pool,
        ffm::db::changes::Actor::User(me),
        Some(them),
        None,
        1,
    )
    .await
    .unwrap();

    let pending = members::pending_invites(&pool).await.unwrap();
    assert_eq!(pending.len(), 1);
    // An admin has to be able to tell a reset from an invite at a glance: one
    // creates a member, the other hands somebody their own account back.
    assert_eq!(pending[0].for_member.as_deref(), Some("ana"));
}

#[tokio::test]
async fn a_new_member_starts_with_the_households_defaults_and_nobody_else_moves() {
    let pool = pool().await;
    let me = household(&pool).await;
    let before = format!("{:?}", settings_of(&pool, me).await);

    for (k, v) in [
        ("default_layout_panes", "tabs"),
        ("default_layout_min_split_width", "700"),
        ("default_layout_min_width", "0"),
        ("default_layout_max_width", "1200"),
        ("default_layout_gap", "24"),
        ("default_theme", "gruvbox"),
        ("default_density", "compact"),
    ] {
        settings::set_household(&pool, HouseholdKey::parse(k).unwrap(), v)
            .await
            .unwrap();
    }

    // The founder's own row is untouched: the defaults seed, they never
    // override.
    assert_eq!(format!("{:?}", settings_of(&pool, me).await), before);

    let token = auth::issue_link(&pool, ffm::db::changes::Actor::User(me), None, None, 1)
        .await
        .unwrap();
    // The theme a newcomer picks on the form is theirs; the invite page only
    // preselects the household's.
    let sid = auth::accept_invite(&pool, &token, "ana", "hunter2hunter2", "en", "nord", 1, "x")
        .await
        .unwrap();
    let ana = auth::session(&pool, &sid).await.unwrap().unwrap().member.id;
    let s = settings_of(&pool, ana).await;
    assert_eq!(s.theme.key(), "nord");
    assert_eq!(
        (
            s.layout_panes.as_str(),
            s.layout_min_split_width,
            s.layout_min_width,
            s.layout_max_width,
            s.layout_gap
        ),
        ("tabs", 700, 0, 1200, 24)
    );
    assert_eq!(s.density, "compact", "density is seeded with the layout");

    // And adopting the defaults later copies all of them, the palette and the
    // density included.
    settings::adopt_member_defaults(&pool, me).await.unwrap();
    let mine = settings_of(&pool, me).await;
    assert_eq!(mine.theme.key(), "gruvbox");
    assert_eq!(mine.density, "compact");
    assert!(
        settings::member_defaults(&pool)
            .await
            .unwrap()
            .matches(&mine)
    );
}

#[tokio::test]
async fn a_default_is_held_to_the_same_bounds_as_the_setting_it_seeds() {
    let pool = pool().await;
    let me = household(&pool).await;

    let household_says = settings::set_household(&pool, HouseholdKey::DefaultGap, "99")
        .await
        .unwrap_err()
        .to_string();
    let profile_says = settings::set_member(&pool, me, MemberKey::LayoutGap, "99")
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(household_says, profile_says, "one bound, whoever writes it");
    assert_eq!(settings::member_defaults(&pool).await.unwrap().gap, 16);

    assert!(
        settings::set_household(&pool, HouseholdKey::DefaultTheme, "nope")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn the_household_starts_in_its_founders_palette() {
    let pool = pool().await;
    // `household()` sets up with theme "ffm"; a founder picking another one
    // starts the household's palette for newcomers there.
    let sid = auth::setup(
        &pool,
        auth::NewHousehold {
            household_name: "Casa",
            currency: "BRL",
            language: "en",
            admin_name: "rodrigo",
            password: "abcd1234",
            theme: "kanagawa",
            line_numbers: "relative",
            device_label: "x",
        },
    )
    .await
    .unwrap();
    assert!(auth::session(&pool, &sid).await.unwrap().is_some());
    assert_eq!(
        settings::member_defaults(&pool).await.unwrap().theme.key(),
        "kanagawa"
    );
}

#[tokio::test]
async fn density_starts_comfortable_and_takes_only_its_three_values() {
    let pool = pool().await;
    let me = household(&pool).await;
    // The founder, and the household's default for new members, both start
    // on the ledger as it always looked.
    assert_eq!(settings_of(&pool, me).await.density, "comfortable");
    assert_eq!(
        settings::member_defaults(&pool).await.unwrap().density,
        "comfortable"
    );

    for v in ["default", "compact", "comfortable"] {
        settings::set_member(&pool, me, MemberKey::Density, v)
            .await
            .unwrap();
        assert_eq!(settings_of(&pool, me).await.density, v);
    }
    let member_says = settings::set_member(&pool, me, MemberKey::Density, "cozy")
        .await
        .unwrap_err()
        .to_string();
    let household_says = settings::set_household(&pool, HouseholdKey::DefaultDensity, "cozy")
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(member_says, household_says, "one bound, whoever writes it");
    assert_eq!(settings_of(&pool, me).await.density, "comfortable");
}
