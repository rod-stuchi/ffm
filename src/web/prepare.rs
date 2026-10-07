//! The trim-and-shrink view: every PDF stops here before it is filed, and a
//! stored one can be brought back to it from `P`.
//!
//! **Save files nothing here.** The page fetches the finished bytes
//! (`GET /prepare/{token}/file`) and sends them through the very upload the
//! file was on its way to — a row's slot, or `A`/`E`'s dropzone — so every
//! check a file already gets runs on what is actually filed: the bill's
//! binding and duplicate check, the amount read off it, `superseded` for a
//! replaced file, the change log. A second copy of any of that, here, would be
//! one more place for the rules to drift apart.
//!
//! What this module does decide is who may stage what (the same answers the
//! uploads give), and what the selection comes to — computed on the server,
//! for the reason the months read-back is: a size worked out in the browser
//! would be a guess about a program that runs here.

use crate::db::rules;
use crate::services::{pdftrim, stage};
use crate::state::AppState;
use crate::ui::fmt;
use crate::ui::i18n::Catalog;
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

/// At most 20 MB — the uploads' own bound.
const MAX_BYTES: usize = 20 * 1024 * 1024;

#[derive(Template)]
#[template(path = "partials/prepare.html")]
pub struct PrepareView {
    t: Catalog,
    token: String,
    filename: String,
    pages: Vec<u32>,
    gs: bool,
    spans: bool,
    /// Reworking a file already in the vault, from `P`, rather than one on
    /// its way in. Only the heading says so.
    stored: bool,
    /// The row it is for — *Nubank · 01/2026* — when there is one. A bill
    /// dropped anywhere on the page found that row itself, so the view says
    /// which before anything is filed.
    row: Option<String>,
}

#[derive(Template)]
#[template(
    source = r#"<span class="pz-orig">{{ original }}</span> → <b class="pz-new">{{ size }}</b>{% if let Some(s) = saved %} <span class="pz-saved">{{ s }}</span>{% endif %}{% if let Some(n) = note %} <span class="pz-note">· {{ n }}</span>{% endif %}"#,
    ext = "html"
)]
struct SizeLine {
    original: String,
    size: String,
    saved: Option<String>,
    note: Option<String>,
}

/// Who and what a staged file is for, and what that decides.
struct Context {
    password: Option<String>,
    spans: bool,
    /// The row's name and month, for the view's heading.
    row: Option<String>,
}

/// `POST /prepare` — a PDF on its way to a row (`tx`) or a rule (`rule`, or
/// neither for `A`'s rule that does not exist yet). Answers the view.
pub async fn stage_upload(State(st): State<AppState>, auth: Auth, mut form: Multipart) -> Response {
    let t = auth.catalog();
    let say = |m: &str| crate::web::tx::refuse(m, true);

    let mut filename = String::new();
    let mut bytes = Vec::new();
    let (mut tx, mut rule, mut typed, mut spans) = (None, None, None, false);
    while let Ok(Some(field)) = form.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            filename = field.file_name().unwrap_or_default().to_string();
            match field.bytes().await {
                Ok(b) if b.len() <= MAX_BYTES => bytes = b.to_vec(),
                Ok(_) => return say(t.file_too_big()),
                Err(_) => return say(t.error_not_written()),
            }
            continue;
        }
        let value = field.text().await.unwrap_or_default();
        let value = value.trim();
        match name.as_str() {
            "tx" => tx = value.parse::<i64>().ok(),
            "rule" => rule = value.parse::<i64>().ok().filter(|r| *r > 0),
            "pdf_password" => typed = Some(value.to_string()).filter(|p| !p.is_empty()),
            "spans" => spans = value == "1",
            _ => {}
        }
    }
    if bytes.is_empty() || filename.is_empty() {
        return say(t.error_not_written());
    }

    let ctx = match (tx, rule) {
        (Some(tx_id), _) => match row_context(&st, &auth, tx_id).await {
            Ok(c) => c,
            Err(res) => return res,
        },
        (None, Some(rule_id)) => {
            let Ok(Some(r)) = rules::get(&st.pool, rule_id).await else {
                return say(t.error_not_written());
            };
            if let Err(refused) = crate::web::rules::rule_access(&st, &auth, &r).await {
                return refused;
            }
            Context {
                // The pane's field wins, as it does on the dropzone itself.
                password: typed.or(r.pdf_password),
                spans: r.spans_period,
                row: None,
            }
        }
        // `A`: the rule does not exist yet; what the pane says is all there is.
        (None, None) => Context {
            password: typed,
            spans,
            row: None,
        },
    };
    stage_bytes(&auth, &t, filename, bytes, &ctx, false).await
}

/// `POST /tx/{id}/file/{kind}/prepare` — a stored PDF, from `P`.
pub async fn stage_stored(
    State(st): State<AppState>,
    auth: Auth,
    Path((tx_id, kind)): Path<(i64, String)>,
) -> Response {
    let t = auth.catalog();
    let ctx = match row_context(&st, &auth, tx_id).await {
        Ok(c) => c,
        Err(res) => return res,
    };
    let Some((path, ext)) = crate::web::files::on_disk(&st, &auth, tx_id, &kind).await else {
        return crate::web::tx::refuse(t.error_not_written(), true);
    };
    if ext != "pdf" {
        return crate::web::tx::refuse(t.prepare_not_pdf(), true);
    }
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return crate::web::tx::refuse(t.error_not_written(), true);
    };
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file.pdf".into());
    stage_bytes(&auth, &t, filename, bytes, &ctx, true).await
}

/// The row a file is for: the same permission its upload asks, the rule's
/// password, and whether its bill spans several months.
async fn row_context(st: &AppState, auth: &Auth, tx_id: i64) -> Result<Context, Response> {
    let t = auth.catalog();
    let Some((_, _, member_id)) = crate::web::files::slot_for(st, auth, tx_id).await else {
        return Err(crate::web::tx::refuse(t.file_not_tracked(), true));
    };
    crate::web::tx::may_write(st, auth, member_id).await?;
    let row = sqlx::query!(
        r#"SELECT r.spans_period AS "s!: bool", t.title AS "title!: String",
                  t.period AS "period!: String"
           FROM transactions t JOIN recurring_rules r ON r.id = t.rule_id
           WHERE t.id = ?1"#,
        tx_id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten();
    Ok(Context {
        password: crate::web::files::rule_pdf_password(st, tx_id).await,
        spans: row.as_ref().is_some_and(|r| r.s),
        row: row.map(|r| {
            let month = crate::domain::period::Period::parse(&r.period)
                .map(crate::ui::fmt::period)
                .unwrap_or(r.period);
            format!("{} · {month}", r.title)
        }),
    })
}

async fn stage_bytes(
    auth: &Auth,
    t: &Catalog,
    filename: String,
    bytes: Vec<u8>,
    ctx: &Context,
    stored: bool,
) -> Response {
    // Unlocked first, with the rule's password: the pages have to be drawn,
    // and the scratch copy should never need the password again.
    let upload = crate::web::import::Upload {
        filename,
        bytes,
        ..Default::default()
    };
    let upload = match crate::web::import::unlock_for(
        t,
        upload,
        ctx.password.as_deref(),
        &crate::web::fidget::Fidget::off(),
    )
    .await
    {
        Ok(u) => u,
        Err(crate::web::import::Refused(m)) => return crate::web::tx::refuse(&m, true),
    };
    let meta = stage::Meta {
        // Staged for the viewer: it is their view, whoever's row it is for.
        member_id: auth.viewer.member_id,
        filename: upload.filename.clone(),
        pages: 1,
        spans: ctx.spans,
    };
    let Ok(token) = stage::park(&meta, &upload.bytes) else {
        return crate::web::tx::refuse(t.error_not_written(), true);
    };
    let Some(path) = stage::original(&token, auth.viewer.member_id) else {
        return crate::web::tx::refuse(t.error_not_written(), true);
    };
    let Some(n) = crate::web::files::page_count(&path)
        .await
        .filter(|n| *n > 0)
    else {
        stage::discard(&token, auth.viewer.member_id);
        return crate::web::tx::refuse(t.file_pdf_unreadable(), true);
    };
    let pages = n as u32;
    if stage::set_pages(&token, auth.viewer.member_id, pages).is_err() {
        return crate::web::tx::refuse(t.error_not_written(), true);
    }

    Html(PrepareView {
        t: *t,
        token,
        filename: upload.filename,
        pages: (1..=pages).collect(),
        gs: pdftrim::gs_available(),
        spans: ctx.spans,
        stored,
        row: ctx.row.clone(),
    })
    .into_response()
}

#[derive(serde::Deserialize)]
pub struct PageParams {
    #[serde(default)]
    big: Option<u8>,
}

/// `GET /prepare/{token}/page/{n}` — a page of the staged original, small
/// for the strip and larger (`?big=1`) for the focused page.
pub async fn page(
    auth: Auth,
    Path((token, n)): Path<(String, u32)>,
    Query(p): Query<PageParams>,
) -> Response {
    let Some(path) = stage::original(&token, auth.viewer.member_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if n == 0 {
        return StatusCode::NOT_FOUND.into_response();
    }
    let dpi = if p.big == Some(1) { "150" } else { "36" };
    let out = tokio::task::spawn_blocking(move || {
        let n = n.to_string();
        std::process::Command::new("pdftoppm")
            .args(["-f", &n, "-l", &n, "-r", dpi, "-png", "-singlefile"])
            .arg(&path)
            .output()
    })
    .await;
    match out {
        Ok(Ok(o)) if o.status.success() && !o.stdout.is_empty() => {
            ([(header::CONTENT_TYPE, "image/png")], o.stdout).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct Selection {
    /// `1,3,4` — the pages to keep. Absent means all of them.
    #[serde(default)]
    keep: Option<String>,
    #[serde(default)]
    shrink: Option<u8>,
}

impl Selection {
    fn keep(&self) -> Option<Vec<u32>> {
        self.keep.as_deref().map(|k| {
            k.split(',')
                .filter_map(|p| p.trim().parse::<u32>().ok())
                .collect()
        })
    }
}

/// `GET /prepare/{token}/size` — the size line for a selection.
pub async fn size(
    State(st): State<AppState>,
    auth: Auth,
    Path(token): Path<String>,
    Query(sel): Query<Selection>,
) -> Response {
    let t = auth.catalog();
    let currency = crate::db::ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();
    let member = auth.viewer.member_id;
    let keep = sel.keep();
    let shrink = sel.shrink == Some(1);
    let outcome =
        tokio::task::spawn_blocking(move || stage::result(&token, member, keep.as_deref(), shrink))
            .await;
    let outcome = match outcome {
        Ok(Ok(o)) => o,
        Ok(Err(why)) => {
            if why == "keep at least one page" {
                return axum::response::Html(t.prepare_keep_one().to_string()).into_response();
            }
            tracing::info!("prepare: {why}");
            return axum::response::Html(t.prepare_failed().to_string()).into_response();
        }
        Err(_) => return axum::response::Html(t.prepare_failed().to_string()).into_response(),
    };
    let saved = (outcome.size < outcome.original).then(|| {
        let pct = 100 - (outcome.size * 100 / outcome.original.max(1));
        format!("−{pct}%")
    });
    let note = match outcome.shrink {
        stage::Shrink::NoGain => Some(t.prepare_shrink_no_gain().to_string()),
        _ => None,
    };
    Html(SizeLine {
        original: fmt::bytes(outcome.original, currency),
        size: fmt::bytes(outcome.size, currency),
        saved,
        note,
    })
    .into_response()
}

/// `GET /prepare/{token}/file` — the finished PDF, which the page sends on
/// through the upload it was headed for.
pub async fn file(
    State(st): State<AppState>,
    auth: Auth,
    headers: axum::http::HeaderMap,
    Path(token): Path<String>,
    Query(sel): Query<Selection>,
) -> Response {
    let t = auth.catalog();
    let member = auth.viewer.member_id;
    let keep = sel.keep();
    let shrink = sel.shrink == Some(1);
    let tok = token.clone();
    // Each step named as it starts, in the box above Save — only the ones
    // the size check had not already done.
    let fidget = crate::web::fidget::Fidget::from(&headers, &st.live, member);
    let (trim, shrinking) = (
        t.progress_trim().to_string(),
        t.progress_shrink().to_string(),
    );
    let outcome = tokio::task::spawn_blocking(move || {
        stage::result_with(&tok, member, keep.as_deref(), shrink, &|step| {
            fidget.step(match step {
                stage::Step::Trim => trim.clone(),
                stage::Step::Shrink => shrinking.clone(),
            })
        })
    })
    .await;
    let Ok(Ok(outcome)) = outcome else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    match tokio::fs::read(&outcome.path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/pdf")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `POST /prepare/{token}/cancel` — drops the staged file. Also called after
/// a Save that went through: the vault has the file now.
pub async fn cancel(auth: Auth, Path(token): Path<String>) -> Response {
    stage::discard(&token, auth.viewer.member_id);
    StatusCode::NO_CONTENT.into_response()
}
