//! `:profile`, `:household` and `:members` — vertical splits beside the
//! ledger, never modals.
//!
//! A full-screen buffer hides the ledger, and `layout.panes` is precisely the
//! setting you want to *watch* take effect. A modal needs a backdrop, a scroll
//! lock and a focus trap, and it implies a dialog with an OK button — a
//! second, pending copy of the truth.
//!
//! **These panes have no Save button, and that is the point.** Every control
//! writes on `change`, the footer confirms and the statusline echoes. There is
//! no pending state, so there is no moment when the form and the row disagree
//! — which is what the single-source-of-truth rule actually forbids.

use crate::db::auth as dbauth;
use crate::db::ledger;
use crate::db::members;
use crate::db::settings::{self, HouseholdKey, MemberKey};
use crate::domain::money::{Cents, Currency};
use crate::state::AppState;
use crate::ui::fmt;
use crate::ui::i18n::{Catalog, Lang};
use crate::ui::theme::{self, ThemeName};
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

pub struct Choice {
    pub code: String,
    pub label: String,
    pub selected: bool,
}

fn choices(options: &[(&str, String)], selected: &str) -> Vec<Choice> {
    options
        .iter()
        .map(|(code, label)| Choice {
            code: (*code).to_string(),
            label: label.clone(),
            selected: *code == selected,
        })
        .collect()
}

/// The three densities, roomiest first, as both `:profile` and `:household`
/// offer them. Words, unlike the pane modes: nobody types these back.
fn density_choices(t: &Catalog, selected: &str) -> Vec<Choice> {
    choices(
        &[
            ("default", t.density_default().to_string()),
            ("comfortable", t.density_comfortable().to_string()),
            ("compact", t.density_compact().to_string()),
        ],
        selected,
    )
}

/// The three pane modes, as both `:profile` and `:household` offer them.
/// Identifiers a member types back (`auto`, `split`, `tabs`), so no catalog.
fn pane_choices(selected: &str) -> Vec<Choice> {
    choices(
        &[
            ("auto", "auto".into()),
            ("split", "split".into()),
            ("tabs", "tabs".into()),
        ],
        selected,
    )
}

pub struct Swatch {
    pub key: &'static str,
    pub label: &'static str,
    pub bars: Vec<String>,
    pub selected: bool,
}

fn swatches(selected: ThemeName) -> Vec<Swatch> {
    ThemeName::ALL
        .iter()
        .map(|n| {
            let p = theme::theme(*n).light;
            Swatch {
                key: n.key(),
                label: n.label(),
                bars: vec![
                    p.paper.to_string(),
                    p.surface_2.to_string(),
                    p.accent.to_string(),
                    p.pos.to_string(),
                    p.neg.to_string(),
                ],
                selected: *n == selected,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// :profile
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "partials/profile_pane.html")]
struct ProfilePane {
    t: Catalog,
    /// `v0.0.3` — the pane's first line, so "which version is this?" has an
    /// answer without a new page.
    version: &'static str,
    /// `PROD` · `dev` · `dev-lan · 15/10/2026` (`crate::run::line`).
    run: String,
    /// Whether this member's palette or layout differ from what the household
    /// gives new members — the "Use the household's defaults" button is there
    /// only then, absent rather than disabled when there is nothing to adopt.
    household_differs: bool,
    themes: Vec<Swatch>,
    languages: Vec<Choice>,
    line_numbers: Vec<Choice>,
    panes: Vec<Choice>,
    sorts: Vec<Choice>,
    scopes: Vec<Choice>,
    folds: Vec<Choice>,
    pdf_views: Vec<Choice>,
    orders: Vec<Choice>,
    visibilities: Vec<Choice>,
    min_split: i64,
    min_width: i64,
    max_width: i64,
    gap: i64,
    densities: Vec<Choice>,
    show_delta: bool,
    /// Whether the panes out of focus recede (`member_settings.dim_unfocused`).
    dim: bool,
    devices: Vec<String>,
}

/// The **session's** settings, never the focused pane's. Focusing somebody
/// else's ledger must not hand you their preferences.
pub async fn profile(State(st): State<AppState>, auth: Auth) -> Response {
    let t = auth.catalog();
    let s = &auth.settings;
    let house = ledger::household(&st.pool).await.ok().flatten();
    let household_default = house
        .as_ref()
        .map(|h| h.default_visibility.clone())
        .unwrap_or_else(|| "household".into());

    let devices = members::list(&st.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.id == auth.viewer.member_id)
        .map(|m| m.devices)
        .unwrap_or_default()
        .into_iter()
        .map(|d| crate::db::auth::device_text(&d, &t))
        .collect();

    Html(ProfilePane {
        version: crate::version(),
        run: crate::run::line(),
        household_differs: !settings::member_defaults(&st.pool)
            .await
            .unwrap_or_default()
            .matches(s),
        themes: swatches(s.theme),
        languages: choices(
            &[
                ("en", "English".into()),
                ("pt-BR", "Português (Brasil)".into()),
            ],
            s.language.code(),
        ),
        line_numbers: choices(
            &[
                ("relative", t.setup_nu_relative().to_string()),
                ("absolute", t.setup_nu_absolute().to_string()),
                ("none", t.setup_nu_none().to_string()),
            ],
            &s.line_numbers,
        ),
        panes: pane_choices(&s.layout_panes),
        // In the order `s` cycles them, so the radios read like the key.
        sorts: choices(
            &crate::db::ledger::Sort::ALL.map(|o| {
                let label = match o {
                    crate::db::ledger::Sort::Manual => t.profile_sort_manual(),
                    crate::db::ledger::Sort::Name => t.profile_sort_name(),
                    crate::db::ledger::Sort::Amount => t.profile_sort_amount(),
                    crate::db::ledger::Sort::Date => t.profile_sort_date(),
                };
                (o.code(), label.to_string())
            }),
            &s.ledger_sort,
        ),
        scopes: choices(
            &[
                ("pane", t.profile_scope_pane().to_string()),
                ("household", t.profile_scope_household().to_string()),
            ],
            &s.ledger_search_scope,
        ),
        folds: choices(
            &[
                ("expanded", t.profile_fold_expanded().to_string()),
                ("collapsed", t.profile_fold_collapsed().to_string()),
            ],
            &s.group_fold_default,
        ),
        // Format names, not words: the catalog has them only so `P`'s switch
        // and this radio cannot spell them differently.
        pdf_views: choices(
            &[
                ("png", t.carousel_view_pages().to_string()),
                ("pdf", t.carousel_view_pdf().to_string()),
            ],
            &s.pdf_view,
        ),
        orders: choices(
            &[
                ("appearance", t.profile_group_order_appearance().to_string()),
                ("name", t.profile_group_order_name().to_string()),
                ("subtotal", t.profile_group_order_subtotal().to_string()),
            ],
            &s.group_order,
        ),
        // The nullable tri-state, rendered honestly: the first option means
        // NULL and is labeled with what it currently resolves to, so nobody
        // has to guess what "follow the household" is following.
        visibilities: choices(
            &[
                (
                    "household_default",
                    t.profile_visibility_inherit(&household_default),
                ),
                ("household", t.profile_visibility_household().to_string()),
                ("private", t.profile_visibility_private().to_string()),
            ],
            s.default_visibility
                .as_deref()
                .unwrap_or("household_default"),
        ),
        min_split: s.layout_min_split_width,
        min_width: s.layout_min_width,
        max_width: s.layout_max_width,
        gap: s.layout_gap,
        densities: density_choices(&t, &s.density),
        show_delta: s.ledger_show_delta,
        dim: s.dim_unfocused,
        devices,
        t,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct SettingForm {
    key: String,
    #[serde(default)]
    value: String,
}

/// One control, one write. No Save, so no pending state.
pub async fn set_member(
    State(st): State<AppState>,
    auth: Auth,
    Form(form): Form<SettingForm>,
) -> Response {
    let t = auth.catalog();
    let Some(key) = MemberKey::parse(&form.key) else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };

    match settings::set_member(&st.pool, auth.viewer.member_id, key, &form.value).await {
        Ok(()) => {
            // A whole-page refresh for anything the SERVER decides per row or
            // per page. Language and theme were the obvious two — every
            // string, every color — but the same is true of the gutter, the
            // row order, grouping, the delta column and every layout value,
            // which is rendered into a media query in the page's own <style>.
            //
            // Getting this list short was a real bug: line numbers wrote the
            // row and changed nothing on screen until the next reload, so
            // `relative` and `none` looked broken rather than unapplied.
            //
            // What is deliberately NOT here is what the next render reads
            // anyway without changing what is already on screen: the fold
            // default (it applies when a grouped pane is next opened), the
            // search scope (read when you search) and the privacy default
            // (read when you add a row).
            // The palette is the exception, and deliberately: it is one
            // `<style>` block and nothing else on the page depends on it, so
            // swapping that block out of band changes every color with no
            // reload at all. Refreshing for it was the lazy read of "the
            // server renders this" — and it closed the very pane you were
            // picking from, which made trying eleven themes eleven reloads.
            //
            // This is also what `CLAUDE.md` already describes: the picker
            // previews by swapping a server-rendered style block, from the
            // same palette table the app itself uses, so the preview provably
            // cannot drift from the real thing.
            if key == MemberKey::ThemeName {
                let palette = settings::member_settings(&st.pool, auth.viewer.member_id)
                    .await
                    .unwrap_or_default()
                    .theme;
                // The phone's bar follows too: the `theme-color` tags, by id.
                // `hx-swap-oob` is honored even though the form asks for
                // `hx-swap="none"`: out-of-band content is matched by id and
                // applied separately from the (absent) main swap.
                let body =
                    crate::ui::theme::render(palette, auth.mode, &auth.settings.theme_overrides)
                        .oob("surface");
                // And the sign-in page follows: `ffm.palette` is kept in step
                // with the row here, not only at sign-out, so a session that
                // simply expires still leaves the right palette behind.
                let jar =
                    axum_extra::extract::CookieJar::new().add(crate::web::session::pref_cookie(
                        crate::web::session::PALETTE,
                        palette.key().to_string(),
                    ));
                return crate::web::tx::say(
                    (jar, axum::response::Html(body)).into_response(),
                    &t.settings_saved(&member_label(&t, key)),
                    false,
                );
            }

            let whole_page = matches!(
                key,
                MemberKey::Language
                    | MemberKey::LineNumbers
                    | MemberKey::LayoutPanes
                    | MemberKey::LayoutMinSplitWidth
                    | MemberKey::LayoutMinWidth
                    | MemberKey::LayoutMaxWidth
                    | MemberKey::LayoutGap
                    | MemberKey::LedgerSort
                    | MemberKey::LedgerShowDelta
                    | MemberKey::LedgerGroup
                    | MemberKey::GroupOrder
                    | MemberKey::Density
                    // The shell carries it (`data-pdf-view`) for `P` to open on.
                    | MemberKey::PdfView
                    // The shell carries it too (`data-dim`), for the CSS.
                    | MemberKey::DimUnfocused
            );
            let mut res =
                crate::web::tx::say_only(&t.settings_saved(&member_label(&t, key)), false);
            if whole_page {
                res.headers_mut()
                    .insert("HX-Refresh", axum::http::HeaderValue::from_static("true"));
            }
            res
        }
        Err(e) => crate::web::tx::say_only(&e.say(&t), true),
    }
}

/// `:profile`'s "Use the household's defaults": the palette and layout the
/// household gives new members, copied into the **session's** own row — never
/// the focused pane's. Layout is rendered server-side, so the page refreshes.
pub async fn adopt_household_defaults(State(st): State<AppState>, auth: Auth) -> Response {
    let t = auth.catalog();
    match settings::adopt_member_defaults(&st.pool, auth.session.member.id).await {
        Ok(()) => {
            let mut res = crate::web::tx::say_only(t.profile_used_household(), false);
            res.headers_mut()
                .insert("HX-Refresh", axum::http::HeaderValue::from_static("true"));
            res
        }
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

// ---------------------------------------------------------------------------
// :household
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "partials/household_pane.html")]
struct HouseholdPane {
    t: Catalog,
    /// `dd/mm/yyyy`, passed in rather than written in the catalog — a `.ftl`
    /// containing a date pattern fails the build, precisely so there is one
    /// place that decides it.
    date_pattern: &'static str,
    name: String,
    currencies: Vec<Choice>,
    languages: Vec<Choice>,
    visibilities: Vec<Choice>,
    show_recurring: bool,
    // What a new member's own settings start as — the palette swatches and
    // the layout band, the same partials `:profile` renders, writing the
    // household's `default_*` columns instead.
    themes: Vec<Swatch>,
    panes: Vec<Choice>,
    min_split: i64,
    min_width: i64,
    max_width: i64,
    gap: i64,
    densities: Vec<Choice>,
    /// Which CLI teaches rules to read their bills, and whether this server
    /// can run it right now — or, while that is being found out, saying so.
    assistants: Vec<Choice>,
    assistant_status: String,
    assistant_pending: bool,
    /// Days a removed file stays in the trash before `:prune` offers it for
    /// removal for good; 0 keeps it forever.
    trash_keep_days: i64,
    db_path: String,
    receipts_root: String,
    backup_dir: String,
}

/// The line under the assistant setting, on its own: `:household` asks for it
/// once the pane is on screen.
#[derive(Template)]
#[template(path = "partials/assistant_status.html")]
struct AssistantLine {
    assistant_status: String,
    assistant_pending: bool,
}

/// The line under the assistant setting: whether the server can run it, and
/// what to type there if it cannot.
fn assistant_status(
    t: &Catalog,
    p: crate::services::assistant::Provider,
    s: crate::services::assistant::Status,
) -> String {
    use crate::services::assistant::{Provider, Status};
    let login = match p {
        Provider::Codex => "codex login",
        Provider::Claude => "claude setup-token",
        Provider::Gemini => "gemini",
        Provider::None => "",
    };
    match s {
        Status::Off => t.household_assistant_off().to_string(),
        Status::NotFound => t.household_assistant_not_found(p.code()),
        Status::LoggedOut => t.household_assistant_logged_out(p.code(), login),
        Status::Ready => t.household_assistant_ready(p.code()),
        Status::Installed => t.household_assistant_installed(p.code()),
    }
}

pub async fn household(State(st): State<AppState>, auth: Auth) -> Response {
    let t = auth.catalog();
    // Admin-only, and under preview it refuses exactly as it would for a real
    // non-admin — the Household button is gone from the toolbar too, rather
    // than sitting there disabled.
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
    }
    let Some(h) = ledger::household(&st.pool).await.ok().flatten() else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    let d = settings::member_defaults(&st.pool)
        .await
        .unwrap_or_default();
    // Whether the server can run the assistant is filled in once the pane is
    // on screen (`household_assistant`): finding out launches the CLI twice,
    // and the form should not wait for it. With none chosen there is nothing
    // to find out.
    let provider = settings::assistant(&st.pool).await;
    let (status_line, pending) = match provider {
        crate::services::assistant::Provider::None => (
            assistant_status(&t, provider, crate::services::assistant::Status::Off),
            false,
        ),
        _ => (t.household_assistant_checking(provider.code()), true),
    };

    Html(HouseholdPane {
        assistants: crate::services::assistant::Provider::ALL
            .iter()
            .map(|p| Choice {
                code: p.code().to_string(),
                label: match p {
                    crate::services::assistant::Provider::None => {
                        t.household_assistant_none().to_string()
                    }
                    // Product names, not copy.
                    crate::services::assistant::Provider::Codex => "Codex · ChatGPT".to_string(),
                    crate::services::assistant::Provider::Claude => "Claude".to_string(),
                    crate::services::assistant::Provider::Gemini => "Gemini".to_string(),
                },
                selected: *p == provider,
            })
            .collect(),
        assistant_status: status_line,
        assistant_pending: pending,
        trash_keep_days: settings::trash_keep_days(&st.pool).await,
        themes: swatches(d.theme),
        panes: pane_choices(&d.panes),
        min_split: d.min_split,
        min_width: d.min_width,
        max_width: d.max_width,
        gap: d.gap,
        densities: density_choices(&t, &d.density),
        name: h.name.clone(),
        date_pattern: fmt::DATE_PATTERN,
        currencies: Currency::ALL
            .iter()
            .map(|c| Choice {
                code: c.code().to_string(),
                // The FORMAT, not the ISO code: picking a currency picks its
                // whole number format as one preset, so the label says so.
                label: format!(
                    "{} · {}",
                    match c {
                        Currency::Brl => t.currency_brl(),
                        Currency::Usd => t.currency_usd(),
                        Currency::Eur => t.currency_eur(),
                        Currency::Gbp => t.currency_gbp(),
                    },
                    fmt::money(Cents(123_456), *c)
                ),
                selected: *c == h.currency,
            })
            .collect(),
        languages: choices(
            &[
                ("en", "English".into()),
                ("pt-BR", "Português (Brasil)".into()),
            ],
            h.default_language.code(),
        ),
        visibilities: choices(
            &[
                ("household", t.profile_visibility_household().to_string()),
                ("private", t.profile_visibility_private().to_string()),
            ],
            &h.default_visibility,
        ),
        show_recurring: h.show_recurring,
        // The operator's three, read-only. A live editor here would accept
        // input and silently not apply it, which is worse than any dialog.
        db_path: st.cfg.db_path.display().to_string(),
        receipts_root: st.cfg.receipts_root.display().to_string(),
        backup_dir: st.cfg.backup_dir.display().to_string(),
        t,
    })
    .into_response()
}

/// The assistant's line in `:household`, asked for once the pane is on
/// screen: it runs the CLI's own `--version` and login check, off the async
/// runtime, and those launches used to sit between the click and the form.
pub async fn household_assistant(State(st): State<AppState>, auth: Auth) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
    }
    let provider = settings::assistant(&st.pool).await;
    let status = tokio::task::spawn_blocking(move || crate::services::assistant::status(provider))
        .await
        .unwrap_or(crate::services::assistant::Status::NotFound);
    Html(AssistantLine {
        assistant_status: assistant_status(&t, provider, status),
        assistant_pending: false,
    })
    .into_response()
}

pub async fn set_household(
    State(st): State<AppState>,
    auth: Auth,
    Form(form): Form<SettingForm>,
) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
    }
    let Some(key) = HouseholdKey::parse(&form.key) else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };

    match settings::set_household(&st.pool, key, &form.value).await {
        Ok(()) => {
            // Currency moves every amount on the page; the name moves the
            // statusline and the heading. A new member's defaults move
            // nothing anybody can see — they are read only when somebody
            // joins — so they write quietly.
            let mut res =
                crate::web::tx::say_only(&t.settings_saved(&household_label(&t, key)), false);
            if key.changes_the_screen() {
                res.headers_mut()
                    .insert("HX-Refresh", axum::http::HeaderValue::from_static("true"));
            }
            res
        }
        Err(e) => crate::web::tx::say_only(&e.say(&t), true),
    }
}

// ---------------------------------------------------------------------------
// :members
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "partials/members_pane.html")]
struct MembersPane {
    t: Catalog,
    active: Vec<members::MemberRow>,
    inactive: Vec<members::MemberRow>,
    trashed: Vec<members::MemberRow>,
    invites: Vec<members::PendingInvite>,
    /// The link just created or reset, lit and selected in its own row. It is
    /// in the list like every other pending link now — the token is kept
    /// while the link is pending (migration 0009), so nothing is "shown once".
    /// Its id, `0` for none — ids start at 1.
    fresh: i64,
    me: i64,
    /// Which tab the pane opens on: the one the member acted on was in, so
    /// turning somebody back on does not bounce you to Active.
    tab: &'static str,
    /// "Links stop working 5 days after they are created."
    link_note: String,
}

async fn members_pane(
    st: &AppState,
    auth: &Auth,
    tab: &'static str,
    fresh_token: Option<&str>,
) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
    }
    let mut all = members::list(&st.pool).await.unwrap_or_default();
    for m in &mut all {
        m.devices = m
            .devices
            .iter()
            .map(|d| crate::db::auth::device_text(d, &t))
            .collect();
    }
    let invites = members::pending_invites(&st.pool).await.unwrap_or_default();
    let of = |s: members::Status| all.iter().filter(|m| m.status == s).cloned().collect();
    Html(MembersPane {
        active: of(members::Status::Active),
        inactive: of(members::Status::Inactive),
        trashed: of(members::Status::Trashed),
        fresh: fresh_token
            .and_then(|tok| {
                invites
                    .iter()
                    .find(|i| i.token.as_deref() == Some(tok))
                    .map(|i| i.id)
            })
            .unwrap_or(0),
        invites,
        me: auth.viewer.member_id,
        tab,
        link_note: t.members_link_days(dbauth::LINK_DAYS),
        t,
    })
    .into_response()
}

pub async fn members_view(State(st): State<AppState>, auth: Auth) -> Response {
    members_pane(&st, &auth, "active", None).await
}

/// The pane again, with a line in the statusline — how every action here
/// answers, refusals included. A refusal must still send the pane: these
/// swap `#settingspane` whole, and an empty answer would remove it.
async fn members_say(
    st: &AppState,
    auth: &Auth,
    tab: &'static str,
    message: &str,
    bad: bool,
) -> Response {
    crate::web::tx::say(members_pane(st, auth, tab, None).await, message, bad)
}

fn this_month() -> crate::domain::period::Period {
    crate::domain::period::Period::of(crate::clock::today())
}

fn tab_of(m: &members::MemberRow) -> &'static str {
    match m.status {
        members::Status::Active => "active",
        members::Status::Inactive => "inactive",
        members::Status::Trashed => "trash",
    }
}

fn refusal(t: &Catalog, r: members::Refusal) -> String {
    match r {
        members::Refusal::Yourself => t.members_not_yourself().to_string(),
        members::Refusal::LastAdmin => t.members_last_admin().to_string(),
        members::Refusal::Unknown => t.members_unknown().to_string(),
        members::Refusal::HasRows => t.members_has_rows().to_string(),
    }
}

/// The guard every account action shares: an admin, not on themselves, not
/// the household's last admin, a member who exists. The pane hides what does
/// not apply; this is the rule.
async fn target(st: &AppState, auth: &Auth, id: i64) -> Result<members::MemberRow, Response> {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return Err(crate::web::tx::say_only(t.settings_admin_only(), true));
    }
    match members::may_change(&st.pool, auth.viewer.member_id, id).await {
        Ok(Ok(m)) => Ok(m),
        Ok(Err(r)) => Err(members_say(st, auth, "active", &refusal(&t, r), true).await),
        Err(_) => Err(crate::web::tx::say_only(t.error_not_written(), true)),
    }
}

#[derive(Deserialize)]
pub struct InviteForm {
    #[serde(default)]
    label: String,
    #[serde(default)]
    role: String,
}

pub async fn invite(
    State(st): State<AppState>,
    auth: Auth,
    Form(form): Form<InviteForm>,
) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
    }
    let label = form.label.trim();
    let role = if form.role == "0" { 0 } else { 1 };

    match dbauth::issue_link(
        &st.pool,
        // A person, at a keyboard — as opposed to the console path, which has
        // no member behind it and may only issue a reset.
        crate::db::changes::Actor::User(auth.viewer.member_id),
        None,
        (!label.is_empty()).then_some(label),
        role,
    )
    .await
    {
        Ok(token) => members_pane(&st, &auth, "active", Some(&token)).await,
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

/// A reset for somebody who has forgotten theirs. Same link mechanism as an
/// invite, pointed at a member who already exists — which is why it never
/// renames them and never orphans their receipt-vault folder. Only for an
/// active member: somebody turned off has no password to set.
pub async fn reset(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    let m = match target(&st, &auth, id).await {
        Ok(m) => m,
        Err(res) => return res,
    };
    if m.status != members::Status::Active {
        return members_say(&st, &auth, tab_of(&m), t.members_unknown(), true).await;
    }
    match dbauth::issue_link(
        &st.pool,
        crate::db::changes::Actor::User(auth.viewer.member_id),
        Some(id),
        None,
        1,
    )
    .await
    {
        Ok(token) => {
            // A reset signs the member out everywhere — see `issue_link` — and
            // their open pages leave now, not at the stream's next re-check.
            st.live.sessions_revoked();
            members_pane(&st, &auth, "active", Some(&token)).await
        }
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

/// Your own row's sign-out: every device but this one, so the admin stays
/// in the pane they clicked it from. `target()` refuses yourself for the
/// other account actions — this one is only ever about yourself.
pub async fn sign_out_others(State(st): State<AppState>, auth: Auth) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
    }
    match members::sign_out_others(&st.pool, auth.session.member.id, &auth.session.id).await {
        Ok(n) => {
            // The other devices go to /login now, not at their next click.
            st.live.sessions_revoked();
            members_say(
                &st,
                &auth,
                "active",
                &t.members_signed_out_others(n as i64),
                false,
            )
            .await
        }
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

pub async fn sign_out(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    let m = match target(&st, &auth, id).await {
        Ok(m) => m,
        Err(res) => return res,
    };
    let n = members::sign_out_everywhere(&st.pool, auth.viewer.member_id, id)
        .await
        .unwrap_or(0);
    st.live.sessions_revoked();
    let msg = t.members_signed_out(&m.name, n as i64);
    members_say(&st, &auth, tab_of(&m), &msg, false).await
}

/// Turn a member off from this month on.
pub async fn disable(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    let m = match target(&st, &auth, id).await {
        Ok(m) => m,
        Err(res) => return res,
    };
    match members::disable(&st.pool, auth.viewer.member_id, id, this_month()).await {
        Ok(_) => {
            // Turning a member off signs them out everywhere; their open pages
            // leave now.
            st.live.sessions_revoked();
            members_say(&st, &auth, "active", &t.members_disabled(&m.name), false).await
        }
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

/// Turn them back on from this month on.
pub async fn enable(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    let m = match target(&st, &auth, id).await {
        Ok(m) => m,
        Err(res) => return res,
    };
    match members::enable(&st.pool, auth.viewer.member_id, id, this_month()).await {
        Ok(_) => members_say(&st, &auth, "inactive", &t.members_enabled(&m.name), false).await,
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

/// Into the trash — only a member with no rows.
pub async fn trash(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    let m = match target(&st, &auth, id).await {
        Ok(m) => m,
        Err(res) => return res,
    };
    match members::trash(&st.pool, auth.viewer.member_id, id).await {
        Ok(Ok(())) => members_say(&st, &auth, tab_of(&m), &t.members_trashed(&m.name), false).await,
        Ok(Err(r)) => members_say(&st, &auth, tab_of(&m), &refusal(&t, r), true).await,
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

/// Out of the trash, as they were.
pub async fn restore(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    let m = match target(&st, &auth, id).await {
        Ok(m) => m,
        Err(res) => return res,
    };
    match members::restore(&st.pool, auth.viewer.member_id, id).await {
        Ok(_) => members_say(&st, &auth, "trash", &t.members_restored(&m.name), false).await,
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

pub async fn revoke(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
    }
    let _ = members::revoke_invite(&st.pool, id).await;
    members_pane(&st, &auth, "active", None).await
}

/// Language is per member and changes the words only — the same write the
/// `:lang` command makes, so there is one path into that column.
pub async fn set_lang_member(st: &AppState, auth: &Auth, lang: Lang) -> Result<(), String> {
    settings::set_member(
        &st.pool,
        auth.viewer.member_id,
        MemberKey::Language,
        lang.code(),
    )
    .await
    .map_err(|e| e.to_string())
}

/// What a member setting is called where it is set — its control's label in
/// `:profile` — for "… saved". The key itself (`theme_name`) is a column name,
/// and reading one back in the statusline is the app talking to itself.
/// Exhaustive, so a new setting cannot go unnamed.
fn member_label(t: &Catalog, key: MemberKey) -> String {
    match key {
        MemberKey::Language => t.invite_language(),
        MemberKey::ThemeName => t.profile_theme(),
        MemberKey::LineNumbers => t.profile_line_numbers(),
        MemberKey::LayoutPanes => t.profile_panes(),
        MemberKey::LayoutMinSplitWidth => t.profile_min_split(),
        MemberKey::LayoutMinWidth => t.profile_min_width(),
        MemberKey::LayoutMaxWidth => t.profile_max_width(),
        MemberKey::LayoutGap => t.profile_gap(),
        MemberKey::LedgerSort => t.profile_sort(),
        MemberKey::LedgerSearchScope => t.profile_search_scope(),
        MemberKey::LedgerShowDelta => t.profile_show_delta(),
        MemberKey::LedgerGroup => t.settings_label_group(),
        MemberKey::GroupFoldDefault => t.profile_fold_default(),
        MemberKey::GroupOrder => t.profile_group_order(),
        MemberKey::DefaultVisibility => t.profile_privacy(),
        MemberKey::Density => t.profile_density(),
        MemberKey::PdfView => t.profile_pdf_view(),
        MemberKey::DimUnfocused => t.profile_dim_unfocused(),
    }
    .to_string()
}

/// The same for a household setting, by its control's label in `:household`.
/// A new member's defaults are set with the very controls `:profile` uses, so
/// they read the same.
fn household_label(t: &Catalog, key: HouseholdKey) -> String {
    match key {
        HouseholdKey::Name => t.household_name(),
        HouseholdKey::Currency => t.household_currency(),
        HouseholdKey::DefaultLanguage => t.household_language(),
        HouseholdKey::DefaultVisibility => t.household_visibility(),
        HouseholdKey::ShowRecurring => t.household_show_recurring(),
        HouseholdKey::DefaultTheme => t.profile_theme(),
        HouseholdKey::DefaultPanes => t.profile_panes(),
        HouseholdKey::DefaultMinSplitWidth => t.profile_min_split(),
        HouseholdKey::DefaultMinWidth => t.profile_min_width(),
        HouseholdKey::DefaultMaxWidth => t.profile_max_width(),
        HouseholdKey::DefaultGap => t.profile_gap(),
        HouseholdKey::DefaultDensity => t.profile_density(),
        HouseholdKey::Assistant => t.household_assistant(),
        HouseholdKey::TrashKeepDays => t.household_trash_keep(),
    }
    .to_string()
}
