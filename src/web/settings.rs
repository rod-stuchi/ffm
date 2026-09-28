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
    themes: Vec<Swatch>,
    languages: Vec<Choice>,
    line_numbers: Vec<Choice>,
    panes: Vec<Choice>,
    sorts: Vec<Choice>,
    scopes: Vec<Choice>,
    folds: Vec<Choice>,
    orders: Vec<Choice>,
    visibilities: Vec<Choice>,
    min_split: i64,
    min_width: i64,
    max_width: i64,
    gap: i64,
    show_delta: bool,
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
        .unwrap_or_default();

    Html(ProfilePane {
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
        panes: choices(
            &[
                ("auto", "auto".into()),
                ("split", "split".into()),
                ("tabs", "tabs".into()),
            ],
            &s.layout_panes,
        ),
        sorts: choices(
            &[
                ("manual", t.profile_sort_manual().to_string()),
                ("date", t.profile_sort_date().to_string()),
                ("amount", t.profile_sort_amount().to_string()),
            ],
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
        show_delta: s.ledger_show_delta,
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
                let css = crate::ui::theme::render(
                    settings::member_settings(&st.pool, auth.viewer.member_id)
                        .await
                        .unwrap_or_default()
                        .theme,
                    auth.mode,
                    &auth.settings.theme_overrides,
                )
                .to_css();
                // `hx-swap-oob` is honored even though the form asks for
                // `hx-swap="none"`: out-of-band content is matched by id and
                // applied separately from the (absent) main swap.
                let body = format!(r#"<style id="ffm-theme" hx-swap-oob="true">{css}</style>"#);
                return crate::web::tx::say(
                    axum::response::Html(body).into_response(),
                    &t.settings_saved(&form.key),
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
            );
            let mut res = crate::web::tx::say_only(&t.settings_saved(&form.key), false);
            if whole_page {
                res.headers_mut()
                    .insert("HX-Refresh", axum::http::HeaderValue::from_static("true"));
            }
            res
        }
        Err(e) => crate::web::tx::say_only(&e.to_string(), true),
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
    db_path: String,
    receipts_root: String,
    backup_dir: String,
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

    Html(HouseholdPane {
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
            // statusline and the heading.
            let mut res = crate::web::tx::say_only(&t.settings_saved(&form.key), false);
            res.headers_mut()
                .insert("HX-Refresh", axum::http::HeaderValue::from_static("true"));
            res
        }
        Err(e) => crate::web::tx::say_only(&e.to_string(), true),
    }
}

// ---------------------------------------------------------------------------
// :members
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "partials/members_pane.html")]
struct MembersPane {
    t: Catalog,
    members: Vec<members::MemberRow>,
    invites: Vec<members::PendingInvite>,
    /// Shown **once**, right after it is created. Never stored in a form and
    /// never re-shown: the token itself is not in the database to re-show.
    new_link: Option<String>,
    me: i64,
}

async fn members_pane(st: &AppState, auth: &Auth, new_link: Option<String>) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
    }
    Html(MembersPane {
        members: members::list(&st.pool).await.unwrap_or_default(),
        invites: members::pending_invites(&st.pool).await.unwrap_or_default(),
        new_link,
        me: auth.viewer.member_id,
        t,
    })
    .into_response()
}

pub async fn members_view(State(st): State<AppState>, auth: Auth) -> Response {
    members_pane(&st, &auth, None).await
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
        Ok(token) => members_pane(&st, &auth, Some(format!("/invite/{token}"))).await,
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

/// A reset for somebody who has forgotten theirs. Same link mechanism as an
/// invite, pointed at a member who already exists — which is why it never
/// renames them and never orphans their receipt-vault folder.
pub async fn reset(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
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
        Ok(token) => members_pane(&st, &auth, Some(format!("/invite/{token}"))).await,
        Err(_) => crate::web::tx::say_only(t.error_not_written(), true),
    }
}

pub async fn sign_out(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
    }
    let name = members::list(&st.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.id == id)
        .map(|m| m.name)
        .unwrap_or_default();

    let n = members::sign_out_everywhere(&st.pool, auth.viewer.member_id, id)
        .await
        .unwrap_or(0);
    let mut res = members_pane(&st, &auth, None).await;
    let msg = t.members_signed_out(&name, n as i64);
    res.headers_mut().insert(
        "HX-Trigger",
        axum::http::HeaderValue::from_str(&crate::web::tx::ascii_json(
            &serde_json::json!({ "ffm:say": { "message": msg, "bad": false } }).to_string(),
        ))
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("{}")),
    );
    res
}

pub async fn revoke(State(st): State<AppState>, auth: Auth, Path(id): Path<i64>) -> Response {
    let t = auth.catalog();
    if !auth.viewer.is_admin() {
        return crate::web::tx::say_only(t.settings_admin_only(), true);
    }
    let _ = members::revoke_invite(&st.pool, id).await;
    members_pane(&st, &auth, None).await
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
