//! The trim-and-shrink view: every PDF stops here before it is filed, and a
//! stored one can be brought back to it from `P`. So does a screenshot or a
//! photo — one page, nothing to trim, only the shrink (`services::imgshrink`).
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
use crate::services::{imgshrink, pdftrim, stage};
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
    /// A PNG or a JPEG: one page, no strip, and a shrink that needs no `gs`.
    image: bool,
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

/// `POST /tx/{id}/file/{kind}/prepare` — a stored PDF or image, from `P`.
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
    if !matches!(ext.as_str(), "pdf" | "png" | "jpg") {
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
    let image = imgshrink::kind(&upload.filename, &upload.bytes);
    let meta = stage::Meta {
        // Staged for the viewer: it is their view, whoever's row it is for.
        member_id: auth.viewer.member_id,
        filename: upload.filename.clone(),
        pages: 1,
        image,
        spans: ctx.spans,
    };
    let Ok(token) = stage::park(&meta, &upload.bytes) else {
        return crate::web::tx::refuse(t.error_not_written(), true);
    };
    // An image is one page; only a PDF has its pages counted.
    let pages = if image.is_some() {
        1
    } else {
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
        pages
    };

    Html(PrepareView {
        t: *t,
        token,
        filename: upload.filename,
        pages: (1..=pages).collect(),
        gs: pdftrim::gs_available(),
        spans: ctx.spans,
        image: image.is_some(),
        stored,
        row: ctx.row.clone(),
    })
    .into_response()
}

#[derive(serde::Deserialize)]
pub struct PageParams {
    #[serde(default)]
    big: Option<u8>,
    /// An image's large view: the shrunk copy (`1`) or the original.
    #[serde(default)]
    shrink: Option<u8>,
}

/// `GET /prepare/{token}/page/{n}` — a page of the staged original, small
/// for the strip and larger (`?big=1`) for the focused page.
///
/// An image is its own page. Its large view shows the copy that would be
/// filed when Shrink is on — the shrink is lossy, so what you see should be
/// what goes to the vault.
pub async fn page(
    auth: Auth,
    Path((token, n)): Path<(String, u32)>,
    Query(p): Query<PageParams>,
) -> Response {
    let member = auth.viewer.member_id;
    let Some(path) = stage::original(&token, member) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if n == 0 {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Some(kind) = stage::meta(&token, member).and_then(|m| m.image) {
        let shrink = p.shrink == Some(1);
        let shown = tokio::task::spawn_blocking(move || {
            stage::result(&token, member, None, shrink)
                .ok()
                .and_then(|o| std::fs::read(o.path).ok())
        })
        .await;
        return match shown {
            Ok(Some(bytes)) => (
                [
                    (header::CONTENT_TYPE, kind.mime()),
                    (header::CACHE_CONTROL, "private, max-age=86400"),
                ],
                bytes,
            )
                .into_response(),
            _ => StatusCode::NOT_FOUND.into_response(),
        };
    }
    let dpi = if p.big == Some(1) { "150" } else { "36" };
    let out =
        tokio::task::spawn_blocking(move || crate::services::pdfpage::render(&path, n, dpi)).await;
    match out {
        // A JPEG, as the carousel's pages are (`services::pdfpage`). The
        // staged file never changes under its single-use token, so a page
        // flipped back to comes from the browser's cache.
        Ok(Some(jpeg)) => (
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "private, max-age=86400"),
            ],
            jpeg,
        )
            .into_response(),
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

/// `GET /prepare/{token}/file` — the finished PDF (or image), which the page
/// sends on through the upload it was headed for.
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
    let mime = stage::meta(&token, member)
        .and_then(|m| m.image)
        .map_or("application/pdf", imgshrink::Kind::mime);
    match tokio::fs::read(&outcome.path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `POST /prepare/{token}/cancel` — drops the staged file. Also called after
/// a Save that went through: the vault has the file now.
pub async fn cancel(auth: Auth, Path(token): Path<String>) -> Response {
    stage::discard(&token, auth.viewer.member_id);
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// P's "Add barcode" and "Append PDF": a longer PDF, into the same trim view
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct CodeForm {
    code: String,
}

/// `POST /tx/{id}/file/{kind}/prepare/barcode` — the PDF on show, with one
/// more page: the barcode drawn from a typed code, and what the code says.
///
/// Nothing is filed here. The longer file opens in the trim view, whose Save
/// sends it through the upload the file came from, so the bill's checks, the
/// supersede into the trash and `u` all hold — exactly as `t` does.
pub async fn stage_barcode(
    State(st): State<AppState>,
    auth: Auth,
    Path((tx_id, kind)): Path<(i64, String)>,
    axum::Form(form): axum::Form<CodeForm>,
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
        return crate::web::tx::refuse(t.carousel_only_pdf(), true);
    }
    // Every check digit, and the digits the standard fixes, before anything
    // is drawn: a mistyped code must never become a scannable barcode for
    // somebody else's bill.
    let barcode = match crate::services::boleto::parse(&form.code, crate::clock::today()) {
        Ok(b) => b,
        Err(e) => return crate::web::tx::refuse(&code_refusal(&t, &e), true),
    };
    let currency = crate::db::ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();
    let info = page_info(&t, &barcode, &form.code, ctx.row.as_deref(), currency);
    // The size of the page it follows, so the file stays one size.
    let (w, h) = crate::services::pdftrim::last_page_size(&path)
        .unwrap_or(crate::services::barcode_page::A4);
    let Some(page) = crate::services::barcode_page::page(&info, w, h) else {
        return crate::web::tx::refuse(t.error_not_written(), true);
    };
    appended(&auth, &t, &path, page, &ctx).await
}

/// `POST /tx/{id}/file/{kind}/prepare/append` — the PDF on show with another
/// PDF's pages after it, into the trim view. A locked one is opened with the
/// rule's password first, as every upload to the row is.
pub async fn stage_append(
    State(st): State<AppState>,
    auth: Auth,
    Path((tx_id, kind)): Path<(i64, String)>,
    mut form: Multipart,
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
        return crate::web::tx::refuse(t.carousel_only_pdf(), true);
    }
    let mut filename = String::new();
    let mut bytes = Vec::new();
    while let Ok(Some(field)) = form.next_field().await {
        if field.name() == Some("file") {
            filename = field.file_name().unwrap_or_default().to_string();
            match field.bytes().await {
                Ok(b) if b.len() <= MAX_BYTES => bytes = b.to_vec(),
                Ok(_) => return crate::web::tx::refuse(t.file_too_big(), true),
                Err(_) => return crate::web::tx::refuse(t.error_not_written(), true),
            }
        }
    }
    if !bytes.starts_with(b"%PDF") {
        return crate::web::tx::refuse(t.carousel_append_not_pdf(), true);
    }
    let upload = crate::web::import::Upload {
        filename,
        bytes,
        ..Default::default()
    };
    let upload = match crate::web::import::unlock_for(
        &t,
        upload,
        ctx.password.as_deref(),
        &crate::web::fidget::Fidget::off(),
    )
    .await
    {
        Ok(u) => u,
        Err(crate::web::import::Refused(m)) => return crate::web::tx::refuse(&m, true),
    };
    appended(&auth, &t, &path, upload.bytes, &ctx).await
}

/// `stored` with `extra`'s pages after it, staged for the trim view under the
/// stored file's own name.
async fn appended(
    auth: &Auth,
    t: &Catalog,
    stored: &std::path::Path,
    extra: Vec<u8>,
    ctx: &Context,
) -> Response {
    let dir = std::env::temp_dir().join(format!("ffm-append-{:016x}", rand::random::<u64>()));
    let base = stored.to_path_buf();
    let merged = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let extra_path = dir.join("extra.pdf");
        let out = dir.join("out.pdf");
        std::fs::write(&extra_path, &extra).map_err(|e| e.to_string())?;
        let done = crate::services::pdftrim::append(&base, &extra_path, &out)
            .and_then(|()| std::fs::read(&out).map_err(|e| e.to_string()));
        let _ = std::fs::remove_dir_all(&dir);
        done
    })
    .await;
    let bytes = match merged {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => {
            tracing::warn!("append: {e}");
            return crate::web::tx::refuse(t.file_pdf_unreadable(), true);
        }
        Err(_) => return crate::web::tx::refuse(t.error_not_written(), true),
    };
    let filename = stored
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file.pdf".into());
    stage_bytes(auth, t, filename, bytes, ctx, true).await
}

/// Why a typed code was refused, in the member's language.
fn code_refusal(t: &Catalog, e: &crate::services::boleto::Error) -> String {
    use crate::services::boleto::Error;
    match e {
        Error::Length(n) => t.barcode_code_length(*n as i64),
        Error::CheckDigit { at } => t.barcode_code_digit(at),
        Error::NotDigits
        | Error::UnknownValueKind(_)
        | Error::Currency(_)
        | Error::NotArrecadacao => t.barcode_code_not().to_string(),
    }
}

/// What the generated page says: the code's kind, what it pays, to whom and
/// when — every value formatted the way the ledger formats it.
fn page_info(
    t: &Catalog,
    b: &crate::services::boleto::Barcode,
    typed: &str,
    row: Option<&str>,
    currency: crate::domain::money::Currency,
) -> crate::services::barcode_page::PageInfo {
    use crate::services::boleto::{Amount, Kind};
    let mut details = Vec::new();
    details.push((
        t.barcode_page_value().to_string(),
        match b.amount {
            Amount::Fixed(c) => fmt::money(c, currency),
            Amount::Open => t.barcode_page_open().to_string(),
            Amount::Reference(n) => t.barcode_page_reference(&n.to_string()),
        },
    ));
    if let Some(c) = &b.company {
        details.push((t.barcode_page_company().to_string(), c.clone()));
    }
    if b.kind == Kind::Arrecadacao {
        let segment = b
            .raw
            .chars()
            .nth(1)
            .and_then(|c| c.to_digit(10))
            .unwrap_or(0);
        details.push((
            t.barcode_page_segment().to_string(),
            segment_name(t, segment),
        ));
    }
    if let Some(bank) = &b.bank {
        details.push((t.barcode_page_bank().to_string(), bank.clone()));
    }
    if let Some(d) = b.due.date() {
        details.push((t.barcode_page_due().to_string(), fmt::date(d)));
    }
    crate::services::barcode_page::PageInfo {
        heading: match b.kind {
            Kind::Arrecadacao => t.barcode_page_heading_consumo().to_string(),
            Kind::Cobranca => t.barcode_page_heading_cobranca().to_string(),
        },
        details,
        typed: grouped(typed),
        barcode: b.raw.clone(),
        footer: t.barcode_page_footer(row.unwrap_or(""), &fmt::date(crate::clock::today())),
    }
}

/// FEBRABAN's segments, position 2 of a utility payment code.
fn segment_name(t: &Catalog, n: u32) -> String {
    match n {
        1 => t.barcode_segment_1(),
        2 => t.barcode_segment_2(),
        3 => t.barcode_segment_3(),
        4 => t.barcode_segment_4(),
        5 => t.barcode_segment_5(),
        6 => t.barcode_segment_6(),
        7 => t.barcode_segment_7(),
        _ => t.barcode_segment_other(),
    }
    .to_string()
}

/// The code as a person types it, in the groups a bill prints: four blocks
/// of `11-1` for 48 digits, the five fields of a bank slip for 47. Anything
/// else is shown as it came.
fn grouped(typed: &str) -> String {
    let d: String = typed.chars().filter(char::is_ascii_digit).collect();
    match d.len() {
        48 => d
            .as_bytes()
            .chunks(12)
            .map(|c| {
                let s = std::str::from_utf8(c).unwrap_or_default();
                format!("{}-{}", &s[..11], &s[11..])
            })
            .collect::<Vec<_>>()
            .join(" "),
        47 => format!(
            "{}.{} {}.{} {}.{} {} {}",
            &d[0..5],
            &d[5..10],
            &d[10..15],
            &d[15..21],
            &d[21..26],
            &d[26..32],
            &d[32..33],
            &d[33..47]
        ),
        _ => d,
    }
}

#[cfg(test)]
mod barcode_tests {
    use super::grouped;

    #[test]
    fn a_code_is_shown_in_the_groups_a_bill_prints() {
        assert_eq!(
            grouped("826600000002053100240018000976822015102026200034"),
            "82660000000-2 05310024001-8 00097682201-5 10202620003-4"
        );
        assert_eq!(
            grouped("34191.79001 01043.510047 91020.150008 1 15700000034362"),
            "34191.79001 01043.510047 91020.150008 1 15700000034362"
        );
    }
}
