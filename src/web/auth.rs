//! The three ways in, and the one way out.
//!
//! `/setup` (first run only), `/invite/:token` (single-use, admin-issued) and
//! `/login`. **There is no `/signup`, and no handler here creates one** —
//! household membership is closed, which is a product decision, not a missing
//! feature.
//!
//! None of these pages is modal. The app is Vim-first everywhere a ledger is on
//! screen; a login page is 100% form, so the rule this project already follows
//! — literal text takes literal keys — yields plain `Tab` and `Enter`. On
//! `/invite`, the person typing has provably never used this app.

use crate::db::auth::{self, AcceptError, Link, MIN_PASSWORD};
use crate::db::ledger;
use crate::domain::money::{Cents, Currency};
use crate::state::AppState;
use crate::ui::i18n::{Catalog, Lang};
use crate::ui::theme::{self, ModePref, ThemeName};
use crate::ui::{fmt, theme::Overrides};
use crate::web::form::FieldErrors;
use crate::web::session::{self, Auth};
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Form, Path, State};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// chrome
// ---------------------------------------------------------------------------

/// The one thing the server refuses about a new password, in the words the
/// live meter (`auth/pwmeter.html`) shows before the server has to.
pub fn too_short(t: &Catalog) -> String {
    t.password_too_short(MIN_PASSWORD as i64)
}

/// Everything the pre-auth shell needs, so each page's struct carries one field
/// instead of twelve.
pub struct Chrome {
    pub household: String,
    pub household_slug: String,
    pub theme_css: String,
    /// The palette's key, for the statusline: `ffm` unless somebody has
    /// signed in on this device.
    pub palette: &'static str,
    pub attr: Option<&'static str>,
    pub lang_code: &'static str,
    pub lang_label: &'static str,
    /// The two chrome buttons carry the ledger bar's icons, so both sides of
    /// the login draw the same control.
    pub lang_svg: &'static str,
    pub mode_svg: &'static str,
    pub next_lang: &'static str,
    pub mode_label: String,
    pub next_mode: &'static str,
    pub where_label: String,
    pub path: String,
    pub msg: String,
    pub msg_bad: bool,
    /// A dev build's badge — shown before you sign in too, since `/login`
    /// is where you would first mistake one server for another.
    pub run: Option<crate::run::Badge>,
}

pub struct Choice {
    pub code: &'static str,
    pub label: &'static str,
    pub selected: bool,
}

pub struct CurrencyChoice {
    pub code: &'static str,
    pub name: String,
    /// Rendered by `ui::fmt::money` from the same table the ledger uses, never
    /// written out in the catalog — the label has to *show* the format,
    /// because picking a currency picks the whole format as one preset.
    pub sample: String,
    pub selected: bool,
}

pub struct Swatch {
    pub key: &'static str,
    pub label: &'static str,
    pub bars: Vec<String>,
    pub selected: bool,
}

/// Which pre-auth page this is, for the statusline's place cell — worded by
/// the catalog, in the language the page itself is in.
#[derive(Clone, Copy)]
enum Where {
    Login,
    Setup,
    Invite,
}

async fn chrome(
    st: &AppState,
    jar: &CookieJar,
    parts: &Parts,
    place: Where,
    path: &str,
) -> (Chrome, Catalog) {
    let house = ledger::household(&st.pool).await.ok().flatten();
    let lang = session::pre_auth_lang(jar, parts, house.as_ref().map(|h| h.default_language));
    let mode = session::mode_of(jar);
    let palette = session::palette_of(jar);
    let t = Catalog::new(lang);

    let name = house
        .as_ref()
        .map(|h| h.name.clone())
        .unwrap_or_else(|| "FFM".into());

    // The label names the state you are IN; the form submits the next one.
    // The cycle itself lives on `ModePref`, because the ledger bar runs the
    // same one and the two must not disagree about the order.
    let mode_label = mode_label(&t, mode);
    let next_mode = mode.next();

    let chrome = Chrome {
        run: crate::run::badge(&t),
        household_slug: crate::domain::slug::slugify(&name),
        household: name,
        // The palette of whoever last signed in on this device, from the
        // `ffm.palette` cookie their session kept in step — `ffm` for a device
        // nobody has. Still no picker here: a palette chosen by an anonymous
        // visitor would have nowhere to live. Light/dark comes from its own
        // cookie, which is why those are two facts stored in two places.
        theme_css: theme::render(palette, mode, &Overrides::default()).to_css(),
        palette: palette.key(),
        attr: mode.attr(),
        lang_code: lang.code(),
        lang_label: match lang {
            Lang::En => "EN",
            Lang::PtBr => "PT",
        },
        lang_svg: crate::ui::icons::svg("globe"),
        mode_svg: crate::ui::icons::svg(mode.icon()),
        next_lang: match lang {
            Lang::En => "pt-BR",
            Lang::PtBr => "en",
        },
        mode_label: mode_label.to_string(),
        next_mode,
        where_label: match place {
            Where::Login => t.auth_where_login(),
            Where::Setup => t.auth_where_setup(),
            Where::Invite => t.auth_where_invite(),
        }
        .to_string(),
        path: path.to_string(),
        msg: String::new(),
        msg_bad: false,
    };
    (chrome, t)
}

fn languages(selected: Lang) -> Vec<Choice> {
    vec![
        Choice {
            code: "en",
            label: "English",
            selected: selected == Lang::En,
        },
        Choice {
            code: "pt-BR",
            // The name of a language is written in that language. It is not a
            // catalog string: "Português" does not become "Portuguese" because
            // the UI happens to be in English.
            label: "Português (Brasil)",
            selected: selected == Lang::PtBr,
        },
    ]
}

fn currencies(t: &Catalog, selected: Currency) -> Vec<CurrencyChoice> {
    Currency::ALL
        .iter()
        .map(|c| CurrencyChoice {
            code: c.code(),
            name: match c {
                Currency::Brl => t.currency_brl(),
                Currency::Usd => t.currency_usd(),
                Currency::Eur => t.currency_eur(),
                Currency::Gbp => t.currency_gbp(),
            }
            .to_string(),
            sample: fmt::money(Cents(123_456), *c),
            selected: *c == selected,
        })
        .collect()
}

/// The 11 palettes, each shown as its own colors rather than its name alone —
/// a name is not a preview.
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
// /login
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "auth/login.html")]
/// The one form in this app that deliberately does **not** report per field.
///
/// `/login` gives one message for both a wrong name and a wrong password, and
/// that is a decision with a reason (see design/spec.md): the two lead to the
/// same next action, and naming the field would say which half was right. It
/// already does the other half of the per-field rule — focus comes back to
/// the password, which is where the correction almost always is.
struct LoginPage {
    chrome: Chrome,
    t: Catalog,
    name: String,
    failed: bool,
    /// The right password for a member who is turned off. Said plainly —
    /// only somebody who knows the password learns it, and their next step
    /// is not to try again but to ask an admin.
    inactive: bool,
    /// Arrived here because this device's session was revoked while the page
    /// was open — another device signed it out, or an admin did (`/live`'s
    /// `signedout`). Said, so the sign-in page is not a mystery.
    signed_out: bool,
}

pub async fn login_page(State(st): State<AppState>, jar: CookieJar, parts: Parts) -> Response {
    // A database with no members has no account to sign into, so sending
    // somebody here is a dead end. The two pages point at each other rather
    // than leaving a state where the app is running and unreachable.
    if auth::is_fresh(&st.pool).await.unwrap_or(false) {
        return Redirect::to("/setup").into_response();
    }
    if let Some(sid) = jar.get(session::SID)
        && auth::session(&st.pool, sid.value())
            .await
            .ok()
            .flatten()
            .is_some()
    {
        return Redirect::to("/").into_response();
    }

    let signed_out = parts
        .uri
        .query()
        .is_some_and(|q| q.split('&').any(|kv| kv == "out=1"));
    let (chrome, t) = chrome(&st, &jar, &parts, Where::Login, "/login").await;
    Html(LoginPage {
        chrome,
        t,
        name: String::new(),
        failed: false,
        inactive: false,
        signed_out,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct LoginForm {
    name: String,
    password: String,
}

pub async fn login(
    State(st): State<AppState>,
    jar: CookieJar,
    parts: Parts,
    Form(form): Form<LoginForm>,
) -> Response {
    match auth::authenticate(&st.pool, &form.name, &form.password).await {
        Ok(Some(member_id))
            if crate::db::members::is_inactive(&st.pool, member_id)
                .await
                .unwrap_or(false) =>
        {
            let (chrome, t) = chrome(&st, &jar, &parts, Where::Login, "/login").await;
            Html(LoginPage {
                chrome,
                t,
                name: form.name,
                failed: false,
                inactive: true,
                signed_out: false,
            })
            .into_response()
        }
        Ok(Some(member_id)) => {
            let label = auth::device_label(session::user_agent(&parts));
            let Ok(mut tx) = st.pool.begin().await else {
                return failed_login(&st, &jar, &parts, form.name).await;
            };
            let Ok(sid) = auth::start_session(&mut tx, member_id, &label).await else {
                return failed_login(&st, &jar, &parts, form.name).await;
            };
            if tx.commit().await.is_err() {
                return failed_login(&st, &jar, &parts, form.name).await;
            }
            // The next visit's sign-in page greets you in your own language:
            // the cookie is refreshed from the member's row at session
            // creation. It is never *read* on an authenticated request.
            // The palette cookie likewise, so the sign-in page after this
            // session looks like the app it came from.
            let prefs = crate::db::settings::member_settings(&st.pool, member_id)
                .await
                .unwrap_or_default();
            let jar = jar
                .add(session::session_cookie(sid))
                .add(session::pref_cookie(
                    session::LANG,
                    prefs.language.code().to_string(),
                ))
                .add(session::pref_cookie(
                    session::PALETTE,
                    prefs.theme.key().to_string(),
                ));
            (jar, Redirect::to("/")).into_response()
        }
        _ => failed_login(&st, &jar, &parts, form.name).await,
    }
}

/// One message for both a wrong name and a wrong password. Not security
/// theater — this design already declines rate limiting on the grounds that the
/// network is the boundary. It is that "we have no rodrigo" and "wrong
/// password" lead to the same next action, and the copy rule says tell people
/// what to do about it.
async fn failed_login(st: &AppState, jar: &CookieJar, parts: &Parts, name: String) -> Response {
    let (chrome, t) = chrome(st, jar, parts, Where::Login, "/login").await;
    Html(LoginPage {
        chrome,
        t,
        name,
        failed: true,
        inactive: false,
        signed_out: false,
    })
    .into_response()
}

pub async fn logout(State(st): State<AppState>, jar: CookieJar) -> Response {
    let mut jar = jar;
    if let Some(sid) = jar.get(session::SID).map(|c| c.value().to_string()) {
        // Read the palette before the session goes: the sign-in page you land
        // on should look like the app you just left, including a palette
        // changed on another device since this one last signed in.
        if let Ok(Some(s)) = auth::session(&st.pool, &sid).await {
            let theme = crate::db::settings::member_settings(&st.pool, s.member.id)
                .await
                .map(|m| m.theme)
                .unwrap_or_default();
            jar = jar.add(session::pref_cookie(
                session::PALETTE,
                theme.key().to_string(),
            ));
        }
        let _ = auth::end_session(&st.pool, &sid).await;
        // Other tabs on this session are signed out too; tell them.
        st.live.sessions_revoked();
    }
    (jar.add(session::clear_session()), Redirect::to("/login")).into_response()
}

// ---------------------------------------------------------------------------
// /setup
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "auth/setup.html")]
struct SetupPage {
    chrome: Chrome,
    t: Catalog,
    household_name: String,
    admin_name: String,
    currencies: Vec<CurrencyChoice>,
    languages: Vec<Choice>,
    themes: Vec<Swatch>,
    date_pattern: &'static str,
    db_path: String,
    receipts_root: String,
    backup_dir: String,
    /// Per-field, plus the card-level summary the template builds from the
    /// same value. One `FieldErrors` rather than a message written twice.
    errors: FieldErrors,
}

#[derive(Template)]
#[template(path = "auth/dead.html")]
struct DeadPage {
    chrome: Chrome,
    t: Catalog,
    title: String,
    body: String,
    /// A finished setup is information, not a warning — the household is fine,
    /// this person simply wants the other door.
    note: bool,
}

pub async fn setup_page(State(st): State<AppState>, jar: CookieJar, parts: Parts) -> Response {
    if !auth::is_fresh(&st.pool).await.unwrap_or(false) {
        let (chrome, t) = chrome(&st, &jar, &parts, Where::Login, "/setup").await;
        return Html(DeadPage {
            title: t.setup_done_title().to_string(),
            body: t.setup_done_body().to_string(),
            note: true,
            chrome,
            t,
        })
        .into_response();
    }
    let (chrome, t) = chrome(&st, &jar, &parts, Where::Setup, "/setup").await;
    Html(setup_form(&st, chrome, t, None, None, FieldErrors::new())).into_response()
}

fn setup_form(
    st: &AppState,
    chrome: Chrome,
    t: Catalog,
    household_name: Option<String>,
    admin_name: Option<String>,
    errors: FieldErrors,
) -> SetupPage {
    let lang = Lang::parse(chrome.lang_code).unwrap_or(Lang::En);
    SetupPage {
        household_name: household_name.unwrap_or_default(),
        admin_name: admin_name.unwrap_or_default(),
        currencies: currencies(&t, Currency::Brl),
        languages: languages(lang),
        themes: swatches(ThemeName::default()),
        date_pattern: fmt::DATE_PATTERN,
        // The operator's three keys, resolved and read-only. Shown so the admin
        // can confirm the vault is where they think it is before a single
        // receipt is filed — without creating a second place to set it.
        db_path: st.cfg.db_path.display().to_string(),
        receipts_root: st.cfg.receipts_root.display().to_string(),
        backup_dir: st.cfg.backup_dir.display().to_string(),
        errors,
        chrome,
        t,
    }
}

#[derive(Deserialize)]
pub struct SetupForm {
    household: String,
    currency: String,
    language: String,
    name: String,
    password: String,
    confirm: String,
    theme: String,
    nu: String,
}

pub async fn setup(
    State(st): State<AppState>,
    jar: CookieJar,
    parts: Parts,
    Form(form): Form<SetupForm>,
) -> Response {
    let (chrome, t) = chrome(&st, &jar, &parts, Where::Setup, "/setup").await;

    let household = form.household.trim();
    let name = form.name.trim();

    // Every field is checked, not just the first that fails: a form that
    // rejects one box per round trip makes somebody submit four times to find
    // out four things. The order is the order they sit on the page, so the
    // summary reads top to bottom and the cursor lands on the first one.
    let mut errors = FieldErrors::new();
    if household.is_empty() {
        errors.add("household", t.field_required(t.setup_household_name()));
    }
    if name.is_empty() {
        errors.add("name", t.field_required(t.login_name()));
    }
    if form.password.chars().count() < MIN_PASSWORD {
        errors.add("password", t.password_too_short(MIN_PASSWORD as i64));
    } else if form.password != form.confirm {
        // On CONFIRM, not on password: the two are a pair, and the box you
        // would retype is the second one. Only checked once the password is
        // long enough, or a short password says two things about itself.
        errors.add("confirm", t.password_mismatch());
    }
    if errors.is_bad() {
        return Html(setup_form(
            &st,
            chrome,
            t,
            Some(household.to_string()),
            Some(name.to_string()),
            errors,
        ))
        .into_response();
    }

    let lang = Lang::parse(&form.language).unwrap_or(Lang::En);
    let theme = ThemeName::parse(&form.theme).unwrap_or_default();
    let currency = Currency::parse(&form.currency).unwrap_or(Currency::Brl);
    let label = auth::device_label(session::user_agent(&parts));

    match auth::setup(
        &st.pool,
        auth::NewHousehold {
            household_name: household,
            currency: currency.code(),
            language: lang.code(),
            admin_name: name,
            password: &form.password,
            theme: theme.key(),
            line_numbers: &form.nu,
            device_label: &label,
        },
    )
    .await
    {
        Ok(sid) => {
            // Lands on the ledger, signed in. It never says "now sign in with
            // the password you just typed".
            let jar = jar
                .add(session::session_cookie(sid))
                .add(session::pref_cookie(session::LANG, lang.code().to_string()))
                .add(session::pref_cookie(
                    session::PALETTE,
                    theme.key().to_string(),
                ));
            (jar, Redirect::to("/")).into_response()
        }
        // A failure from the database has no field to point at — it is about
        // the submission as a whole — so it goes in the summary alone, under
        // a name no input carries.
        Err(e) => Html(setup_form(
            &st,
            chrome,
            t,
            Some(household.to_string()),
            Some(name.to_string()),
            FieldErrors::new().with("", e.to_string()),
        ))
        .into_response(),
    }
}

// ---------------------------------------------------------------------------
// /invite/:token  — and the reset that shares its mechanism
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "auth/invite.html")]
struct InvitePage {
    chrome: Chrome,
    t: Catalog,
    action: String,
    is_reset: bool,
    inviter: String,
    expires: String,
    name: String,
    languages: Vec<Choice>,
    themes: Vec<Swatch>,
    /// The one failure with a second line of its own: name-taken needs to
    /// explain *why* names are unique, which does not fit under a field.
    note: Option<String>,
    errors: FieldErrors,
}

pub async fn invite_page(
    State(st): State<AppState>,
    jar: CookieJar,
    parts: Parts,
    Path(token): Path<String>,
) -> Response {
    let (chrome, t) = chrome(
        &st,
        &jar,
        &parts,
        Where::Invite,
        &format!("/invite/{token}"),
    )
    .await;
    let link = auth::read_link(&st.pool, &token)
        .await
        .unwrap_or(Link::Unknown);
    // The picker opens on the household's palette for new members — what
    // the newcomer gets unless they choose otherwise.
    let palette = crate::db::settings::member_defaults(&st.pool)
        .await
        .map(|d| d.theme)
        .unwrap_or_default();
    render_link(
        chrome,
        t,
        &token,
        link,
        FieldErrors::new(),
        None,
        String::new(),
        palette,
    )
}

/// `palette` is the swatch the picker opens on: the household's default on a
/// first visit, and what was picked when a refused form comes back.
#[allow(clippy::too_many_arguments)]
fn render_link(
    chrome: Chrome,
    t: Catalog,
    token: &str,
    link: Link,
    errors: FieldErrors,
    note: Option<String>,
    name: String,
    palette: ThemeName,
) -> Response {
    let lang = Lang::parse(chrome.lang_code).unwrap_or(Lang::En);
    match link {
        Link::Invite {
            inviter, expires, ..
        } => Html(InvitePage {
            action: format!("/invite/{token}"),
            is_reset: false,
            inviter,
            expires: iso_to_date(&expires),
            name,
            languages: languages(lang),
            themes: swatches(palette),
            note,
            errors,
            chrome,
            t,
        })
        .into_response(),
        Link::Reset { expires, .. } => Html(InvitePage {
            action: format!("/invite/{token}"),
            is_reset: true,
            inviter: String::new(),
            expires: iso_to_date(&expires),
            name,
            languages: languages(lang),
            themes: Vec::new(),
            note,
            errors,
            chrome,
            t,
        })
        .into_response(),
        // A used, expired or unknown token renders a page with NO FORM: a
        // disabled form invites retrying something that cannot work.
        Link::Used => dead(chrome, t.invite_used_title(), t.invite_used_body(), t),
        Link::Expired { on } => {
            let title = t.invite_expired_title(&fmt_date(&on));
            let body = t.invite_expired_body().to_string();
            dead_owned(chrome, title, body, t)
        }
        Link::Unknown => dead(chrome, t.invite_unknown_title(), t.invite_unknown_body(), t),
    }
}

fn dead(chrome: Chrome, title: &str, body: &str, t: Catalog) -> Response {
    dead_owned(chrome, title.to_string(), body.to_string(), t)
}

fn dead_owned(chrome: Chrome, title: String, body: String, t: Catalog) -> Response {
    Html(DeadPage {
        chrome,
        t,
        title,
        body,
        note: false,
    })
    .into_response()
}

fn iso_to_date(iso: &str) -> String {
    fmt_date(iso)
}

/// An ISO timestamp from the database, shown the one way this app writes a
/// date. Never localized, never reformatted per language.
fn fmt_date(iso: &str) -> String {
    chrono::NaiveDate::parse_from_str(&iso[..iso.len().min(10)], "%Y-%m-%d")
        .map(fmt::date)
        .unwrap_or_else(|_| iso.to_string())
}

#[derive(Deserialize)]
pub struct AcceptForm {
    #[serde(default)]
    name: String,
    password: String,
    confirm: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    theme: String,
}

pub async fn accept(
    State(st): State<AppState>,
    jar: CookieJar,
    parts: Parts,
    Path(token): Path<String>,
    Form(form): Form<AcceptForm>,
) -> Response {
    let (chrome, t) = chrome(
        &st,
        &jar,
        &parts,
        Where::Invite,
        &format!("/invite/{token}"),
    )
    .await;
    let link = auth::read_link(&st.pool, &token)
        .await
        .unwrap_or(Link::Unknown);
    let is_reset = matches!(link, Link::Reset { .. });
    let name = form.name.trim().to_string();

    let mut errors = FieldErrors::new();
    // A reset asks for password and confirm only — no name, because renaming
    // is a vault move once files exist and this person is not new.
    if !is_reset && name.is_empty() {
        errors.add("name", t.field_required(t.login_name()));
    }
    if form.password.chars().count() < MIN_PASSWORD {
        errors.add("password", t.password_too_short(MIN_PASSWORD as i64));
    } else if form.password != form.confirm {
        errors.add("confirm", t.password_mismatch());
    }
    let theme = ThemeName::parse(&form.theme).unwrap_or_default();
    if errors.is_bad() {
        return render_link(chrome, t, &token, link, errors, None, name, theme);
    }

    let device = auth::device_label(session::user_agent(&parts));
    let lang = Lang::parse(&form.language).unwrap_or(Lang::En);

    let result = match &link {
        Link::Invite { role, .. } => {
            auth::accept_invite(
                &st.pool,
                &token,
                &name,
                &form.password,
                lang.code(),
                theme.key(),
                *role,
                &device,
            )
            .await
        }
        Link::Reset { .. } => auth::accept_reset(&st.pool, &token, &form.password, &device).await,
        _ => Err(AcceptError::Gone),
    };

    match result {
        Ok(sid) => {
            let jar = jar.add(session::session_cookie(sid));
            let jar = if is_reset {
                jar
            } else {
                jar.add(session::pref_cookie(session::LANG, lang.code().to_string()))
                    .add(session::pref_cookie(
                        session::PALETTE,
                        theme.key().to_string(),
                    ))
            };
            (jar, Redirect::to("/")).into_response()
        }
        // Reachable, because members.name is UNIQUE — so it needs real copy,
        // and everything typed is preserved except the passwords.
        Err(AcceptError::NameTaken) => render_link(
            chrome,
            t,
            &token,
            link,
            FieldErrors::new().with("name", t.name_taken()),
            Some(t.name_taken_body().to_string()),
            name,
            theme,
        ),
        Err(_) => {
            // The link was consumed or revoked between rendering and
            // submitting. Say that, rather than re-offering a form that cannot
            // succeed.
            let title = t.invite_used_title().to_string();
            let body = t.invite_used_body().to_string();
            dead_owned(chrome, title, body, t)
        }
    }
}

// ---------------------------------------------------------------------------
// preferences that are not a member's: language and light/dark
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PrefForm {
    #[serde(default)]
    lang: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    next: String,
}

/// Sets `ffm.lang` and redirects, so the next page's first byte is already in
/// the right language. Works with JS disabled and with HTMX absent.
pub async fn set_lang(jar: CookieJar, Form(form): Form<PrefForm>) -> Response {
    let lang = Lang::parse(&form.lang).unwrap_or(Lang::En);
    let jar = jar.add(session::pref_cookie(session::LANG, lang.code().to_string()));
    (jar, Redirect::to(&safe_next(&form.next))).into_response()
}

pub async fn set_theme(jar: CookieJar, Form(form): Form<PrefForm>) -> Response {
    let mode = ModePref::parse(&form.mode);
    let jar = jar.add(session::pref_cookie(
        session::THEME,
        mode.code().to_string(),
    ));
    (jar, Redirect::to(&safe_next(&form.next))).into_response()
}

/// The word for a light/dark state, in the viewer's language. Shared with the
/// ledger bar's own control — the ids are `auth-theme-*` only because that is
/// where the first one lived; they name the mode, not the page.
pub fn mode_label(t: &Catalog, mode: ModePref) -> &'static str {
    match mode {
        ModePref::System => t.auth_theme_system(),
        ModePref::Light => t.auth_theme_light(),
        ModePref::Dark => t.auth_theme_dark(),
    }
}

/// A redirect target that came in on a form is attacker-controlled, so only a
/// path on this server is ever honored. `//evil.example` is a protocol-relative
/// URL and is rejected along with everything else that is not a local path.
fn safe_next(next: &str) -> String {
    if next.starts_with('/') && !next.starts_with("//") {
        next.to_string()
    } else {
        "/".to_string()
    }
}

/// Toggles preview-as-non-admin for this session. Reversible any time, nothing
/// destructive, so no confirmation — and a non-admin asking for it is simply
/// already seeing what it shows.
pub async fn toggle_preview(State(st): State<AppState>, auth: Auth) -> Response {
    let on = !auth.session.preview;
    let _ = auth::set_preview(&st.pool, &auth.session.id, on && auth.session.role_admin).await;
    Redirect::to("/").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redirect_target_from_a_form_must_be_local() {
        assert_eq!(safe_next("/setup"), "/setup");
        assert_eq!(safe_next("/?period=2026-09"), "/?period=2026-09");
        // Protocol-relative: a real open redirect if this were allowed.
        assert_eq!(safe_next("//evil.example/x"), "/");
        assert_eq!(safe_next("https://evil.example"), "/");
        assert_eq!(safe_next(""), "/");
    }

    #[test]
    fn a_date_is_written_the_one_way_this_app_writes_dates() {
        assert_eq!(fmt_date("2026-09-27T10:00:00Z"), "27/09/2026");
        assert_eq!(fmt_date("nonsense"), "nonsense");
    }
}
