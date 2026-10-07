//! Uploading, opening and deleting a tracked occurrence's files.
//!
//! Scoped to **recurring-rule occurrences only**: a one-off has no taxonomy to
//! file itself under, so it gets no attachment feature at all.
//!
//! Three distinct actions rather than one upload plus a question — each kind
//! has its own fixed name on disk and its own effect on state, and asking
//! afterwards would be asking about something already decided.

use crate::db::changes::Actor;
use crate::db::ledger;
use crate::domain::period::Period;
use crate::domain::receipt_path::Kind;
use crate::services::receipts::{self, Slot};
use crate::state::AppState;
use crate::web::session::Auth;
use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

/// At most 20 MB. A scanned bill is a few hundred kilobytes; this is the
/// bound that keeps a mistake from filling the vault's disk, not a target.
const MAX_BYTES: usize = 20 * 1024 * 1024;

/// Gathers everything the path depends on for one occurrence, and refuses
/// anything this viewer has no business touching. The path itself is
/// `receipts::slot_of`'s — the one place it is decided — so an upload lands
/// exactly where the ledger looks.
pub(crate) async fn slot_for(
    st: &AppState,
    auth: &Auth,
    tx_id: i64,
) -> Option<(Slot, Period, i64)> {
    let row = sqlx::query!(
        r#"SELECT member_id AS "member_id!: i64", visibility AS "visibility!: String"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        tx_id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()??;

    // A private row you cannot see is a file you cannot touch — otherwise its
    // absence from your ledger would be contradicted by its disappearance
    // from somebody else's.
    if row.visibility == "private" && !auth.viewer.sees_private_of(row.member_id) {
        return None;
    }
    // Untracked, or a one-off: nothing to file under — `slot_of` says so.
    let (slot, member_id) = receipts::slot_of(&st.pool, tx_id).await.ok()??;
    let period = slot.period;
    Some((slot, period, member_id))
}

/// After a write that can move where a row's files belong — a rule renamed, a
/// row given to another member, an amount in a filename that changed — its
/// files follow (`receipts::refile_row`). A failure is logged rather than
/// failing the request: the write itself is committed and correct, and the
/// next start re-files whatever is still out of place.
pub(crate) async fn refile(st: &AppState, tx_id: i64) -> receipts::Refiled {
    receipts::refile_row(&st.pool, &st.cfg.receipts_root, tx_id)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("files of row {tx_id} were not re-filed: {e}");
            receipts::Refiled::default()
        })
}

/// [`refile`] for every occurrence of a rule — after its save.
pub(crate) async fn refile_rule(st: &AppState, rule_id: i64) -> receipts::Refiled {
    receipts::refile_rule(&st.pool, &st.cfg.receipts_root, rule_id)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("files of rule {rule_id} were not re-filed: {e}");
            receipts::Refiled::default()
        })
}

/// What a re-filing adds to a statusline message: how many files moved, and
/// any that could not because something already sat where they belong.
pub(crate) fn refiled_note(t: &crate::ui::i18n::Catalog, r: &receipts::Refiled) -> Option<String> {
    let mut parts = Vec::new();
    if !r.moved.is_empty() {
        parts.push(t.files_refiled(r.moved.len() as i64));
    }
    if let Some(p) = r.blocked.first() {
        parts.push(t.files_refile_blocked(p));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// `POST /tx/{id}/file/{kind}` — add barcode / add scheduled / add receipt.
pub async fn upload(
    State(st): State<AppState>,
    auth: Auth,
    headers: axum::http::HeaderMap,
    Path((tx_id, kind)): Path<(i64, String)>,
    mut form: Multipart,
) -> Response {
    let t = auth.catalog();
    let Some(kind) = Kind::parse(&kind) else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    let Some((slot, period, member_id)) = slot_for(&st, &auth, tx_id).await else {
        return crate::web::tx::say_only(t.file_not_tracked(), true);
    };
    if let Err(refused) = crate::web::tx::may_write(&st, &auth, member_id).await {
        return refused;
    }
    let password = rule_pdf_password(&st, tx_id).await;
    let fidget = crate::web::fidget::Fidget::from(&headers, &st.live, auth.viewer.member_id);

    let mut filename = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    // What the person answered a bill's questions — only a bill asks.
    let mut answers = crate::web::import::Answers::default();
    // The trim view's Shrink — done as the file is stored, never before it
    // is read (`import::filed_bytes`).
    let mut shrink = crate::web::import::ShrinkAsk::default();
    while let Ok(Some(field)) = form.next_field().await {
        if let Some(name @ ("attach_anyway" | "recipe")) = field.name() {
            let name = name.to_string();
            let value = field.text().await.unwrap_or_default();
            crate::web::import::Answers::read(&name, &value, &mut answers);
            continue;
        }
        if let Some(name @ ("shrink" | "stage" | "keep")) = field.name() {
            let name = name.to_string();
            let value = field.text().await.unwrap_or_default();
            shrink.read(&name, &value);
            continue;
        }
        if field.name() != Some("file") {
            continue;
        }
        filename = field.file_name().unwrap_or_default().to_string();
        match field.bytes().await {
            Ok(b) if b.len() <= MAX_BYTES => bytes = b.to_vec(),
            Ok(_) => return crate::web::tx::say_only(t.file_too_big(), true),
            Err(_) => return crate::web::tx::say_only(t.error_not_written(), true),
        }
    }
    if bytes.is_empty() || filename.is_empty() {
        return crate::web::tx::say_only(t.error_not_written(), true);
    }

    // A locked PDF is opened with the rule's password BEFORE anything else
    // sees it — the bill's barcode is read from the unlocked copy, and the
    // unlocked copy is what the vault keeps, on every slot.
    let upload = crate::web::import::Upload {
        filename,
        bytes,
        shrink,
    };
    let upload =
        match crate::web::import::unlock_for(&t, upload, password.as_deref(), &fidget).await {
            Ok(u) => u,
            Err(crate::web::import::Refused(m)) => return crate::web::tx::say_only(&m, true),
        };

    // A bill is not just a file: it is checked against what the rule is bound
    // to, through the same attach the rule pane's dropzone uses — see
    // `import::attach_on_row`. Scheduled and receipt stay plain files.
    if kind == Kind::Barcode {
        return crate::web::import::attach_on_row(&st, &auth, tx_id, upload, answers, &fidget)
            .await;
    }

    let bytes = crate::web::import::filed_bytes(&upload, auth.viewer.member_id, &t, &fidget).await;
    let filename = upload.filename;
    fidget.step(t.progress_file());
    match receipts::store(
        &st.pool,
        &st.cfg.receipts_root,
        Actor::User(auth.viewer.member_id),
        &slot,
        kind,
        &filename,
        &bytes,
    )
    .await
    {
        // The row is re-read through the pane query, so the status mark it
        // comes back with was decided by the disk — not by the fact that an
        // upload just succeeded.
        Ok(_) => {
            crate::web::tx::row_fragment(&st, &auth, tx_id, member_id, period, t.file_added()).await
        }
        Err(receipts::StoreError::Locked) => crate::web::tx::say_only(t.file_pdf_locked(), true),
        Err(e) => crate::web::tx::say_only(&e.to_string(), true),
    }
}

/// The password a row's rule opens its PDFs with, if it has one.
pub(crate) async fn rule_pdf_password(st: &AppState, tx_id: i64) -> Option<String> {
    sqlx::query_scalar!(
        r#"SELECT r.pdf_password AS "pdf_password: String"
           FROM transactions t JOIN recurring_rules r ON r.id = t.rule_id
           WHERE t.id = ?1"#,
        tx_id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten()
    .flatten()
}

/// `?in=edit` — the request came from the `e` form's file strip.
#[derive(serde::Deserialize)]
pub struct DeleteQuery {
    #[serde(rename = "in")]
    from: Option<String>,
}

/// `POST /tx/{id}/file/{kind}/delete` — a soft delete: the file moves into the
/// member's trash, `{slug}/.delete/`, never off disk.
pub async fn delete(
    State(st): State<AppState>,
    auth: Auth,
    Path((tx_id, kind)): Path<(i64, String)>,
    Query(q): Query<DeleteQuery>,
) -> Response {
    let t = auth.catalog();
    let Some(kind) = Kind::parse(&kind) else {
        return refuse(t.error_not_written(), true);
    };
    let Some((slot, period, member_id)) = slot_for(&st, &auth, tx_id).await else {
        return refuse(t.file_not_tracked(), true);
    };
    if let Err(refused) = crate::web::tx::may_write(&st, &auth, member_id).await {
        return refused;
    }

    match receipts::soft_delete(
        &st.pool,
        &st.cfg.receipts_root,
        Actor::User(auth.viewer.member_id),
        &slot,
        kind,
        "manual",
        None,
    )
    .await
    {
        // From inside the `e` form, only the form's file strip is answered:
        // swapping the row would throw away the draft being typed.
        Ok(true) if q.from.as_deref() == Some("edit") => {
            crate::web::tx::edit_files_answer(&st, &auth, tx_id, t.file_removed()).await
        }
        Ok(true) => {
            crate::web::tx::row_fragment(&st, &auth, tx_id, member_id, period, t.file_removed())
                .await
        }
        Ok(false) => refuse(
            match kind {
                Kind::Barcode => t.file_none_bill(),
                Kind::Scheduled => t.file_none_scheduled(),
                Kind::Receipt => t.file_none_receipt(),
            },
            true,
        ),
        Err(e) => refuse(&e.to_string(), true),
    }
}

/// A refusal with nothing to swap — see `tx::refuse`.
fn refuse(message: &str, bad: bool) -> Response {
    crate::web::tx::refuse(message, bad)
}

/// Where one of a row's files is on disk, and its extension — or `None` when
/// the viewer may not see it or it is not there.
///
/// The path is computed from the same builder the store writes with, so no
/// handler that serves a file takes a user-supplied path, and there is
/// nothing to traverse out of.
pub(crate) async fn on_disk(
    st: &AppState,
    auth: &Auth,
    tx_id: i64,
    kind: &str,
) -> Option<(std::path::PathBuf, String)> {
    let kind = Kind::parse(kind)?;
    let (slot, _, _) = slot_for(st, auth, tx_id).await?;
    let present = receipts::presence(&st.pool, &st.cfg.receipts_root, &slot)
        .await
        .ok()?;
    let ext = match kind {
        Kind::Barcode => present.bill,
        Kind::Scheduled => present.scheduled,
        Kind::Receipt => present.receipt,
    }?;
    Some((st.cfg.receipts_root.join(slot.path(kind, &ext)), ext))
}

/// `GET /tx/{id}/file/{kind}` — the file itself, in a new browser tab.
///
/// All four allowed extensions render natively, which is exactly why no custom
/// renderer is built: this hands the browser the bytes and gets out of the way.
pub async fn open(
    State(st): State<AppState>,
    auth: Auth,
    Path((tx_id, kind)): Path<(i64, String)>,
) -> Response {
    let Some((path, ext)) = on_disk(&st, &auth, tx_id, &kind).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, content_type(&ext)),
                // `inline`, so the browser renders it rather than downloading
                // it — the whole point of keeping the extension set closed.
                (header::CONTENT_DISPOSITION, "inline"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

// ---------------------------------------------------------------------------
// the `P` carousel's stage
// ---------------------------------------------------------------------------

/// What the carousel's stage shows for one file.
///
/// **Images of the pages, not the browser's PDF viewer.** The first version
/// embedded the viewer in a frame, and the carousel's `+`/`-` could not drive
/// it: CSS zoom on a frame scales the viewer's toolbar while the page shrinks
/// to compensate; a new `#zoom=` needs a fresh frame, which Chrome drew blank
/// or ignored; and widening the frame only zooms while the viewer happens to
/// be fitted to the width — otherwise `+` just slid the page sideways. A
/// frame is somebody else's program, and it answers to its own controls.
///
/// So a PDF's pages are drawn by `pdftoppm` — poppler, which the importer
/// already needs for `pdftotext` — and every file type becomes the same
/// thing on screen: images the carousel sizes itself, so zoom means one
/// thing everywhere and works in every browser. "Open in new tab" still hands
/// over the real file.
///
/// **The browser's viewer is still offered — beside the images, by choice.**
/// An image has the resolution it was drawn at, and a PDF does not: the tab
/// stays sharp at any size and lets text be selected. So `v` (or the corner's
/// *PDF* button) swaps the pages for an `<embed>` of the file, zooming
/// itself with its own controls. The images stay the default, because only
/// they answer the carousel's zoom and keys the same way in every browser;
/// the viewer, once clicked into, takes the keyboard.
#[derive(Template)]
#[template(path = "partials/file_view.html")]
pub struct FileView {
    t: crate::ui::i18n::Catalog,
    /// `/tx/{id}/file/{kind}` — the file, and the root its pages hang off.
    base: String,
    /// A PDF's page numbers, `1..=n`. Empty for anything else.
    pages: Vec<i64>,
    /// The page a shared bill owes this month (`spans_period`), scrolled to
    /// on open. 0 = none.
    owed: i64,
    /// jpg / png: the file itself is the one image.
    image: bool,
    /// txt: the text, escaped by the template like any other content.
    text: Option<String>,
    /// A PDF handed whole to the browser's own viewer (`?pdf=1`), opened on
    /// `owed` when there is one.
    embed: bool,
    /// The file's size, `412 KB` / `1,2 MB` — shown at the left of the
    /// carousel's bottom bar. Empty when the file could not be read.
    size: String,
}

#[derive(serde::Deserialize)]
pub struct ViewParams {
    #[serde(default, deserialize_with = "crate::web::form::blank_as_none")]
    page: Option<i64>,
    /// `1`: the browser's PDF viewer instead of the page images.
    #[serde(default, deserialize_with = "crate::web::form::blank_as_none")]
    pdf: Option<i64>,
}

/// `GET /tx/{id}/file/{kind}/view` — the stage, as a fragment.
pub async fn view(
    State(st): State<AppState>,
    auth: Auth,
    Path((tx_id, kind)): Path<(i64, String)>,
    axum::extract::Query(p): axum::extract::Query<ViewParams>,
) -> Response {
    let t = auth.catalog();
    let Some((path, ext)) = on_disk(&st, &auth, tx_id, &kind).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let base = format!("/tx/{tx_id}/file/{kind}");
    // In the household's numbers, like every other size this app shows.
    let currency = ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();
    let size = tokio::fs::metadata(&path)
        .await
        .map(|m| crate::ui::fmt::bytes(m.len(), currency))
        .unwrap_or_default();
    let mut view = FileView {
        t,
        base,
        pages: Vec::new(),
        owed: 0,
        image: false,
        text: None,
        embed: false,
        size,
    };
    match ext.as_str() {
        // The viewer draws every page itself; only the owed one is needed,
        // for `#page=`.
        "pdf" if p.pdf == Some(1) => {
            view.embed = true;
            view.owed = p.page.filter(|p| *p > 0).unwrap_or(0);
        }
        "pdf" => {
            // A PDF poppler cannot count is still a file: one page is tried,
            // and a page that will not draw says so in its own place.
            let n = page_count(&path).await.unwrap_or(1).max(1);
            view.pages = (1..=n).collect();
            view.owed = p.page.filter(|p| (1..=n).contains(p)).unwrap_or(0);
        }
        "txt" => view.text = tokio::fs::read_to_string(&path).await.ok(),
        _ => view.image = true,
    }
    crate::web::template::Html(view).into_response()
}

/// How finely a PDF page is drawn for the carousel.
///
/// 300 dpi: an A4 page comes out ~2480 px wide, and a statement drawn small in
/// PDF points (the Inter one is 446 pt wide, ~1860 px here) still fills the
/// stage without being stretched. It was 150, which left that statement at
/// 929 px and stretched it 2–4× — blurry beside the same file in a tab. A page
/// is ~2.5× the bytes and the time (~1 MB, ~0.8 s for an A4 bill), which a
/// household's LAN does not notice.
const PAGE_DPI: &str = "300";

/// `GET /tx/{id}/file/{kind}/page/{n}` — one page of a PDF, as a PNG, at
/// [`PAGE_DPI`].
pub async fn page(
    State(st): State<AppState>,
    auth: Auth,
    Path((tx_id, kind, n)): Path<(i64, String, u32)>,
) -> Response {
    let Some((path, ext)) = on_disk(&st, &auth, tx_id, &kind).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if ext != "pdf" || n == 0 {
        return StatusCode::NOT_FOUND.into_response();
    }
    // A subprocess, like the importer's `pdftotext`; off the async runtime,
    // since it blocks for the few hundred milliseconds a page takes.
    let out = tokio::task::spawn_blocking(move || {
        let n = n.to_string();
        std::process::Command::new("pdftoppm")
            .args(["-f", &n, "-l", &n, "-r", PAGE_DPI, "-png", "-singlefile"])
            .arg(&path)
            .output()
    })
    .await;
    match out {
        Ok(Ok(o)) if o.status.success() && !o.stdout.is_empty() => (
            [
                (header::CONTENT_TYPE, "image/png"),
                // A replaced bill keeps its path, so the page must not be
                // served from cache after the file under it changed.
                (header::CACHE_CONTROL, "no-cache"),
            ],
            o.stdout,
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `pdfinfo`'s page count.
pub(crate) async fn page_count(path: &std::path::Path) -> Option<i64> {
    let path = path.to_path_buf();
    let out = tokio::task::spawn_blocking(move || {
        std::process::Command::new("pdfinfo").arg(path).output()
    })
    .await
    .ok()?
    .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("Pages:"))
        .and_then(|n| n.trim().parse().ok())
}

fn content_type(ext: &str) -> &'static str {
    match ext {
        "pdf" => "application/pdf",
        "jpg" => "image/jpeg",
        "png" => "image/png",
        _ => "text/plain; charset=utf-8",
    }
}
