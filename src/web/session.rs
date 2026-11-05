//! Cookies, and the extractor every authenticated handler asks for.
//!
//! Five cookies, and which is which matters:
//!
//! * **`ffm.sid`** — the session. Opaque, `HttpOnly`, `SameSite=Lax`. It
//!   carries a random id and nothing else; the member it belongs to is a row.
//!   400 days, renewed as the row slides — never a browser-session cookie,
//!   which an installed iPhone app drops every time iOS closes it.
//! * **`ffm.lang`** — the pre-login language. A cookie rather than
//!   `localStorage` because the server renders the page: storage is readable
//!   only after JS runs, which leaves a flash of the wrong words or a
//!   re-translated DOM. A cookie arrives *with the request*, so the first byte
//!   is already right, with zero JS. `HttpOnly`, since only the server reads it.
//! * **`ffm.theme`** — light / dark / system. A property of the *device*, not
//!   of the member, which is exactly what lets `/login` honor it without
//!   knowing who you are.
//! * **`ffm.palette`** — the palette of whoever last signed in on this device,
//!   so the sign-in page after `:logout` looks like the app you just left. A
//!   cookie for `ffm.lang`'s reason: in `localStorage` it would reach the page
//!   only after a script ran, as a flash of `ffm` and a repaint. Written from
//!   `member_settings` at sign-in, on a palette change and at sign-out; read
//!   only before sign-in, where there is no member row to ask.
//! * **`ffm.hide`** — whether money is pixelated (`gh`, the statusline eye).
//!   Who might be looking at a screen is a fact about the device, like
//!   light/dark, not about the member. Rendered as `data-hide` on `<html>`, so
//!   the first frame is already hidden — the reason it is not `localStorage`.

use crate::db::auth::{self, Session};
use crate::db::settings::{self, MemberSettings};
use crate::domain::viewer::Viewer;
use crate::state::AppState;
use crate::ui::i18n::{Catalog, Lang};
use crate::ui::theme::{ModePref, ThemeName};
use axum::extract::{FromRequestParts, Request};
use axum::http::HeaderValue;
use axum::http::header::{ACCEPT_LANGUAGE, SET_COOKIE, USER_AGENT};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use std::sync::{Arc, OnceLock};

pub const SID: &str = "ffm.sid";
pub const LANG: &str = "ffm.lang";
pub const THEME: &str = "ffm.theme";
pub const PALETTE: &str = "ffm.palette";
pub const HIDE: &str = "ffm.hide";

/// A year. Language and light/dark are preferences, not secrets with a
/// lifetime.
const PREF_MAX_AGE: time::Duration = time::Duration::days(365);

fn base<'c>(name: &'static str, value: String) -> Cookie<'c> {
    let mut c = Cookie::new(name, value);
    c.set_path("/");
    c.set_http_only(true);
    c.set_same_site(SameSite::Lax);
    c
}

/// 400 days, renewed whenever the session slides — so in practice, never
/// reached by anybody who opens the ledger once a year. 400 is Chrome's
/// ceiling: a longer `Max-Age` is cut to it anyway.
const SESSION_MAX_AGE: time::Duration = time::Duration::days(400);

/// The session cookie, with a lifetime of its own.
///
/// It had none, on the reasoning that the row decides expiry and a cookie
/// lifetime would be a second answer. But a cookie with no `Max-Age` is not
/// "no expiry": it is a browser-session cookie, gone when the browser quits —
/// and on an iPhone, the installed app loses it every time iOS closes the app,
/// so opening the ledger from the home screen meant signing in again. The row
/// still decides whether a session is alive; the cookie's lifetime is only how
/// long the device keeps the key, and `renew` slides it with the row.
pub fn session_cookie<'c>(id: String) -> Cookie<'c> {
    let mut c = base(SID, id);
    c.set_max_age(SESSION_MAX_AGE);
    c
}

/// Carries "this request's session slid" from the `Auth` extractor out to
/// `renew`, which is the one that can see the response. Request extensions
/// do not travel back out, so the two share this handle.
#[derive(Clone, Default)]
pub struct Renewal(Arc<OnceLock<String>>);

/// Re-sends the session cookie on a response whose request slid the session
/// (at most once a minute), so its 400 days count from the last visit. A
/// cookie set before the cookie had a lifetime is upgraded the same way, on
/// its next slide.
///
/// Never over a response that already sets `ffm.sid` — a sign-out clearing it,
/// or a sign-in replacing it, must have the last word.
pub async fn renew(mut req: Request, next: Next) -> Response {
    let renewal = Renewal::default();
    req.extensions_mut().insert(renewal.clone());
    let mut res = next.run(req).await;
    if let Some(id) = renewal.0.get()
        && !sets_session(&res)
        && let Ok(v) = HeaderValue::from_str(&session_cookie(id.clone()).to_string())
    {
        res.headers_mut().append(SET_COOKIE, v);
    }
    res
}

fn sets_session(res: &Response) -> bool {
    let prefix = format!("{SID}=");
    res.headers()
        .get_all(SET_COOKIE)
        .iter()
        .any(|v| v.to_str().is_ok_and(|s| s.starts_with(&prefix)))
}

pub fn clear_session<'c>() -> Cookie<'c> {
    let mut c = base(SID, String::new());
    c.set_max_age(time::Duration::ZERO);
    c
}

pub fn pref_cookie<'c>(name: &'static str, value: String) -> Cookie<'c> {
    let mut c = base(name, value);
    c.set_max_age(PREF_MAX_AGE);
    c
}

/// Light/dark from the device's cookie. `system` is the default and means
/// exactly that: no `data-theme` attribute, so the media query decides.
pub fn mode_of(jar: &CookieJar) -> ModePref {
    jar.get(THEME)
        .map(|c| ModePref::parse(c.value()))
        .unwrap_or(ModePref::System)
}

/// Whether this device has money hidden. Only `1` means yes: no cookie, or
/// anything else in it, shows the figures.
pub fn hide_of(jar: &CookieJar) -> bool {
    jar.get(HIDE).is_some_and(|c| c.value() == "1")
}

/// The pre-auth palette. Anything the cookie does not name — no cookie, a
/// palette since removed, a hand-edited value — is the bundled `ffm`.
pub fn palette_of(jar: &CookieJar) -> ThemeName {
    jar.get(PALETTE)
        .and_then(|c| ThemeName::parse(c.value()))
        .unwrap_or_default()
}

/// Pre-login language resolution, in order: the cookie, then `Accept-Language`
/// matched against what we actually ship, then the household's default, then
/// English.
pub fn pre_auth_lang(jar: &CookieJar, parts: &Parts, household_default: Option<Lang>) -> Lang {
    if let Some(l) = jar.get(LANG).and_then(|c| Lang::parse(c.value())) {
        return l;
    }
    if let Some(l) = parts
        .headers
        .get(ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .and_then(best_accept_language)
    {
        return l;
    }
    household_default.unwrap_or(Lang::En)
}

/// `pt-BR;q=0.9, en;q=0.8` → the best supported match. Quality values are
/// honored, because a browser that lists English first at q=0.5 is stating a
/// preference, not an order.
pub fn best_accept_language(header: &str) -> Option<Lang> {
    let mut best: Option<(f32, Lang)> = None;
    for part in header.split(',') {
        let mut bits = part.split(';');
        let tag = bits.next()?.trim();
        let q = bits
            .find_map(|b| {
                b.trim()
                    .strip_prefix("q=")
                    .and_then(|v| v.parse::<f32>().ok())
            })
            .unwrap_or(1.0);

        // `pt-BR` matches exactly; a bare `pt` still means Portuguese, and
        // `en-GB` still means English. Anything else is not a language we ship.
        let lang = Lang::parse(tag).or_else(|| {
            let primary = tag.split('-').next()?.to_ascii_lowercase();
            match primary.as_str() {
                "pt" => Some(Lang::PtBr),
                "en" => Some(Lang::En),
                _ => None,
            }
        })?;
        if best.is_none_or(|(bq, _)| q > bq) {
            best = Some((q, lang));
        }
    }
    best.map(|(_, l)| l)
}

pub fn user_agent(parts: &Parts) -> Option<&str> {
    parts.headers.get(USER_AGENT).and_then(|v| v.to_str().ok())
}

// ---------------------------------------------------------------------------
// the authenticated extractor
// ---------------------------------------------------------------------------

/// Everything an authenticated page needs about who is asking.
///
/// A handler that takes this cannot be reached without a session, which is the
/// point: authentication is a type, not a line of code at the top of each
/// handler that somebody eventually omits.
#[derive(Clone, Debug)]
pub struct Auth {
    pub session: Session,
    pub viewer: Viewer,
    pub settings: MemberSettings,
    pub mode: ModePref,
    /// `ffm.hide` — money pixelated on this device.
    pub hide: bool,
}

impl Auth {
    /// The catalog in **this member's** language. After login the member's row
    /// is authoritative, full stop — the cookie is never read again.
    pub fn catalog(&self) -> Catalog {
        Catalog::new(self.settings.language)
    }
}

/// No session: go to `/login`. The one exception is a database with no members
/// at all, which means nobody has run `/setup` yet — sending them to a sign-in
/// page for an account that cannot exist is a dead end.
pub struct NotSignedIn(bool);

impl IntoResponse for NotSignedIn {
    fn into_response(self) -> Response {
        Redirect::to(if self.0 { "/setup" } else { "/login" }).into_response()
    }
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = NotSignedIn;

    async fn from_request_parts(parts: &mut Parts, st: &AppState) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let fresh = || async { auth::is_fresh(&st.pool).await.unwrap_or(false) };

        let Some(sid) = jar.get(SID).map(|c| c.value().to_string()) else {
            return Err(NotSignedIn(fresh().await));
        };
        let Ok(Some(session)) = auth::session(&st.pool, &sid).await else {
            // A cookie naming a session that no longer exists — signed out on
            // another device, or a reset revoked it. Same answer as no cookie.
            return Err(NotSignedIn(fresh().await));
        };

        if session.slid
            && let Some(r) = parts.extensions.get::<Renewal>()
        {
            let _ = r.0.set(session.id.clone());
        }

        let settings = settings::member_settings(&st.pool, session.member.id)
            .await
            .unwrap_or_default();

        Ok(Auth {
            viewer: session.viewer(),
            session,
            settings,
            mode: mode_of(&jar),
            hide: hide_of(&jar),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_language_honors_quality_not_order() {
        assert_eq!(
            best_accept_language("pt-BR,pt;q=0.9,en;q=0.8"),
            Some(Lang::PtBr)
        );
        // Listed first, wanted least.
        assert_eq!(
            best_accept_language("en;q=0.5,pt-BR;q=0.9"),
            Some(Lang::PtBr)
        );
        // A bare primary tag still names a language we ship.
        assert_eq!(best_accept_language("pt"), Some(Lang::PtBr));
        assert_eq!(best_accept_language("en-GB"), Some(Lang::En));
        assert_eq!(best_accept_language("de,fr;q=0.8"), None);
        assert_eq!(best_accept_language(""), None);
    }

    #[test]
    fn the_session_cookie_is_httponly_lax_and_outlives_the_browser() {
        let c = session_cookie("abc".into());
        assert_eq!(c.http_only(), Some(true));
        assert_eq!(c.same_site(), Some(SameSite::Lax));
        assert_eq!(c.path(), Some("/"));
        // Without a Max-Age it is a browser-session cookie, and an installed
        // iPhone app loses it every time iOS closes the app.
        assert_eq!(c.max_age(), Some(time::Duration::days(400)));
    }

    /// One request through `renew`, its handler setting the renewal (as `Auth`
    /// does when the session slid) and answering with `answer`'s cookies.
    async fn through_renew(slid: bool, answer: Option<Cookie<'static>>) -> Vec<String> {
        use axum::routing::get;
        use tower::ServiceExt;

        let handler = move |req: Request| async move {
            if slid {
                let _ = req
                    .extensions()
                    .get::<Renewal>()
                    .unwrap()
                    .0
                    .set("abc".into());
            }
            match answer {
                Some(c) => (CookieJar::new().add(c), "ok").into_response(),
                None => "ok".into_response(),
            }
        };
        let app = axum::Router::new()
            .route("/", get(handler))
            .layer(axum::middleware::from_fn(renew));
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        res.headers()
            .get_all(SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn a_slide_renews_the_cookie_and_a_quiet_request_does_not() {
        let set = through_renew(true, None).await;
        assert_eq!(set.len(), 1);
        assert!(set[0].starts_with("ffm.sid=abc"), "{set:?}");
        assert!(set[0].contains("Max-Age=34560000"), "{set:?}");

        assert!(through_renew(false, None).await.is_empty());
    }

    #[tokio::test]
    async fn a_renewal_never_undoes_a_sign_out_in_the_same_response() {
        let set = through_renew(true, Some(clear_session())).await;
        assert_eq!(set.len(), 1, "{set:?}");
        assert!(set[0].contains("Max-Age=0"), "{set:?}");

        // Another cookie is no reason to hold the renewal back.
        let set = through_renew(true, Some(pref_cookie(HIDE, "1".into()))).await;
        assert_eq!(set.len(), 2, "{set:?}");
    }

    #[test]
    fn the_palette_cookie_falls_back_to_ffm() {
        let jar = CookieJar::new();
        assert_eq!(palette_of(&jar), ThemeName::default());
        let jar = jar.add(pref_cookie(PALETTE, "nonsense".into()));
        assert_eq!(palette_of(&jar), ThemeName::default());
        for name in ThemeName::ALL {
            let jar = CookieJar::new().add(pref_cookie(PALETTE, name.key().into()));
            assert_eq!(palette_of(&jar), name);
        }
    }

    #[test]
    fn only_a_one_hides_the_money() {
        assert!(!hide_of(&CookieJar::new()));
        assert!(hide_of(
            &CookieJar::new().add(pref_cookie(HIDE, "1".into()))
        ));
        for v in ["0", "", "yes", "true"] {
            let jar = CookieJar::new().add(pref_cookie(HIDE, v.into()));
            assert!(!hide_of(&jar), "{v:?} must show the figures");
        }
    }

    #[test]
    fn clearing_the_session_actually_expires_it() {
        assert_eq!(clear_session().max_age(), Some(time::Duration::ZERO));
    }
}
