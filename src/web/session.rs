//! Cookies, and the extractor every authenticated handler asks for.
//!
//! Three cookies, and which is which matters:
//!
//! * **`ffm.sid`** — the session. Opaque, `HttpOnly`, `SameSite=Lax`. It
//!   carries a random id and nothing else; the member it belongs to is a row.
//! * **`ffm.lang`** — the pre-login language. A cookie rather than
//!   `localStorage` because the server renders the page: storage is readable
//!   only after JS runs, which leaves a flash of the wrong words or a
//!   re-translated DOM. A cookie arrives *with the request*, so the first byte
//!   is already right, with zero JS. `HttpOnly`, since only the server reads it.
//! * **`ffm.theme`** — light / dark / system. A property of the *device*, not
//!   of the member, which is exactly what lets `/login` honour it without
//!   knowing who you are.

use crate::db::auth::{self, Session};
use crate::db::settings::{self, MemberSettings};
use crate::domain::viewer::Viewer;
use crate::state::AppState;
use crate::ui::i18n::{Catalog, Lang};
use crate::ui::theme::ModePref;
use axum::extract::FromRequestParts;
use axum::http::header::{ACCEPT_LANGUAGE, USER_AGENT};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};

pub const SID: &str = "ffm.sid";
pub const LANG: &str = "ffm.lang";
pub const THEME: &str = "ffm.theme";

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

/// The session cookie. No `Max-Age`: sessions are long-lived and sliding
/// server-side, and the row is what expiry actually means — a cookie lifetime
/// would be a second, disagreeing answer.
pub fn session_cookie<'c>(id: String) -> Cookie<'c> {
    base(SID, id)
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
/// honoured, because a browser that lists English first at q=0.5 is stating a
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

        let settings = settings::member_settings(&st.pool, session.member.id)
            .await
            .unwrap_or_default();

        Ok(Auth {
            viewer: session.viewer(),
            session,
            settings,
            mode: mode_of(&jar),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_language_honours_quality_not_order() {
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
    fn the_session_cookie_is_httponly_lax_and_has_no_lifetime_of_its_own() {
        let c = session_cookie("abc".into());
        assert_eq!(c.http_only(), Some(true));
        assert_eq!(c.same_site(), Some(SameSite::Lax));
        assert_eq!(c.path(), Some("/"));
        // Expiry is the sessions row, not the cookie: two answers would
        // eventually disagree, and the row is the one that can be revoked.
        assert!(c.max_age().is_none());
    }

    #[test]
    fn clearing_the_session_actually_expires_it() {
        assert_eq!(clear_session().max_age(), Some(time::Duration::ZERO));
    }
}
