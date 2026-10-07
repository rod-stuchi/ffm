//! Attaching a bill to a rule, and reading what it says.
//!
//! **The only place a barcode file is ever attached is the rule's own
//! editor.** An earlier design had a panel drop parse the file and infer which
//! rule it belonged to — by issuer and period and value proximity, or by
//! matching the dropped filename against a row's title. Both were rejected as
//! too fragile: a real bill's filename, or its content, should not have to
//! line up with what is already in the ledger for the household to trust the
//! result. Here the rule is already identified, so there is no matching left
//! to get wrong.
//!
//! What the parse then writes is deliberately conservative. It **fills what is
//! empty, replaces what was only a seed, and flags what a person typed** — a
//! row's amount copied in by the materializer is a placeholder waiting for
//! this bill, but a number somebody set with `e` is never silently replaced,
//! because the one thing worse than failing to import is replacing a right
//! answer with a wrong one.

use crate::db::changes::{Actor, Change, FileAction, TxAction, Value};
use crate::db::recipes::{self, RuleRecipe};
use crate::db::rules;
use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::receipt_path::Kind as FileKind;
use crate::domain::viewer::Viewer;
use crate::services::boleto::Amount;
use crate::services::import::{
    self, Binding, Gate, Ident, ImportError, Override, Parsed, binding_check, gate,
};
use crate::services::pending;
use crate::services::receipts;
use crate::state::AppState;
use crate::ui::fmt;
use crate::ui::i18n::Catalog;
use crate::web::fidget::Fidget;
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::response::IntoResponse;
use axum::response::Response;
use chrono::NaiveDate;
use serde::Deserialize;

const MAX_BYTES: usize = 20 * 1024 * 1024;

#[derive(Deserialize)]
pub struct Params {
    /// The month the ledger is showing — a barcode file belongs to a period,
    /// and the pane knows which one is on screen.
    #[serde(default)]
    period: String,
}

/// A file as it arrived, before anything has been decided about it.
#[derive(Default)]
pub struct Upload {
    pub filename: String,
    pub bytes: Vec<u8>,
    /// Whether it is shrunk as it is filed. Every read — the barcode, the
    /// recipe, the binding, the amount, the fingerprint — is done on `bytes`
    /// as they are; the shrink is the last step before the vault
    /// ([`filed_bytes`]), because Ghostscript can re-encode a PDF's text
    /// (*é* came back *Ž*), and a bill read from that is not the same bill.
    pub shrink: ShrinkAsk,
}

/// The trim view's Shrink, as an upload carries it: on or off, and — when
/// the view staged the file — the staged token and the pages kept, so the
/// shrunk copy its size line already built is the one filed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShrinkAsk {
    pub on: bool,
    pub stage: Option<String>,
    pub keep: Option<String>,
}

impl ShrinkAsk {
    /// Reads the `shrink`, `stage` and `keep` fields, from any upload handler.
    pub fn read(&mut self, name: &str, value: &str) -> bool {
        let value = value.trim();
        match name {
            "shrink" => self.on = value == "1",
            "stage" => self.stage = Some(value.to_string()).filter(|v| !v.is_empty()),
            "keep" => self.keep = Some(value.to_string()).filter(|v| !v.is_empty()),
            _ => return false,
        }
        true
    }

    fn pages(&self) -> Option<Vec<u32>> {
        self.keep.as_deref().map(|k| {
            k.split(',')
                .filter_map(|p| p.trim().parse::<u32>().ok())
                .collect()
        })
    }
}

/// The bytes that go to the vault: the upload's own, shrunk now when that was
/// asked — **after** everything that reads the bill has read it.
///
/// The trim view's size line has normally built the shrunk copy already, so
/// it is taken from the stage — but only when the stage's unshrunk copy is
/// byte for byte what was uploaded, so a stale token can never file another
/// file. Otherwise (`A`'s Create comes after the stage was dropped) the bytes
/// are shrunk here. A shrink that is not smaller is not used, as anywhere.
pub async fn filed_bytes(upload: &Upload, member_id: i64, t: &Catalog, fidget: &Fidget) -> Vec<u8> {
    use crate::services::{pdftrim, stage};
    let ask = &upload.shrink;
    let is_pdf =
        upload.filename.to_ascii_lowercase().ends_with(".pdf") || upload.bytes.starts_with(b"%PDF");
    if !ask.on || !is_pdf || !pdftrim::gs_available() {
        return upload.bytes.clone();
    }
    let step = t.progress_shrink().to_string();
    if let Some(token) = ask.stage.clone() {
        let keep = ask.pages();
        let same = {
            let (token, keep) = (token.clone(), keep.clone());
            tokio::task::spawn_blocking(move || {
                stage::result(&token, member_id, keep.as_deref(), false)
                    .ok()
                    .and_then(|o| std::fs::read(o.path).ok())
            })
            .await
            .ok()
            .flatten()
        };
        if same.as_deref() == Some(upload.bytes.as_slice()) {
            let live = fidget.clone();
            let shrunk = tokio::task::spawn_blocking(move || {
                stage::result_with(&token, member_id, keep.as_deref(), true, &|_| {
                    live.step(step.clone())
                })
                .ok()
                .filter(|o| o.shrink == stage::Shrink::Applied)
                .and_then(|o| std::fs::read(o.path).ok())
            })
            .await
            .ok()
            .flatten();
            return shrunk.unwrap_or_else(|| upload.bytes.clone());
        }
    }
    fidget.step(t.progress_shrink());
    let bytes = upload.bytes.clone();
    tokio::task::spawn_blocking(move || pdftrim::shrink_bytes(&bytes))
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
        .unwrap_or_else(|| upload.bytes.clone())
}

/// Why an attach did not happen — always a statusline message, and always
/// "nothing was written".
pub struct Refused(pub String);

/// Why an attach stopped short of writing anything: a refusal, or a question
/// for the person — a bill that cannot be confirmed as this rule's, or that
/// contradicts what the rule is bound to. **Never a dead end**: the question
/// is answered with `y`, which re-sends the upload with `attach_anyway`.
pub enum Halt {
    Refused(String),
    Ask { message: String, mismatch: bool },
}

impl Halt {
    pub fn respond(self) -> Response {
        match self {
            Halt::Refused(m) => crate::web::tx::say_only(&m, true),
            Halt::Ask { message, mismatch } => crate::web::tx::ask(&message, mismatch),
        }
    }

    /// The words, for a caller that can only report — `A`'s Create, where the
    /// rule was just made and could not have asked anything.
    pub fn message(self) -> String {
        match self {
            Halt::Refused(m) | Halt::Ask { message: m, .. } => m,
        }
    }
}

impl From<Refused> for Halt {
    fn from(Refused(m): Refused) -> Halt {
        Halt::Refused(m)
    }
}

/// The change-log notes an attach that went ahead against the binding carries.
/// Not localized, like `receipts::MOVED`: the log records what happened.
pub const ATTACHED_UNCONFIRMED: &str = "attached anyway — not confirmed as this rule's bill";
pub const ATTACHED_MISMATCH: &str = "attached anyway — it names something else";

/// What the person answered the questions an attach can ask — carried back
/// on the re-send, every answer so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answers {
    /// `y` to "attach anyway?".
    pub anyway: bool,
    pub teach: Teach,
}

/// The answer to "teach this rule to read its bills?".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Teach {
    /// Not asked yet: the assistant may be.
    #[default]
    Ask,
    /// `y`: the parked proposal under this token becomes the rule's recipe.
    Approved(String),
    /// Anything else: attach with what the bill gave, teach nothing.
    Skip,
}

impl Answers {
    /// Reads the `attach_anyway` and `recipe` fields, from either handler.
    pub fn read(name: &str, value: &str, into: &mut Answers) -> bool {
        match name {
            "attach_anyway" => into.anyway = value == "1",
            "recipe" => {
                into.teach = match value {
                    "" => Teach::Ask,
                    "skip" => Teach::Skip,
                    token => Teach::Approved(token.to_string()),
                }
            }
            _ => return false,
        }
        true
    }
}

/// An upload as the handlers read it.
pub struct Incoming {
    pub upload: Upload,
    /// What the rule pane's PDF password field says right now.
    pub password: Option<String>,
    pub answers: Answers,
}

/// Reads the `file` field of a multipart body — and `pdf_password`, which the
/// rule pane's dropzone sends with it: what the pane's field says right now,
/// saved or not, so typing the password and then dropping the statement works
/// in one go, before Create or Save. And `attach_anyway`, the re-send after a
/// person answered `y`.
async fn read_upload(t: &Catalog, form: &mut Multipart) -> Result<Incoming, Refused> {
    let mut filename = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut password = None;
    let mut answers = Answers::default();
    let mut shrink = ShrinkAsk::default();
    while let Ok(Some(field)) = form.next_field().await {
        match field.name() {
            Some(name @ ("attach_anyway" | "recipe")) => {
                let name = name.to_string();
                let value = field.text().await.unwrap_or_default();
                Answers::read(&name, &value, &mut answers);
            }
            Some(name @ ("shrink" | "stage" | "keep")) => {
                let name = name.to_string();
                let value = field.text().await.unwrap_or_default();
                shrink.read(&name, &value);
            }
            Some("file") => {
                filename = field.file_name().unwrap_or_default().to_string();
                match field.bytes().await {
                    Ok(b) if b.len() <= MAX_BYTES => bytes = b.to_vec(),
                    Ok(_) => return Err(Refused(t.file_too_big().to_string())),
                    Err(_) => return Err(Refused(t.error_not_written().to_string())),
                }
            }
            Some("pdf_password") => {
                password = field
                    .text()
                    .await
                    .ok()
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty());
            }
            _ => {}
        }
    }
    if bytes.is_empty() {
        return Err(Refused(t.error_not_written().to_string()));
    }
    Ok(Incoming {
        upload: Upload {
            filename,
            bytes,
            shrink,
        },
        password,
        answers,
    })
}

/// Opens a password-protected PDF with the rule's password, before anything
/// reads, parks or stores it — the vault keeps the unlocked copy. Anything
/// that is not a locked PDF comes back exactly as it was.
///
/// Called on every way a file reaches a rule's row: the row's own slots, and
/// `A`'s and `E`'s dropzone. A file this password does not open is refused
/// here, so nothing downstream ever sees it.
pub async fn unlock_for(
    t: &Catalog,
    upload: Upload,
    password: Option<&str>,
    fidget: &Fidget,
) -> Result<Upload, Refused> {
    use crate::services::pdflock::{self, LockError};
    if pdflock::looks_locked(&upload.filename, &upload.bytes) {
        fidget.step(t.progress_unlock());
    }
    // `qpdf` is a child process: off the async worker, or the step just
    // sent waits behind it (see [`read_bill_off`]).
    let password = password.map(str::to_string);
    let filename = upload.filename.clone();
    let unlocked = tokio::task::spawn_blocking(move || {
        match pdflock::unlock(&upload.filename, &upload.bytes, password.as_deref()) {
            Ok(std::borrow::Cow::Borrowed(_)) => Ok(upload),
            Ok(std::borrow::Cow::Owned(bytes)) => Ok(Upload {
                filename: upload.filename,
                bytes,
                shrink: upload.shrink,
            }),
            Err(e) => Err(e),
        }
    })
    .await
    .unwrap_or_else(|e| Err(LockError::Tool(e.to_string())));
    match unlocked {
        Ok(u) => Ok(u),
        Err(LockError::NeedsPassword) => Err(Refused(t.file_pdf_locked().to_string())),
        Err(LockError::WrongPassword) => Err(Refused(t.file_pdf_wrong_password().to_string())),
        Err(LockError::NoTool) => Err(Refused(t.file_pdf_no_tool().to_string())),
        // qpdf's own reason is for the log, not the statusline.
        Err(LockError::Tool(why)) => {
            tracing::info!("could not open {filename}: {why}");
            Err(Refused(t.file_pdf_unreadable().to_string()))
        }
    }
}

/// Reads a bill. Pure: nothing lands in the vault or the database here —
/// `pdftotext` takes a path, so the bytes visit a scratch file and leave.
/// `recipe` is the rule's own, when it has one — there is no global list of
/// issuers any more.
pub fn parse(
    t: &Catalog,
    upload: &Upload,
    recipe: Option<&RuleRecipe>,
    kind: Option<&str>,
) -> Result<Parsed, Refused> {
    read_bill(upload, recipe, kind).map_err(|e| Refused(unread_reason(t, &e)))
}

/// Why a file could not be read as a bill, in words.
pub fn unread_reason(t: &Catalog, e: &ImportError) -> String {
    match e {
        ImportError::NoText => t.import_why_no_text().to_string(),
        ImportError::NoBarcode { issuer: Some(name) } => t.import_why_no_barcode_known(name),
        ImportError::NoBarcode { issuer: None } => t.import_why_no_barcode().to_string(),
        ImportError::Misread(_) => t.import_why_misread().to_string(),
        ImportError::Tool(why) => {
            // The reader's own words (pdftotext's stderr) are for the log.
            tracing::info!("unreadable bill: {why}");
            t.import_why_unreadable().to_string()
        }
    }
}

/// [`parse`], keeping why it failed.
fn read_bill(
    upload: &Upload,
    recipe: Option<&RuleRecipe>,
    kind: Option<&str>,
) -> Result<Parsed, ImportError> {
    let scratch = scratch_file(upload).map_err(|e| ImportError::Tool(e.to_string()))?;
    let profiles = recipe.map(|r| r.profiles(kind)).unwrap_or_default();
    // A misread is reported, never worked around: a number that failed its
    // own checksum is not a number to fall back from.
    import::import(&scratch.0, &profiles, crate::clock::today())
}

/// [`read_bill`] on a blocking thread. `pdftotext` is a child process, and a
/// tokio worker blocked on it also holds back the task it just woke — a
/// progress step sent to `/live` sits in that worker's own slot, out of reach
/// of the others, until the read is over. So the box above Save named each
/// step only once it had finished.
async fn read_bill_off(
    upload: &Upload,
    recipe: Option<&RuleRecipe>,
    kind: Option<&str>,
) -> Result<Parsed, ImportError> {
    let upload = Upload {
        filename: upload.filename.clone(),
        bytes: upload.bytes.clone(),
        ..Default::default()
    };
    let (recipe, kind) = (recipe.cloned(), kind.map(str::to_string));
    tokio::task::spawn_blocking(move || read_bill(&upload, recipe.as_ref(), kind.as_deref()))
        .await
        .unwrap_or_else(|e| Err(ImportError::Tool(e.to_string())))
}

/// The upload as a file in the temp dir, removed when this goes out of scope
/// — `pdftotext` takes a path. The extension is kept: `import` reads a `.txt`
/// as text and hands anything else to `pdftotext`, and it decides by the name.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn scratch_file(upload: &Upload) -> std::io::Result<Scratch> {
    let mut raw = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut raw);
    let tag: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let ext = std::path::Path::new(&upload.filename)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("pdf");
    let path = std::env::temp_dir().join(format!("ffm-import-{tag}.{ext}"));
    std::fs::write(&path, &upload.bytes)?;
    Ok(Scratch(path))
}

/// What a bill is recognized by, as the binding check reads it. A bill with
/// no barcode is taken to be the kind its issuer's profile declares.
fn read_binding(parsed: &Parsed) -> Binding<'_> {
    let barcode = parsed.barcode.as_ref();
    Binding {
        client_code: parsed.import.client_code.as_deref(),
        company: barcode.and_then(|b| b.company.as_deref()),
        kind: barcode
            .map(|b| b.kind.code())
            .or(parsed.import.issuer_kind.as_deref()),
        bank: barcode.and_then(|b| b.bank.as_deref()),
        issuer: None,
        cnpjs: &parsed.import.cnpjs,
        cards: &parsed.import.cards,
    }
}

/// What a rule's bill is bound to, as the binding check reads it.
fn rule_binding(rule: &rules::Rule) -> Binding<'_> {
    Binding {
        client_code: rule.client_code.as_deref(),
        company: rule.barcode_company.as_deref(),
        kind: rule.barcode_kind.as_deref(),
        bank: None,
        issuer: rule.issuer_id.as_deref(),
        cnpjs: &[],
        cards: rule.card_last4.as_slice(),
    }
}

/// One side of a disagreement, in words: "company 0162", "bank 077",
/// "Nu Pagamentos S.A.", "CNPJ 00.416.968/0001-01", "card ending 7766". An
/// issuer is named when the rule's recipe knows its name, else shown by CNPJ.
fn ident_label(t: &Catalog, recipe: Option<&RuleRecipe>, id: &Ident) -> String {
    match id {
        Ident::Account(c) => t.import_id_account(c),
        Ident::Company(c) => t.import_id_company(c),
        Ident::Bank(c) => t.import_id_bank(c),
        Ident::Kind(k) => kind_label(t, Some(k)).unwrap_or_else(|| k.clone()),
        Ident::Issuer(i) => recipe
            .filter(|r| r.cnpj.as_deref() == Some(i.as_str()))
            .and_then(|r| r.issuer_name.clone())
            .unwrap_or_else(|| t.import_id_cnpj(&fmt_cnpj(i))),
        Ident::Card(d) => t.import_id_card(d),
    }
}

/// `18.236.120/0001-58` from its 14 digits.
pub fn fmt_cnpj(d: &str) -> String {
    if d.len() != 14 || !d.chars().all(|c| c.is_ascii_digit()) {
        return d.to_string();
    }
    format!(
        "{}.{}.{}/{}-{}",
        &d[0..2],
        &d[2..5],
        &d[5..8],
        &d[8..12],
        &d[12..14]
    )
}

/// `POST /rules/{id}/barcode` — the dropzone in `E`.
pub async fn attach(
    State(st): State<AppState>,
    auth: Auth,
    headers: axum::http::HeaderMap,
    Path(rule_id): Path<i64>,
    Query(p): Query<Params>,
    mut form: Multipart,
) -> Response {
    let t = auth.catalog();
    let say = |m: &str, bad: bool| crate::web::tx::say_only(m, bad);

    let Ok(Some(rule)) = rules::get(&st.pool, rule_id).await else {
        return say(t.error_not_written(), true);
    };
    if let Err(refused) = crate::web::rules::rule_access(&st, &auth, &rule).await {
        return refused;
    }
    // The pane's field wins over the saved password: it is what the person
    // is looking at, and the Save that would store it may not have happened.
    let fidget = Fidget::from(&headers, &st.live, auth.viewer.member_id);
    let input = match read_upload(&t, &mut form).await {
        Ok(i) => i,
        Err(Refused(m)) => return say(&m, true),
    };
    let answers = input.answers;
    let password = input.password.as_deref().or(rule.pdf_password.as_deref());
    let upload = match unlock_for(&t, input.upload, password, &fidget).await {
        Ok(u) => u,
        Err(Refused(m)) => return say(&m, true),
    };
    let period = Period::parse(&p.period).unwrap_or_else(|| Period::of(crate::clock::today()));
    let Some(tx_id) = occurrence_of(&st, rule_id, period).await else {
        return say(t.import_no_occurrence(), true);
    };
    attach_bill(&st, &auth, &rule, tx_id, period, upload, answers, &fidget).await
}

/// The row's own slot 1 — the file rail's `+` menu, a drop on target `1`,
/// and `gu1` all land here via `POST /tx/{id}/file/barcode`.
///
/// **It is the same attach as the rule pane's, not a second one.** It used to
/// store whatever it was given: a credit-card statement replaced a rule's
/// bound utility bill without a word, and the row went on recording the old
/// barcode beside a file that no longer matched it. Now the bill is read and
/// checked against the rule's binding exactly as `E`'s dropzone does.
///
/// A file that cannot be read — a photo, a scan with no text layer — is a
/// question on a BOUND rule, because there is nothing to confirm it is that
/// rule's bill; `y` files it as a plain document, noted. On a rule with
/// nothing bound it is filed as a plain document, which is all it ever was.
pub async fn attach_on_row(
    st: &AppState,
    auth: &Auth,
    tx_id: i64,
    upload: Upload,
    answers: Answers,
    fidget: &Fidget,
) -> Response {
    let t = auth.catalog();
    let say = |m: &str, bad: bool| crate::web::tx::say_only(m, bad);

    let row = sqlx::query!(
        r#"SELECT rule_id AS "rule_id: i64", period AS "period!: String"
           FROM transactions WHERE id = ?1 AND deleted_at IS NULL"#,
        tx_id
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten();
    let Some((Some(rule_id), Some(period))) = row.map(|r| (r.rule_id, Period::parse(&r.period)))
    else {
        return say(t.file_not_tracked(), true);
    };
    let Ok(Some(rule)) = rules::get(&st.pool, rule_id).await else {
        return say(t.file_not_tracked(), true);
    };

    attach_bill(st, auth, &rule, tx_id, period, upload, answers, fidget).await
}

/// One bill onto one occurrence of a rule — the attach every gesture shares:
/// `E`'s dropzone, and the row's own slot 1.
///
/// The bill is read with the rule's own **recipe**, when it has one. When it
/// has none yet, or the recipe no longer reads what the bill leaves open, the
/// household's assistant is asked to read it — and its proposal is a question
/// for the person, `y` to keep it as the rule's recipe. Every later month is
/// then read here, with no assistant at all.
#[allow(clippy::too_many_arguments)]
async fn attach_bill(
    st: &AppState,
    auth: &Auth,
    rule: &rules::Rule,
    tx_id: i64,
    period: Period,
    upload: Upload,
    answers: Answers,
    fidget: &Fidget,
) -> Response {
    let t = auth.catalog();
    let mut notes: Vec<String> = Vec::new();

    // `y` to a proposal: it becomes the rule's recipe before the bill is read
    // with it. A token already used (the same re-send asked something else
    // next) is simply gone — the recipe is stored.
    if let Teach::Approved(token) = &answers.teach
        && let Some((source, recipe)) = claim_proposal(token, auth.viewer.member_id)
    {
        if recipes::store(
            &st.pool,
            Actor::User(auth.viewer.member_id),
            rule.id,
            &source,
            &recipe,
        )
        .await
        .is_err()
        {
            return crate::web::tx::say_only(t.error_not_written(), true);
        }
        notes.push(t.import_teach_kept(&rule.title));
    }

    let recipe = recipes::live(&st.pool, rule.id).await;
    let kind = rule.barcode_kind.as_deref();
    fidget.step(t.progress_read());
    let read = read_bill_off(&upload, recipe.as_ref(), kind).await;

    if answers.teach == Teach::Ask && wants_teaching(rule, recipe.as_ref(), &read) {
        match teach(st, &t, &upload, kind, fidget).await {
            Ok(Some(proposal))
                if recipe
                    .as_ref()
                    .is_none_or(|r| !same_recipe(r, &proposal.recipe)) =>
            {
                return proposal.ask(&t, &rule.title).respond();
            }
            Ok(_) => {}
            Err(why) => {
                tracing::info!("assistant, rule {}: {why}", rule.id);
                if why != crate::services::assistant::AssistantError::Off {
                    notes.push(t.import_teach_failed().to_string());
                }
            }
        }
    }

    let finish = |message: String| {
        if notes.is_empty() {
            message
        } else {
            format!("{message} · {}", notes.join(" · "))
        }
    };
    match read {
        Ok(parsed) => {
            match attach_to(
                st,
                auth,
                rule,
                tx_id,
                period,
                &upload,
                &parsed,
                answers.anyway,
                fidget,
            )
            .await
            {
                // The row is re-read through the pane query, so what comes back
                // was decided by the database and the disk rather than by this
                // handler.
                Ok((member_id, message)) => {
                    crate::web::tx::row_fragment(
                        st,
                        auth,
                        tx_id,
                        member_id,
                        period,
                        &finish(message),
                    )
                    .await
                }
                Err(halt) => halt.respond(),
            }
        }
        // Nothing can confirm an unreadable file is this rule's bill — so the
        // person is asked, rather than refused outright.
        Err(e) if rule_binding(rule).is_bound() && !answers.anyway => crate::web::tx::ask(
            &t.import_ask_unreadable(&unread_reason(&t, &e), &rule.title),
            false,
        ),
        Err(_) => {
            let noted = rule_binding(rule).is_bound();
            store_plain(st, auth, rule, tx_id, period, &upload, noted, fidget).await
        }
    }
}

/// Whether a bill is worth asking the assistant about: a spend rule's bill
/// the recipe could not finish — no amount, no due date, not read at all —
/// or a rule with no recipe yet whose issuer one printed CNPJ cannot name.
/// A utility bill whose barcode says everything never asks.
fn wants_teaching(
    rule: &rules::Rule,
    recipe: Option<&RuleRecipe>,
    read: &Result<Parsed, ImportError>,
) -> bool {
    if rule.kind != crate::domain::status::TxKind::Out || rule.no_barcode {
        return false;
    }
    match read {
        Err(ImportError::Misread(_)) => false,
        Err(_) => true,
        Ok(p) => {
            p.import.amount.is_none()
                || p.import.due.is_none()
                || (recipe.is_none() && rule.issuer_id.is_none() && p.import.cnpjs.len() != 1)
        }
    }
}

fn same_recipe(live: &RuleRecipe, new: &crate::services::assistant::Recipe) -> bool {
    live.cnpj == new.cnpj
        && live.amount_regex == new.amount_regex
        && live.due_regex == new.due_regex
        && live.client_regex == new.client_regex
        && live.card_regex == new.card_regex
}

/// A verified proposal, parked under a token until the person answers.
pub struct Proposal {
    pub source: &'static str,
    pub recipe: crate::services::assistant::Recipe,
    pub token: String,
    /// What the bill reads as with it.
    pub parsed: Option<Parsed>,
    pub currency: crate::domain::money::Currency,
}

impl Proposal {
    /// "Teach Nubank to read its bills?" — `y` keeps the recipe and attaches
    /// with it; anything else attaches with what the bill gave without it.
    fn ask(&self, t: &Catalog, rule: &str) -> crate::web::tx::Ask {
        crate::web::tx::Ask {
            title: Some(t.import_teach_title(rule)),
            message: t.import_teach_message(self.source),
            lines: self.lines(t),
            yes_label: Some(t.import_teach_yes().to_string()),
            mismatch: false,
            yes: vec![("recipe", self.token.clone())],
            no: Some(vec![("recipe", "skip".to_string())]),
        }
    }

    /// What the proposal reads off this bill, one fact a line.
    pub fn lines(&self, t: &Catalog) -> Vec<String> {
        let mut out = Vec::new();
        let r = &self.recipe;
        let issuer = match (&r.issuer_name, &r.cnpj) {
            (Some(n), Some(c)) => Some(format!("{n} · {}", fmt_cnpj(c))),
            (Some(n), None) => Some(n.clone()),
            (None, Some(c)) => Some(fmt_cnpj(c)),
            (None, None) => None,
        };
        if let Some(i) = issuer {
            out.push(format!("{} — {i}", t.import_meta_issuer()));
        }
        if let Some(p) = &self.parsed {
            if let Some(a) = p.import.amount {
                out.push(format!(
                    "{} — {}",
                    t.import_teach_amount(),
                    fmt::money(a, self.currency)
                ));
            }
            if let Some(d) = p.import.due {
                out.push(format!("{} — {}", t.import_teach_due(), fmt::date(d)));
            }
            if let Some(c) = &p.import.client_code {
                out.push(format!("{} — {c}", t.import_client_code()));
            }
            if let Some(c) = p.import.cards.first() {
                out.push(format!(
                    "{} — {}",
                    t.import_meta_card(),
                    t.import_meta_card_value(c)
                ));
            }
        }
        out
    }
}

/// Asks the household's assistant to read a bill, checks what it says, and
/// parks a proposal worth asking about. `Ok(None)`: it answered, and nothing
/// it said held up.
pub async fn teach(
    st: &AppState,
    t: &Catalog,
    upload: &Upload,
    kind: Option<&str>,
    fidget: &Fidget,
) -> Result<Option<Proposal>, crate::services::assistant::AssistantError> {
    use crate::services::assistant::{self, AssistantError, Provider};
    let provider = crate::db::settings::assistant(&st.pool).await;
    if provider == Provider::None {
        return Err(AssistantError::Off);
    }
    let up = Upload {
        filename: upload.filename.clone(),
        bytes: upload.bytes.clone(),
        ..Default::default()
    };
    fidget.step(t.progress_ask(provider.code()));
    let (proposal, text) = tokio::task::spawn_blocking(move || {
        let scratch =
            scratch_file(&up).map_err(|e| AssistantError::Failed(format!("scratch file: {e}")))?;
        let page = assistant::page_one(&scratch.0);
        let full = import::text_of(&scratch.0).ok();
        let page = page.ok_or_else(|| AssistantError::Unreadable("no text on page 1".into()))?;
        let proposal = assistant::ask(provider, &page)?;
        Ok::<_, AssistantError>((proposal, full.unwrap_or(page)))
    })
    .await
    .map_err(|e| AssistantError::Failed(e.to_string()))??;
    fidget.step(t.progress_check());
    let recipe = assistant::verify(&proposal, &text);
    if recipe.is_empty() {
        return Ok(None);
    }
    // What the bill reads as with it — the lines the person confirms.
    let trial = RuleRecipe {
        cnpj: recipe.cnpj.clone(),
        issuer_name: recipe.issuer_name.clone(),
        amount_regex: recipe.amount_regex.clone(),
        due_regex: recipe.due_regex.clone(),
        client_regex: recipe.client_regex.clone(),
        card_regex: recipe.card_regex.clone(),
        ..Default::default()
    };
    let parsed = read_bill_off(upload, Some(&trial), kind).await.ok();
    let token = park_proposal(provider.code(), &recipe)
        .map_err(|e| AssistantError::Failed(format!("parking the proposal: {e}")))?;
    let currency = crate::db::ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();
    Ok(Some(Proposal {
        source: provider.code(),
        recipe,
        token,
        parsed,
        currency,
    }))
}

/// A proposal waiting for its answer, under a random token in the temp dir —
/// the same posture as `services::pending`'s parked files.
fn park_proposal(source: &str, r: &crate::services::assistant::Recipe) -> std::io::Result<String> {
    let mut raw = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let json = serde_json::json!({
        "source": source,
        "cnpj": r.cnpj, "issuer_name": r.issuer_name,
        "amount_regex": r.amount_regex, "due_regex": r.due_regex,
        "client_regex": r.client_regex, "card_regex": r.card_regex,
    });
    let dir = std::env::temp_dir().join("ffm-proposals");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{token}.json")), json.to_string())?;
    Ok(token)
}

/// Reads a parked proposal without taking it.
fn peek_proposal(token: &str) -> Option<(String, crate::services::assistant::Recipe)> {
    let (source, recipe, _) = read_proposal(token)?;
    Some((source, recipe))
}

/// Takes a parked proposal back, once. The member is not part of the token:
/// whoever may write the rule may approve it, and the token is 128 random
/// bits that only the page that was asked holds.
pub fn claim_proposal(
    token: &str,
    _member: i64,
) -> Option<(String, crate::services::assistant::Recipe)> {
    let (source, recipe, path) = read_proposal(token)?;
    let _ = std::fs::remove_file(&path);
    Some((source, recipe))
}

fn read_proposal(
    token: &str,
) -> Option<(
    String,
    crate::services::assistant::Recipe,
    std::path::PathBuf,
)> {
    if token.len() != 32 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let path = std::env::temp_dir()
        .join("ffm-proposals")
        .join(format!("{token}.json"));
    let raw = std::fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let s = |k: &str| v[k].as_str().map(str::to_string);
    Some((
        s("source")?,
        crate::services::assistant::Recipe {
            cnpj: s("cnpj"),
            issuer_name: s("issuer_name"),
            amount_regex: s("amount_regex"),
            due_regex: s("due_regex"),
            client_regex: s("client_regex"),
            card_regex: s("card_regex"),
        },
        path,
    ))
}

/// A file that is not a bill this app can read, filed under slot 1 as a plain
/// document — on an unbound rule, or on a bound one after the person said
/// `y`, which the log then notes.
#[allow(clippy::too_many_arguments)]
async fn store_plain(
    st: &AppState,
    auth: &Auth,
    rule: &rules::Rule,
    tx_id: i64,
    period: Period,
    upload: &Upload,
    noted: bool,
    fidget: &Fidget,
) -> Response {
    let t = auth.catalog();
    let say = |m: &str| crate::web::tx::say_only(m, true);
    let Some((slot, _, member_id)) = crate::web::files::slot_for(st, auth, tx_id).await else {
        return say(t.file_not_tracked());
    };
    let filed = filed_bytes(upload, auth.viewer.member_id, &t, fidget).await;
    fidget.step(t.progress_file());
    match receipts::store(
        &st.pool,
        &st.cfg.receipts_root,
        Actor::User(auth.viewer.member_id),
        &slot,
        FileKind::Barcode,
        &upload.filename,
        &filed,
    )
    .await
    {
        Ok(_) => {
            let mut message = t.file_added().to_string();
            if noted {
                let entry = Change::file(
                    Actor::User(auth.viewer.member_id),
                    FileAction::Attach,
                    tx_id,
                )
                .from_rule(Some(rule.id))
                .in_period(period)
                .note(ATTACHED_UNCONFIRMED);
                if let Ok(mut tx) = st.pool.begin().await
                    && entry.write(&mut tx).await.is_ok()
                {
                    let _ = tx.commit().await;
                }
                message = format!("{message} · {}", t.import_attached_anyway(&rule.title));
            }
            crate::web::tx::row_fragment(st, auth, tx_id, member_id, period, &message).await
        }
        Err(receipts::StoreError::Locked) => say(t.file_pdf_locked()),
        Err(e) => say(&e.to_string()),
    }
}

/// The live occurrence of a rule in one period, if it has been materialized.
pub async fn occurrence_of(st: &AppState, rule_id: i64, period: Period) -> Option<i64> {
    let period_s = period.to_string();
    sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM transactions
           WHERE rule_id = ?1 AND period = ?2 AND deleted_at IS NULL"#,
        rule_id,
        period_s
    )
    .fetch_optional(&st.pool)
    .await
    .ok()
    .flatten()
}

/// Attaches an already-parsed bill to one occurrence of a rule, and binds the
/// rule to it where nothing is bound yet.
///
/// **Nothing is written before every check has passed.** A bill that
/// contradicts what the rule is bound to, or that nothing can confirm, is a
/// question (`Halt::Ask`) — `anyway` is the person's `y`, and then it goes
/// ahead, noted in the log, teaching the rule nothing after a contradiction.
/// A bill already filed elsewhere is a refusal `anyway` never lifts. Returns
/// the owning member and the statusline message.
#[allow(clippy::too_many_arguments)]
pub async fn attach_to(
    st: &AppState,
    auth: &Auth,
    rule: &rules::Rule,
    tx_id: i64,
    period: Period,
    upload: &Upload,
    parsed: &Parsed,
    anyway: bool,
    fidget: &Fidget,
) -> Result<(i64, String), Halt> {
    let t = auth.catalog();
    let not_written = || Halt::Refused(t.error_not_written().to_string());
    let recipe = recipes::live(&st.pool, rule.id).await;

    // --- is this the right row? ---------------------------------------------
    let overridden = match gate(
        binding_check(rule_binding(rule), read_binding(parsed)),
        anyway,
    ) {
        Gate::Pass(o) => o,
        Gate::Ask(Some(m)) => {
            return Err(Halt::Ask {
                message: t.import_ask_mismatch(
                    &ident_label(&t, recipe.as_ref(), &m.found),
                    &rule.title,
                    &ident_label(&t, recipe.as_ref(), &m.known),
                ),
                mismatch: true,
            });
        }
        Gate::Ask(None) => {
            return Err(Halt::Ask {
                message: t.import_ask_unconfirmed(&rule.title),
                mismatch: false,
            });
        }
    };

    // --- is it already filed somewhere? -------------------------------------
    // `barcode_raw` is the natural idempotency key, with a unique index behind
    // it: the same bill cannot end up on two rows.
    //
    // **A row holds a bill only while it holds the file.** One whose bill was
    // removed still carries the barcode, and used to refuse every other row
    // the same bill — "already on Moto CG", about a row showing no bill at
    // all. Such a row gives the bill up to this one (`takeover`, below),
    // provided the viewer may write to it.
    //
    // **Unless the barcode cannot tell one bill from another**: a card's
    // payment slip with no date and no amount, identical on every month's
    // fatura (`boleto::Barcode::names_one_bill`), or no barcode at all. Then
    // the bill's TEXT is the key — its fingerprint, the same for a trimmed or
    // shrunk copy — and the same text on another live row is the same bill
    // twice.
    let barcode = parsed.barcode.as_ref();
    let raw = barcode.map(|b| b.raw.clone());
    let keyed_by_text = !barcode.is_some_and(|b| b.names_one_bill());
    let already = holder_of(st, parsed, tx_id).await;
    let (refuse, takeover) = match already {
        Some(r) if r.holds || !auth.viewer.may_write(r.member_id) => (Some(r), None),
        other => (None, other),
    };
    if let Some(row) = refuse {
        return Err(Halt::Refused(already_message(st, &t, auth, &row).await));
    }

    // The file itself is kept, under the deterministic name every other
    // receipt uses.
    let Some((slot, _, member_id)) = crate::web::files::slot_for(st, auth, tx_id).await else {
        return Err(Halt::Refused(t.file_not_tracked().to_string()));
    };
    // Shrunk only now: everything above read the bill as it was uploaded.
    let filed = filed_bytes(upload, auth.viewer.member_id, &t, fidget).await;
    fidget.step(t.progress_file());
    if receipts::store(
        &st.pool,
        &st.cfg.receipts_root,
        Actor::User(auth.viewer.member_id),
        &slot,
        FileKind::Barcode,
        &upload.filename,
        &filed,
    )
    .await
    .is_err()
    {
        return Err(not_written());
    }

    let mut notes: Vec<String> = Vec::new();
    let currency = crate::db::ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();

    // --- what the bill said about this occurrence -----------------------
    let row = sqlx::query!(
        r#"SELECT amount_cents AS "amount_cents: i64", due_date AS "due_date: String",
                  amount_source AS "amount_source: String",
                  due_date_source AS "due_date_source: String"
           FROM transactions WHERE id = ?1"#,
        tx_id
    )
    .fetch_one(&st.pool)
    .await
    .ok();

    let existing_amount = row.as_ref().and_then(|r| r.amount_cents).map(Cents);
    let existing_due = row
        .as_ref()
        .and_then(|r| r.due_date.as_deref())
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());

    let bill_amount = parsed.import.amount;
    let bill_due = parsed.import.due;

    // Fill what is empty; replace a seed; flag what a person typed. The policy
    // itself lives in `services::import` so it can be pinned by a test rather
    // than re-derived in each handler that imports something.
    use crate::services::import::{Fill, reconcile};

    // The amount always follows the bill, whoever set the row's figure: the
    // checksummed barcode — or, with none, the issuer's own page — is the
    // evidence of what is owed, and the change is its own entry in the log
    // (`import`, "read from a bill") that `u` puts back. The due date keeps
    // the older rule — a date a person set with `e` is kept and the
    // difference flagged.
    let typed = |source: Option<&str>| source == Some("user");
    let due_typed = typed(row.as_ref().and_then(|r| r.due_date_source.as_deref()));

    let amount_fill = reconcile(bill_amount, existing_amount, false);
    match amount_fill {
        Fill::Differs { found, kept } => notes.push(
            t.import_amount_differs(&fmt::money(found, currency), &fmt::money(kept, currency)),
        ),
        Fill::Replace { was, .. } => notes.push(t.import_replaced(&fmt::money(was, currency))),
        _ => {}
    }
    let due_fill = reconcile(bill_due, existing_due, due_typed);
    match due_fill {
        Fill::Differs { found, kept } => {
            notes.push(t.import_date_differs(&fmt::date(found), &fmt::date(kept)))
        }
        Fill::Replace { was, .. } => notes.push(t.import_replaced(&fmt::date(was))),
        _ => {}
    }

    let write_amount = amount_fill.to_write();
    let write_due = due_fill.to_write();

    let Ok(mut tx) = st.pool.begin().await else {
        return Err(not_written());
    };

    // --- a row whose bill was removed gives it up -------------------------
    // Its barcode is cleared first, so this row can take `idx_tx_barcode`;
    // what claimed to come from that barcode goes with it, since the schema
    // ties those sources to one. And its copy in the trash is retired
    // (`receipts::MOVED`): a bill is on one row only, so that copy can never
    // come back. The file stays in `.delete/` — nothing is destroyed.
    if let (Some(old), Some(raw)) = (&takeover, &raw) {
        let released =
            crate::db::tx::release_bill(&mut tx, Actor::User(auth.viewer.member_id), old.id, raw)
                .await;
        if released.is_err() {
            return Err(not_written());
        }
        notes.push(
            t.import_bill_moved(
                &old.title,
                &Period::parse(&old.period)
                    .map(fmt::period)
                    .unwrap_or_else(|| old.period.clone()),
            ),
        );
    }

    // The fingerprint, on the file just stored — only where the barcode
    // cannot tell one bill from another, which is where it is looked up.
    if keyed_by_text {
        let fingerprint = parsed.fingerprint.clone();
        let stamped = sqlx::query!(
            "UPDATE receipts SET text_hash = ?2
             WHERE transaction_id = ?1 AND kind = 'barcode' AND deleted_at IS NULL",
            tx_id,
            fingerprint
        )
        .execute(&mut *tx)
        .await;
        if stamped.is_err() {
            return Err(not_written());
        }
    }

    let kind = barcode.map(|b| b.kind.code());
    // A source is stamped only with the value it describes. It used to be
    // written whenever the bill carried one, so a row that KEPT its own figure
    // still claimed the barcode as the source of it. A bill read from its
    // text also stamps a value it merely agreed with: that figure is now what
    // the bill says, and nothing else claims it.
    let text_only = barcode.is_none();
    let amount_source =
        if write_amount.is_some() || (text_only && matches!(amount_fill, Fill::Agrees)) {
            parsed.import.amount_source
        } else {
            None
        };
    let due_source = if write_due.is_some() || (text_only && matches!(due_fill, Fill::Agrees)) {
        parsed.import.due_source
    } else {
        None
    };
    let amount_value = write_amount.map(|c| c.get());
    let due_value = write_due.map(|d| d.format("%Y-%m-%d").to_string());
    // The bill's issuer, as a CNPJ: the one the rule is bound to when the
    // bill prints it; else the recipe's; else the bill's only CNPJ. A
    // statement that prints several and names none is left unbound — the
    // assistant is what tells them apart.
    let cnpjs = &parsed.import.cnpjs;
    let issuer = rule
        .issuer_id
        .clone()
        .filter(|k| cnpjs.contains(k))
        .or_else(|| {
            recipe
                .as_ref()
                .and_then(|r| r.cnpj.clone())
                .filter(|c| cnpjs.contains(c))
        })
        .or_else(|| (cnpjs.len() == 1).then(|| cnpjs[0].clone()));

    // A bill with no barcode replaces whatever barcode the row's last bill
    // had — a row holds a bill only while it holds the file — and the sources
    // that claimed that barcode go with it, as the schema requires.
    let ok = sqlx::query!(
        r#"UPDATE transactions SET
               barcode_raw = ?2, barcode_kind = ?3, issuer_id = ?4,
               amount_cents = coalesce(?5, amount_cents),
               due_date = coalesce(?6, due_date),
               amount_source = coalesce(?7, CASE WHEN ?2 IS NULL AND amount_source = 'barcode'
                                                 THEN NULL ELSE amount_source END),
               due_date_source = coalesce(?8, CASE WHEN ?2 IS NULL
                                                    AND due_date_source IN ('fator', 'free_field')
                                                   THEN NULL ELSE due_date_source END),
               -- A new amount moved under any `e` form open on this row, so
               -- its Save must be refused rather than write the old figure
               -- back: the lost-update guard, doing its job.
               version = version + (?5 IS NOT NULL AND ?5 IS NOT amount_cents)
           WHERE id = ?1"#,
        tx_id,
        raw,
        kind,
        issuer,
        amount_value,
        due_value,
        amount_source,
        due_source,
    )
    .execute(&mut *tx)
    .await
    .is_ok();

    if !ok {
        return Err(not_written());
    }

    // --- the binding, which lives on the RULE ---------------------------
    // The same account every period, unlike the value and the due date, which
    // are genuinely per-period. Only what is empty is filled: the check above
    // already asked about anything that disagreed with what is bound.
    //
    // Nothing at all after the person overrode a CONTRADICTION: the values
    // would come from a bill already known not to be this rule's. And nothing
    // on a rule the binding does not belong to — income, or "no barcode" —
    // where the schema's triggers would abort the transaction after the file
    // is already in the vault.
    let card = rule
        .card_last4
        .clone()
        .or(parsed.import.cards.first().cloned());
    let client_code = rule
        .client_code
        .clone()
        .or(parsed.import.client_code.clone());
    let company = rule
        .barcode_company
        .clone()
        .or(barcode.and_then(|b| b.company.clone()));
    let issuer_id = rule.issuer_id.clone().or(issuer.clone());
    let barcode_kind = rule
        .barcode_kind
        .clone()
        .or(kind.map(str::to_string))
        .or(parsed.import.issuer_kind.clone());
    let learns = !matches!(overridden, Some(Override::Mismatch(_)))
        && rule.kind == crate::domain::status::TxKind::Out
        && !rule.no_barcode;
    if learns {
        let rule_id = rule.id;
        let bound_ok = sqlx::query!(
            "UPDATE recurring_rules SET client_code = ?2, barcode_company = ?3,
                    issuer_id = ?4, barcode_kind = ?5, card_last4 = ?6
             WHERE id = ?1",
            rule_id,
            client_code,
            company,
            issuer_id,
            barcode_kind,
            card
        )
        .execute(&mut *tx)
        .await
        .is_ok();
        if !bound_ok {
            return Err(not_written());
        }
    }
    let (client_code, company, issuer_id, card) = if learns {
        (client_code, company, issuer_id, card)
    } else {
        (
            rule.client_code.clone(),
            rule.barcode_company.clone(),
            rule.issuer_id.clone(),
            rule.card_last4.clone(),
        )
    };

    let mut entry = Change::file(
        Actor::User(auth.viewer.member_id),
        FileAction::Attach,
        tx_id,
    )
    .from_rule(Some(rule.id))
    .in_period(period)
    .diff("barcode", Value::Text(None), Value::Text(raw.clone()))
    .diff(
        "client_code",
        Value::Text(rule.client_code.clone()),
        Value::Text(client_code),
    )
    .diff(
        "company_code",
        Value::Text(rule.barcode_company.clone()),
        Value::Text(company),
    )
    .diff(
        "issuer",
        Value::Text(rule.issuer_id.clone()),
        Value::Text(issuer_id),
    )
    .diff(
        "card",
        Value::Text(rule.card_last4.clone()),
        Value::Text(card),
    );
    match &overridden {
        Some(Override::Unconfirmed) => entry = entry.note(ATTACHED_UNCONFIRMED),
        Some(Override::Mismatch(m)) => {
            entry = entry.note(format!(
                "{ATTACHED_MISMATCH}: {} ≠ {}",
                ident_label(&t, recipe.as_ref(), &m.found),
                ident_label(&t, recipe.as_ref(), &m.known)
            ))
        }
        None => {}
    }
    let _ = entry.write(&mut tx).await;

    // The amount, as an entry of its own rather than one field folded inside
    // "file attached · 3 fields": the one change a person needs to see is that
    // the row's figure moved, and why. Written after the upload's entry, so
    // `u` puts the old amount back before it takes the file away.
    let amount_entry = Change::transaction(
        Actor::Import(Some(auth.viewer.member_id)),
        TxAction::Import,
        tx_id,
    )
    .from_rule(Some(rule.id))
    .in_period(period)
    .diff(
        "amount",
        Value::Money(existing_amount),
        Value::Money(write_amount.or(existing_amount)),
    );
    if !amount_entry.is_empty() && amount_entry.write(&mut tx).await.is_err() {
        return Err(not_written());
    }
    if tx.commit().await.is_err() {
        return Err(not_written());
    }

    // What actually happened, in one line — and where the figure came from:
    // a bill with no barcode, or one whose barcode carries no amount (a card's
    // open slip), was read off its page, not off the barcode.
    let from_text = barcode.is_none() || parsed.import.amount_source == Some("text");
    let unresolved = || {
        let what = parsed.import.unresolved.join(", ");
        (!what.is_empty()).then(|| t.import_unresolved(&what))
    };
    let headline = match (bill_amount, bill_due) {
        (Some(a), Some(d)) if from_text => {
            t.import_filled_text(&fmt::money(a, currency), &fmt::date(d))
        }
        (Some(a), Some(d)) => t.import_filled(&fmt::money(a, currency), &fmt::date(d)),
        // The amount is the news; what the bill did not carry follows it,
        // rather than being the only thing said.
        (Some(a), None) => {
            let read = if from_text {
                t.import_read_amount_text(&fmt::money(a, currency))
            } else {
                t.import_read_amount(&fmt::money(a, currency))
            };
            match unresolved() {
                Some(u) => format!("{read} · {u}"),
                None => read,
            }
        }
        _ => unresolved().unwrap_or_else(|| t.file_added().to_string()),
    };
    if overridden.is_some() {
        notes.push(t.import_attached_anyway(&rule.title));
    }
    let message = if notes.is_empty() {
        headline
    } else {
        format!("{headline} · {}", notes.join(" · "))
    };
    Ok((member_id, message))
}

/// The live row, other than `except_tx`, that already holds this bill — by
/// its barcode, or by its text where the barcode cannot tell one bill from
/// another (see `attach_to`). Asked again by [`find`] before the trim view
/// opens, so a bill filed twice is said at once rather than after Save.
async fn holder_of(st: &AppState, parsed: &Parsed, except_tx: i64) -> Option<Holder> {
    let barcode = parsed.barcode.as_ref();
    let raw = barcode.map(|b| b.raw.clone());
    let keyed_by_text = !barcode.is_some_and(|b| b.names_one_bill());
    let tx_id = except_tx;
    match (&raw, keyed_by_text) {
        (Some(raw), false) => sqlx::query!(
            r#"SELECT t.id AS "id!: i64", t.period AS "period!: String",
                      t.title AS "title!: String",
                      t.amount_cents AS "amount_cents: i64", t.member_id AS "member_id!: i64",
                      t.visibility AS "visibility!: String",
                      EXISTS (SELECT 1 FROM receipts r
                              WHERE r.transaction_id = t.id AND r.kind = 'barcode'
                                AND r.deleted_at IS NULL) AS "holds!: bool"
               FROM transactions t
               WHERE t.barcode_raw = ?1 AND t.id <> ?2 AND t.deleted_at IS NULL"#,
            raw,
            tx_id
        )
        .fetch_optional(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|r| Holder {
            id: r.id,
            period: r.period,
            title: r.title,
            amount_cents: r.amount_cents,
            member_id: r.member_id,
            visibility: r.visibility,
            holds: r.holds,
        }),
        _ => sqlx::query!(
            r#"SELECT t.id AS "id!: i64", t.period AS "period!: String",
                      t.title AS "title!: String",
                      t.amount_cents AS "amount_cents: i64", t.member_id AS "member_id!: i64",
                      t.visibility AS "visibility!: String"
               FROM receipts r JOIN transactions t ON t.id = r.transaction_id
               WHERE r.text_hash = ?1 AND r.kind = 'barcode' AND r.deleted_at IS NULL
                 AND t.deleted_at IS NULL AND t.id <> ?2
               LIMIT 1"#,
            parsed.fingerprint,
            tx_id
        )
        .fetch_optional(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|r| Holder {
            id: r.id,
            period: r.period,
            title: r.title,
            amount_cents: r.amount_cents,
            member_id: r.member_id,
            visibility: r.visibility,
            // A fingerprint is only ever on a file that is there: the row
            // holds it, and there is nothing to take over.
            holds: true,
        }),
    }
}

/// Why a bill is refused as already filed — naming the row that holds it,
/// unless that row is private to somebody else.
async fn already_message(st: &AppState, t: &Catalog, auth: &Auth, row: &Holder) -> String {
    let label = Period::parse(&row.period)
        .map(fmt::period)
        .unwrap_or_else(|| row.period.clone());
    // Naming the row that holds it is what makes the refusal useful —
    // unless that row is private to somebody else, when its title and
    // amount are exactly what `private` hides. The month alone says no
    // more than the refusal itself already does.
    if row.visibility == "private" && !auth.viewer.sees_private_of(row.member_id) {
        return t.import_already_attached_hidden(&label);
    }
    let currency = crate::db::ledger::household(&st.pool)
        .await
        .ok()
        .flatten()
        .map(|h| h.currency)
        .unwrap_or_default();
    let amount = row
        .amount_cents
        .map(|c| fmt::money(Cents(c), currency))
        .unwrap_or_else(|| fmt::money_placeholder(currency));
    t.import_already_attached(&label, &row.title, &amount)
}

/// A live row already carrying the bill being attached.
struct Holder {
    id: i64,
    period: String,
    title: String,
    amount_cents: Option<i64>,
    member_id: i64,
    visibility: String,
    /// It still has the bill's file — otherwise it gives the bill up.
    holds: bool,
}

// ---------------------------------------------------------------------------
// what the pane shows about the bill
// ---------------------------------------------------------------------------

/// "Bill on file": what a rule is bound to, or — in `A`, before the rule
/// exists — what the dropped bill will bind it to.
///
/// No amount and no due date: those change every period, and this box is
/// about the identity that does not.
#[derive(Template)]
#[template(path = "partials/barcode_meta.html")]
pub struct BarcodeMeta {
    pub t: Catalog,
    /// `E`: the rule, for Unlink. `None` in `A`.
    pub rule_id: Option<i64>,
    pub issuer: Option<String>,
    /// Already a label, not the stored code.
    pub kind: Option<String>,
    pub company: Option<String>,
    pub client_code: Option<String>,
    /// The card the rule is bound to, already a label ("ending 7766").
    pub card: Option<String>,
    /// `A`: the dropped bill had no barcode and was read from its text.
    pub text_only: bool,
    /// `A` only: the parked file Create will attach.
    pub token: Option<String>,
    pub filename: Option<String>,
    /// `A` only: the assistant's parked recipe, kept by Create — and the line
    /// saying who read it.
    pub recipe_token: Option<String>,
    pub taught: Option<String>,
    /// `A` only: what the recipe read off the bill, one fact a line.
    pub taught_lines: Vec<String>,
}

/// What a rule's issuer is called: its recipe's name, else its CNPJ.
fn issuer_label(recipe: Option<&RuleRecipe>, cnpj: Option<&str>) -> Option<String> {
    recipe
        .and_then(|r| r.issuer_name.clone())
        .or_else(|| cnpj.map(fmt_cnpj))
}

fn kind_label(t: &Catalog, code: Option<&str>) -> Option<String> {
    match code? {
        "consumo" => Some(t.import_meta_kind_consumo().to_string()),
        "cobranca" => Some(t.import_meta_kind_cobranca().to_string()),
        other => Some(other.to_string()),
    }
}

impl BarcodeMeta {
    pub fn empty(t: Catalog) -> BarcodeMeta {
        BarcodeMeta {
            t,
            rule_id: None,
            issuer: None,
            kind: None,
            company: None,
            client_code: None,
            card: None,
            text_only: false,
            token: None,
            filename: None,
            recipe_token: None,
            taught: None,
            taught_lines: Vec::new(),
        }
    }

    pub fn of_rule(t: Catalog, rule: &rules::Rule, recipe: Option<&RuleRecipe>) -> BarcodeMeta {
        BarcodeMeta {
            rule_id: Some(rule.id),
            issuer: issuer_label(recipe, rule.issuer_id.as_deref()),
            kind: kind_label(&t, rule.barcode_kind.as_deref()),
            company: rule.barcode_company.clone(),
            client_code: rule.client_code.clone(),
            card: rule
                .card_last4
                .as_deref()
                .map(|d| t.import_meta_card_value(d)),
            text_only: false,
            token: None,
            filename: None,
            recipe_token: None,
            taught: None,
            taught_lines: Vec::new(),
            t,
        }
    }

    pub fn of_parsed(t: Catalog, parsed: &Parsed, token: String, filename: String) -> BarcodeMeta {
        BarcodeMeta {
            issuer: parsed.import.issuer_name.clone().or_else(|| {
                // With no recipe yet, a bill printing exactly one CNPJ is
                // that issuer's — it is what the rule will bind to.
                (parsed.import.cnpjs.len() == 1).then(|| fmt_cnpj(&parsed.import.cnpjs[0]))
            }),
            kind: kind_label(
                &t,
                parsed
                    .barcode
                    .as_ref()
                    .map(|b| b.kind.code())
                    .or(parsed.import.issuer_kind.as_deref()),
            ),
            company: parsed.barcode.as_ref().and_then(|b| b.company.clone()),
            client_code: parsed.import.client_code.clone(),
            card: parsed
                .import
                .cards
                .first()
                .map(|d| t.import_meta_card_value(d)),
            text_only: parsed.barcode.is_none(),
            recipe_token: None,
            taught: None,
            taught_lines: Vec::new(),
            rule_id: None,
            token: Some(token),
            filename: Some(filename),
            t,
        }
    }

    /// Nothing bound and nothing parked: the box is not drawn at all — the
    /// same "absent, not a blank field" treatment `instance` gets.
    pub fn is_empty(&self) -> bool {
        self.token.is_none()
            && self.issuer.is_none()
            && self.kind.is_none()
            && self.company.is_none()
            && self.client_code.is_none()
            && self.card.is_none()
    }
}

/// `GET /rules/{id}/meta` — the box on its own, refreshed after an `E` drop.
pub async fn meta(State(st): State<AppState>, auth: Auth, Path(rule_id): Path<i64>) -> Response {
    let t = auth.catalog();
    match rules::get(&st.pool, rule_id).await {
        Ok(Some(rule))
            if rule.visibility != crate::domain::status::Visibility::Private
                || auth.viewer.sees_private_of(rule.member_id) =>
        {
            let recipe = recipes::live(&st.pool, rule.id).await;
            Html(BarcodeMeta::of_rule(t, &rule, recipe.as_ref())).into_response()
        }
        _ => Html(BarcodeMeta::empty(t)).into_response(),
    }
}

/// One row a dropped bill was recognized for.
pub struct Found {
    pub tx_id: i64,
    /// The rule's name, as the picker's bold line.
    pub title: String,
    /// Whose ledger and which month — the picker's quieter line.
    pub whose: String,
    /// *Nubank · 01/2026*, for the statusline when the row is off screen.
    pub label: String,
}

/// The rows a dropped bill was recognized for — one is sent there at once,
/// several are asked about (`#billpick`).
#[derive(Template)]
#[template(path = "partials/bill_pick.html")]
pub struct BillPick {
    pub t: Catalog,
    pub found: Vec<Found>,
}

/// `POST /bills/find` — a bill dropped anywhere on the page, with no row
/// named. Answers the rows it belongs to; files nothing.
///
/// **It recognizes, it never binds.** The only candidates are the viewer's
/// writable, tracked spend rules that a bill has ALREADY bound — by `A`/`E`
/// or by a row's own slot 1 — and a rule is a match only when its binding
/// agrees on something that names an account or an issuer
/// ([`import::names_rule`]): the client code, the issuer's CNPJ, the card,
/// the company code. No filename, no title, no nearest guess, and an unbound
/// rule is never offered. The row is the rule's in the bill's due-date month
/// (the month on screen when the bill shows none).
///
/// The page then sends the file through the very upload a drop on target `1`
/// makes, so the trim view, the binding check, every question, the amount
/// read off the bill and the log all run unchanged.
pub async fn find(
    State(st): State<AppState>,
    auth: Auth,
    headers: axum::http::HeaderMap,
    mut form: Multipart,
) -> Response {
    let t = auth.catalog();
    let say = |m: &str| crate::web::tx::say_only(m, true);
    let fidget = Fidget::from(&headers, &st.live, auth.viewer.member_id);

    let mut filename = String::new();
    let mut bytes = Vec::new();
    let mut period = String::new();
    while let Ok(Some(field)) = form.next_field().await {
        match field.name() {
            Some("file") => {
                filename = field.file_name().unwrap_or_default().to_string();
                match field.bytes().await {
                    Ok(b) if b.len() <= MAX_BYTES => bytes = b.to_vec(),
                    Ok(_) => return say(t.file_too_big()),
                    Err(_) => return say(t.error_not_written()),
                }
            }
            Some("period") => period = field.text().await.unwrap_or_default(),
            _ => {}
        }
    }
    if bytes.is_empty() || filename.is_empty() {
        return say(t.error_not_written());
    }
    let today = crate::clock::today();
    let on_screen = Period::parse(period.trim()).unwrap_or_else(|| Period::of(today));
    fidget.step(t.progress_find());

    let candidates = candidates(&st.pool, auth.viewer).await;
    if candidates.is_empty() {
        return say(t.drop_no_rule());
    }

    // The text, once — opened with whichever rule's password opens it.
    let mut passwords: Vec<String> = candidates
        .iter()
        .filter_map(|r| r.pdf_password.clone())
        .collect();
    passwords.sort();
    passwords.dedup();
    if crate::services::pdflock::looks_locked(&filename, &bytes) {
        fidget.step(t.progress_unlock());
    }
    let upload = Upload {
        filename,
        bytes,
        ..Default::default()
    };
    let text = tokio::task::spawn_blocking(move || read_text(upload, &passwords))
        .await
        .unwrap_or_else(|e| Err(TextError::Read(ImportError::Tool(e.to_string()))));
    let text = match text {
        Ok(text) => text,
        Err(TextError::Locked) => return say(t.drop_locked()),
        Err(TextError::Read(e)) => return say(&unread_reason(&t, &e)),
    };

    fidget.step(t.progress_read());
    let Recognized { matched, no_row } =
        recognize(&st.pool, &candidates, &text, on_screen, today).await;
    if matched.is_empty() {
        return match no_row {
            Some((title, period)) => say(&t.drop_no_row(&title, &fmt::period(period))),
            None => say(t.drop_no_rule()),
        };
    }

    // Filed already? Said now, rather than after the trim view's Save — on
    // any row, its own included: a bill dropped anywhere is never a re-file
    // (that is `t` from `P`, which replaces a row's file on purpose).
    if let Some(row) = holder_of(&st, &matched[0].parsed, 0).await
        && (row.holds || !auth.viewer.may_write(row.member_id))
    {
        return say(&already_message(&st, &t, &auth, &row).await);
    }

    let names: std::collections::HashMap<i64, String> = crate::db::members::list(&st.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|m| (m.id, fmt::name(&m.name)))
        .collect();
    let found = matched
        .into_iter()
        .map(|m| {
            let month = fmt::period(m.period);
            Found {
                tx_id: m.tx_id,
                title: m.title.clone(),
                whose: match names.get(&m.member_id) {
                    Some(name) => format!("{name} · {month}"),
                    None => month.clone(),
                },
                label: format!("{} · {month}", m.title),
            }
        })
        .collect();
    Html(BillPick { t, found }).into_response()
}

/// Every rule a bill has already bound, that `viewer` may write to — the
/// only rules a dropped bill can be recognized for. A binding by the barcode
/// kind alone is left out: it names no account and no issuer.
pub async fn candidates(pool: &sqlx::SqlitePool, viewer: Viewer) -> Vec<rules::Rule> {
    let ids: Vec<i64> = sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM recurring_rules
           WHERE deleted_at IS NULL AND kind = 'out' AND tracked = 1 AND no_barcode = 0
             AND (client_code IS NOT NULL OR issuer_id IS NOT NULL
                  OR card_last4 IS NOT NULL OR barcode_company IS NOT NULL)
           ORDER BY id"#
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let mut out = Vec::new();
    for id in ids {
        if let Ok(Some(rule)) = rules::get(pool, id).await
            && viewer.may_write(rule.member_id)
        {
            out.push(rule);
        }
    }
    out
}

/// A rule a dropped bill was recognized for, and the row it goes on.
pub struct Match {
    pub rule_id: i64,
    pub title: String,
    pub member_id: i64,
    pub tx_id: i64,
    pub period: Period,
    pub parsed: Parsed,
}

/// What a dropped bill's text was recognized as.
pub struct Recognized {
    pub matched: Vec<Match>,
    /// A rule that knew the bill but has no row in its month — said when
    /// nothing else matched, rather than "no rule knows it".
    pub no_row: Option<(String, Period)>,
}

/// Reads `text` with each candidate's own recipe and keeps the rules whose
/// binding it names ([`import::names_rule`]). The row is the rule's in the
/// bill's due-date month, or `on_screen` when the bill shows no due date.
pub async fn recognize(
    pool: &sqlx::SqlitePool,
    candidates: &[rules::Rule],
    text: &str,
    on_screen: Period,
    today: NaiveDate,
) -> Recognized {
    let mut matched = Vec::new();
    let mut no_row = None;
    for rule in candidates {
        let recipe = recipes::live(pool, rule.id).await;
        let profiles = recipe
            .as_ref()
            .map(|r| r.profiles(rule.barcode_kind.as_deref()))
            .unwrap_or_default();
        let Ok(parsed) = import::parse_text(text, &profiles, today) else {
            continue;
        };
        if !import::names_rule(rule_binding(rule), read_binding(&parsed)) {
            continue;
        }
        let period = parsed.import.due.map(Period::of).unwrap_or(on_screen);
        let key = period.to_string();
        let tx = sqlx::query_scalar!(
            r#"SELECT id AS "id!: i64" FROM transactions
               WHERE rule_id = ?1 AND period = ?2 AND deleted_at IS NULL"#,
            rule.id,
            key
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        match tx {
            Some(tx_id) => matched.push(Match {
                rule_id: rule.id,
                title: rule.title.clone(),
                member_id: rule.member_id,
                tx_id,
                period,
                parsed,
            }),
            None => no_row = no_row.or(Some((rule.title.clone(), period))),
        }
    }
    Recognized { matched, no_row }
}

/// Why a dropped file's text could not be had.
enum TextError {
    /// Locked, and no candidate rule's password opens it.
    Locked,
    Read(ImportError),
}

/// A dropped file's text, unlocked with the first password that opens it.
/// Blocking: `qpdf` and `pdftotext` are child processes.
fn read_text(upload: Upload, passwords: &[String]) -> Result<String, TextError> {
    use crate::services::pdflock::{self, LockError};
    let mut open = None;
    let tries = std::iter::once(None).chain(passwords.iter().map(|p| Some(p.as_str())));
    for password in tries {
        match pdflock::unlock(&upload.filename, &upload.bytes, password) {
            Ok(bytes) => {
                open = Some(bytes.into_owned());
                break;
            }
            Err(LockError::NeedsPassword | LockError::WrongPassword) => continue,
            Err(LockError::NoTool) => {
                return Err(TextError::Read(ImportError::Tool(
                    "qpdf is not installed".into(),
                )));
            }
            Err(LockError::Tool(why)) => return Err(TextError::Read(ImportError::Tool(why))),
        }
    }
    let Some(bytes) = open else {
        return Err(TextError::Locked);
    };
    let unlocked = Upload {
        filename: upload.filename,
        bytes,
        ..Default::default()
    };
    let scratch =
        scratch_file(&unlocked).map_err(|e| TextError::Read(ImportError::Tool(e.to_string())))?;
    import::text_of(&scratch.0).map_err(TextError::Read)
}

/// `POST /rules/barcode/preview` — the dropzone in `A`.
///
/// The rule does not exist yet, so nothing is attached: the bill is read,
/// parked under a token, and the pane gets back what it says. Create carries
/// the token and attaches it — see `services::pending`.
pub async fn preview(
    State(st): State<AppState>,
    auth: Auth,
    headers: axum::http::HeaderMap,
    mut form: Multipart,
) -> Response {
    let t = auth.catalog();
    let fidget = Fidget::from(&headers, &st.live, auth.viewer.member_id);
    // No rule yet, so the only password is the one typed in the pane. What
    // is parked is the UNLOCKED copy, so Create attaches it as it is.
    let upload = match read_upload(&t, &mut form).await {
        Ok(i) => unlock_for(&t, i.upload, i.password.as_deref(), &fidget).await,
        Err(refused) => Err(refused),
    };
    let upload = match upload {
        Ok(u) => u,
        Err(Refused(m)) => return crate::web::tx::say_only(&m, true),
    };
    // A new rule has no recipe: when the bill leaves anything open — or its
    // issuer cannot be told from one printed CNPJ — the assistant is asked,
    // and what it read goes in the box, kept when the rule is created.
    fidget.step(t.progress_read());
    let first = read_bill_off(&upload, None, None).await;
    let open = match &first {
        Err(ImportError::Misread(_)) => false,
        Err(_) => true,
        Ok(p) => p.import.amount.is_none() || p.import.due.is_none() || p.import.cnpjs.len() != 1,
    };
    let mut proposal = None;
    if open {
        match teach(&st, &t, &upload, None, &fidget).await {
            Ok(p) => proposal = p,
            Err(why) => tracing::info!("assistant, new rule: {why}"),
        }
    }
    let parsed = match (proposal.as_mut().and_then(|p| p.parsed.take()), first) {
        (Some(p), _) | (None, Ok(p)) => p,
        (None, Err(e)) => return crate::web::tx::say_only(&unread_reason(&t, &e), true),
    };
    let Ok(token) = pending::park(
        auth.viewer.member_id,
        &upload.filename,
        &upload.bytes,
        upload.shrink.on,
    ) else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    let mut meta = BarcodeMeta::of_parsed(t, &parsed, token, upload.filename);
    if let Some(p) = proposal {
        p.into_meta(&t, parsed, &mut meta);
    }
    Html(meta).into_response()
}

impl Proposal {
    /// Puts a proposal in `A`'s box: who read it, what it read, and the token
    /// Create keeps it by.
    fn into_meta(mut self, t: &Catalog, parsed: Parsed, meta: &mut BarcodeMeta) {
        self.parsed = Some(parsed);
        meta.taught = Some(t.import_meta_taught(self.source));
        meta.taught_lines = self.lines(t);
        meta.recipe_token = Some(self.token);
    }
}

/// The same box for a parked bill, re-read — a rejected Create renders the
/// pane again and must not lose what was dropped.
pub fn meta_of_parked(
    t: Catalog,
    token: &str,
    recipe_token: Option<&str>,
    member_id: i64,
) -> Option<BarcodeMeta> {
    let filename = pending::filename_of(token, member_id)?;
    let bytes = pending::peek(token, member_id)?;
    let upload = Upload {
        filename: filename.clone(),
        bytes,
        ..Default::default()
    };
    // The parked recipe is read, not claimed: Create still needs it.
    let proposal = recipe_token.and_then(|tok| peek_proposal(tok).map(|(s, r)| (tok, s, r)));
    let trial = proposal.as_ref().map(|(_, _, r)| RuleRecipe {
        cnpj: r.cnpj.clone(),
        issuer_name: r.issuer_name.clone(),
        amount_regex: r.amount_regex.clone(),
        due_regex: r.due_regex.clone(),
        client_regex: r.client_regex.clone(),
        card_regex: r.card_regex.clone(),
        ..Default::default()
    });
    let parsed = parse(&t, &upload, trial.as_ref(), None).ok()?;
    let mut meta = BarcodeMeta::of_parsed(t, &parsed, token.to_string(), filename);
    if let Some((tok, source, recipe)) = proposal {
        let p = Proposal {
            source: crate::services::assistant::Provider::parse(&source)
                .map(|p| p.code())
                .unwrap_or("assistant"),
            recipe,
            token: tok.to_string(),
            parsed: None,
            currency: crate::domain::money::Currency::default(),
        };
        p.into_meta(&t, parsed, &mut meta);
    }
    Some(meta)
}

/// Clears what the rule is bound to — a narrow correction for "the wrong
/// file got attached the first time", distinct from `u`'s full revert of the
/// last change. It touches nothing else about the rule, and it answers with
/// the (now empty) box, so the pane stays open for the corrected drop.
pub async fn unlink(State(st): State<AppState>, auth: Auth, Path(rule_id): Path<i64>) -> Response {
    let t = auth.catalog();
    let Ok(Some(rule)) = rules::get(&st.pool, rule_id).await else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    if let Err(refused) = crate::web::rules::rule_access(&st, &auth, &rule).await {
        return refused;
    }

    let Ok(mut tx) = st.pool.begin().await else {
        return crate::web::tx::say_only(t.error_not_written(), true);
    };
    let _ = sqlx::query!(
        "UPDATE recurring_rules SET client_code = NULL, barcode_company = NULL,
                issuer_id = NULL, barcode_kind = NULL, card_last4 = NULL
         WHERE id = ?1",
        rule_id
    )
    .execute(&mut *tx)
    .await;
    let _ = Change::rule(
        Actor::User(auth.viewer.member_id),
        crate::db::changes::RuleAction::Edit,
        rule_id,
    )
    .diff(
        "client_code",
        Value::Text(rule.client_code),
        Value::Text(None),
    )
    .diff(
        "company_code",
        Value::Text(rule.barcode_company),
        Value::Text(None),
    )
    .diff("issuer", Value::Text(rule.issuer_id), Value::Text(None))
    .diff("card", Value::Text(rule.card_last4), Value::Text(None))
    .write(&mut tx)
    .await;
    if tx.commit().await.is_err() {
        return crate::web::tx::say_only(t.error_not_written(), true);
    }

    let res = Html(BarcodeMeta::empty(t)).into_response();
    crate::web::tx::say(res, t.import_unlinked(), false)
}

/// Amount and due date as the import sees them, for the resolver.
impl crate::services::issuers::Import {
    pub fn describe(&self) -> String {
        match (self.amount, self.due) {
            (Some(_), Some(_)) => "amount and due date".into(),
            (Some(_), None) => "amount".into(),
            (None, Some(_)) => "due date".into(),
            _ => "nothing".into(),
        }
    }
}

/// `Amount` is a sum type on purpose; the resolver already flattened it to
/// `Option<Cents>`, and this is the reminder of why that flattening is only
/// safe there — `Reference` must never become money.
#[allow(dead_code)]
fn never_coerce(a: Amount) -> Option<Cents> {
    match a {
        Amount::Fixed(c) => Some(c),
        Amount::Open | Amount::Reference(_) => None,
    }
}

#[cfg(test)]
mod shrink_tests {
    use super::*;
    use crate::ui::i18n::Lang;

    #[test]
    fn the_trim_views_fields_are_read_whatever_their_order() {
        let mut ask = ShrinkAsk::default();
        assert!(ask.read("keep", " 1,3 "));
        assert!(ask.read("stage", "abc"));
        assert!(ask.read("shrink", "1"));
        assert!(!ask.read("file", "x"));
        assert!(ask.on);
        assert_eq!(ask.stage.as_deref(), Some("abc"));
        assert_eq!(ask.pages(), Some(vec![1, 3]));
        // An empty field is no value, not an empty one.
        ask.read("stage", "");
        assert!(ask.stage.is_none());
    }

    #[tokio::test]
    async fn nothing_is_shrunk_unless_asked_and_only_a_pdf() {
        let t = Catalog::new(Lang::En);
        let off = Upload {
            filename: "conta.pdf".into(),
            bytes: b"%PDF-1.4 not really".to_vec(),
            ..Default::default()
        };
        assert_eq!(filed_bytes(&off, 1, &t, &Fidget::off()).await, off.bytes);
        let text = Upload {
            filename: "conta.txt".into(),
            bytes: b"Total a pagar R$ 1,00".to_vec(),
            shrink: ShrinkAsk {
                on: true,
                ..Default::default()
            },
        };
        assert_eq!(filed_bytes(&text, 1, &t, &Fidget::off()).await, text.bytes);
    }
}
