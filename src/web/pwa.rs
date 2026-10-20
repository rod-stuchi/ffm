//! `GET /manifest.webmanifest` — what lets a phone install the ledger as an
//! app: Android's "Install app", and the name and full-screen launch iOS's
//! "Add to Home Screen" gives it.
//!
//! **Public, outside every guard.** A browser fetches the manifest without
//! cookies (unless the link asks for them), so behind a session it would fail
//! — and `/login` is the page somebody installs from on a new phone. It says
//! nothing `/login` does not already show: the household's name.
//!
//! **No service worker.** Installing no longer needs one, and the only thing
//! it could add is an offline copy of a page — a stale copy of the truth, in
//! an app whose database is the single source of it.
//!
//! **Its colors are the bundled `ffm` palette's**, for the splash screen and
//! the title bar until the page loads. The manifest cannot see a member's
//! palette or the device's light/dark, so each page carries its own
//! `theme-color` (`RenderedTheme::meta_tags`), which wins once it is up.

use crate::db::ledger;
use crate::state::AppState;
use crate::ui::theme::{self, Mode, ThemeName, Token};
use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};

pub async fn manifest(State(st): State<AppState>) -> Response {
    let house = ledger::household(&st.pool).await.ok().flatten();
    // The name under the icon. A household names itself on `/setup`; before
    // that there is nothing to install yet, but the route still answers.
    let name = house
        .as_ref()
        .map(|h| h.name.clone())
        .unwrap_or_else(|| "FFM".into());
    let lang = house.as_ref().map_or("en", |h| h.default_language.code());
    let ffm = theme::palette(ThemeName::Ffm, Mode::Light);

    let body = serde_json::json!({
        "id": "/",
        "name": name,
        "short_name": name,
        "lang": lang,
        "start_url": "/",
        "scope": "/",
        "display": "standalone",
        "background_color": ffm.get(Token::Paper),
        "theme_color": ffm.get(Token::Surface),
        "icons": [
            { "src": "/static/icons/icon-192.png", "sizes": "192x192", "type": "image/png" },
            { "src": "/static/icons/icon-512.png", "sizes": "512x512", "type": "image/png" },
            {
                "src": "/static/icons/icon-maskable-512.png",
                "sizes": "512x512",
                "type": "image/png",
                "purpose": "maskable"
            }
        ]
    });
    (
        [(header::CONTENT_TYPE, "application/manifest+json")],
        body.to_string(),
    )
        .into_response()
}
