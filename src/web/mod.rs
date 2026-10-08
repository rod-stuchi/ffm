//! HTTP. Routing, and the one place a template becomes a response.

pub mod auth;
pub mod commands;
pub mod csrf;
#[cfg(feature = "dev-reload")]
pub mod dev_reload;
pub mod fidget;
pub mod files;
pub mod form;
pub mod import;
pub mod ledger;
pub mod live;
pub mod log;
pub mod months;
pub mod order;
pub mod prepare;
pub mod prune;
pub mod rules;
pub mod selection;
pub mod session;
pub mod settings;
pub mod template;
pub mod trash;
pub mod tx;

#[cfg(debug_assertions)]
pub mod sandbox;

use crate::state::AppState;
use axum::Router;
use axum::routing::{get, post};
use tower_http::services::ServeDir;

pub fn router(state: AppState) -> Router {
    // The three ways in, plus the two preferences that belong to a device
    // rather than to a member. All plain form POSTs, and all deliberately
    // OUTSIDE the same-origin guard: they are submitted from a page that may
    // have been opened cold, and a blanket check rejects every one of them and
    // makes the app impossible to enter. `SameSite=Lax` is what protects them,
    // which is sound here — a forged login POST gains an attacker nothing,
    // `/lang` has no security value, and setup and invite-accept are
    // single-use by construction.
    let open = Router::new()
        .route("/login", get(auth::login_page).post(auth::login))
        .route("/setup", get(auth::setup_page).post(auth::setup))
        .route("/invite/{token}", get(auth::invite_page).post(auth::accept))
        .route("/lang", post(auth::set_lang))
        .route("/theme", post(auth::set_theme));

    // The mutations that can move a month in or out of "overdue" — the dot on
    // the month picker's trigger. Listed once, here, and marked by one layer
    // (`live::mark_dot`) so the page re-fetches the dot after any of them;
    // no handler has to remember to say so.
    let overdue = Router::new()
        .route("/tx/{id}/delete", post(tx::delete))
        .route("/undo", post(tx::undo))
        .route("/redo", post(tx::redo))
        .route("/trash/restore", post(trash::restore))
        .route("/tx/{id}", get(tx::one_row).post(tx::save_edit))
        .route("/panes/{member}/{period}", get(tx::pane_get))
        .route("/tx/{id}/privacy", post(tx::privacy))
        .route("/tx/{id}/member", post(tx::reattribute))
        // Files. Three distinct actions rather than one upload plus a
        // question: each kind has its own fixed name on disk and its own
        // effect on state.
        .route("/tx/{id}/file/{kind}", get(files::open).post(files::upload))
        .route("/tx/{id}/file/{kind}/delete", post(files::delete))
        .route("/tx/{id}/file/{kind}/prepare", post(prepare::stage_stored))
        .route(
            "/tx/{id}/file/{kind}/prepare/barcode",
            post(prepare::stage_barcode),
        )
        .route(
            "/tx/{id}/file/{kind}/prepare/append",
            post(prepare::stage_append),
        )
        .route("/prepare", post(prepare::stage_upload))
        .route("/prepare/{token}/cancel", post(prepare::cancel))
        .route("/rules", post(rules::create))
        .route("/rules/{id}", post(rules::save))
        .route("/rules/{id}/delete", post(rules::delete))
        .route("/members/{id}/disable", post(settings::disable))
        .route("/members/{id}/enable", post(settings::enable))
        .route_layer(axum::middleware::from_fn(live::mark_dot));

    // Authenticated, state-changing: these carry the same-origin check.
    let guarded = Router::new()
        .route("/logout", post(auth::logout))
        .route("/preview", post(auth::toggle_preview))
        // Every mutation in the app goes through one response shape: a row
        // fragment, or an error row carrying Try Again. See web::tx.
        .route("/tx", post(tx::add))
        // `Ctrl-u` / `:trash` — every deleted row and removed file, each one
        // restorable on its own rather than in `u`'s order.
        .route("/trash", get(trash::view))
        // `:prune` — admin only, and the one place a file leaves the disk:
        // only a copy in the trash of a file that stays. Not in `overdue`: a
        // trashed copy never decides whether a month is overdue.
        .route("/prune", get(prune::view).post(prune::run))
        .route("/tx/{id}/move", post(tx::move_row))
        // `:order` / `gs` — this pane's manual order, copied to other months.
        // Not in `overdue`: an order decides nothing about what is late.
        .route("/order", get(order::view))
        .route("/order/months", get(order::preview))
        .route("/order/copy", post(order::copy))
        // `e` — the row opens into its own form, in place.
        .route("/tx/{id}/edit", get(tx::edit_form))
        .route("/tx/{id}/paid", post(tx::paid))
        // The statusline's selection cell - `x`, summed like the pane header.
        .route("/tx/selection", get(selection::cell))
        // The `P` carousel's stage, and a PDF's pages as images — see
        // `files::FileView` for why the browser's PDF viewer is not used.
        .route("/tx/{id}/file/{kind}/view", get(files::view))
        .route("/tx/{id}/file/{kind}/page/{n}", get(files::page))
        .route("/prepare/{token}/page/{n}", get(prepare::page))
        .route("/prepare/{token}/size", get(prepare::size))
        .route("/prepare/{token}/file", get(prepare::file))
        // `gl` — read-only, and the same column the rule editor uses.
        .route("/tx/{id}/log", get(log::log))
        // `:grep` — every month, which is what makes it a different verb.
        .route("/grep", get(log::grep))
        // The month picker's numbers — fetched when it opens, not on every
        // ledger load. See `ledger::month_pop`.
        .route("/months", get(ledger::month_pop))
        .route("/months/dot", get(ledger::month_dot))
        .route("/live", get(live::stream))
        // The rule pane. A / I open it blank, E on a recurring row opens it
        // filled in; saving reconciles immediately so the periods the rule is
        // responsible for arrive in the same response.
        .route("/rules/new", get(rules::new_rule))
        // The months read-back. A pure GET: twelve numbers of arithmetic,
        // rendered by the same parser that stores the value.
        .route("/rules/months", get(rules::months_preview))
        .route("/rules/{id}/edit", get(rules::edit_rule))
        // The ONE place a barcode file is ever attached: the rule's own
        // editor, where the rule is already identified.
        .route("/rules/{id}/barcode", post(import::attach))
        .route("/rules/{id}/unlink", post(import::unlink))
        // `A`'s dropzone: the rule does not exist yet, so the bill is read
        // and parked, and Create attaches it.
        .route("/rules/barcode/preview", post(import::preview))
        // A bill dropped anywhere on the page: recognized by a rule's
        // EXISTING binding, never bound here — see `import::find`.
        .route("/bills/find", post(import::find))
        .route("/rules/{id}/meta", get(import::meta))
        // The settings surfaces. Vsplit panes with no Save button: every
        // control writes on `change`, so there is never a pending second copy
        // of the truth.
        .route("/profile", get(settings::profile))
        .route("/household", get(settings::household))
        .route("/members", get(settings::members_view))
        .route("/settings/member", post(settings::set_member))
        .route(
            "/settings/member/household-defaults",
            post(settings::adopt_household_defaults),
        )
        .route("/settings/household", post(settings::set_household))
        .route("/members/invite", post(settings::invite))
        .route("/members/{id}/reset", post(settings::reset))
        .route(
            "/members/me/signout-others",
            post(settings::sign_out_others),
        )
        .route("/members/{id}/signout", post(settings::sign_out))
        .route("/members/{id}/trash", post(settings::trash))
        .route("/members/{id}/restore", post(settings::restore))
        .route("/members/invites/{id}/revoke", post(settings::revoke))
        .merge(overdue)
        .layer(axum::middleware::from_fn(csrf::guard));

    let app = Router::new()
        .route("/", get(ledger::ledger))
        .merge(open)
        .merge(guarded)
        .nest_service("/static", static_files());

    // Gated at the module AND at the mount. None of the sandbox exists in a
    // release binary, so there is no auth guard to forget.
    #[cfg(debug_assertions)]
    let app = app.merge(sandbox::router());

    // `just dev`'s reload stream. Outside every guard: the pre-auth pages and
    // the sandbox reload too.
    #[cfg(feature = "dev-reload")]
    let app = app.route("/_dev/reload", get(dev_reload::stream));

    app.with_state(state)
}

/// `/static`. Under `just dev` / `just dev-lan` (the `dev-reload` feature)
/// every response says `Cache-Control: no-cache`, so the browser revalidates
/// on each load and a rebuilt `build.css` shows up on a plain reload. Without
/// it Safari kept serving a stale stylesheet from its heuristic cache, and a
/// CSS fix looked like it had not landed. A production build leaves caching
/// alone: there, the files only change on a deploy.
fn static_files() -> Router {
    let files = Router::new().fallback_service(ServeDir::new("static"));
    #[cfg(feature = "dev-reload")]
    let files = files.layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache"),
    ));
    files
}
