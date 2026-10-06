//! The household's assistant: a CLI on the server, run on the household's own
//! subscription, that reads a bill's first page and writes the rule's
//! **recipe** — how to read that issuer's bills from then on.
//!
//! **The only place a CLI is ever run.** `codex exec`, `claude -p` and
//! `gemini -p` all take a prompt and the page's text, and answer JSON in a
//! shape this module dictates. Each runs with its tools off and nothing
//! persisted, from a scratch directory, with a time limit — it is asked a
//! question about a piece of text, not given a machine.
//!
//! **What leaves the machine is page 1, redacted.** A card statement lists
//! every purchase, the holder's CPF and address; the total, the due date and
//! the issuer are on the first page. CPF-shaped numbers and full card
//! numbers are masked before anything is sent ([`redact`]); a CNPJ is a
//! company's and stays.
//!
//! **Nothing it says is trusted until it is checked.** A pattern is kept only
//! when it finds, in the bill's own text, exactly the value the assistant
//! claims it finds ([`verify`]) — an assistant's `R\$\s*([\d.,]+)` happily
//! reads a card's credit limit as its total. And a person approves the
//! recipe before it is stored.
//!
//! Using a subscription this way is the subscriber's own use — a household's
//! tool on its own server. Serving other people's bills would need API keys.

use crate::domain::money::Cents;
use regex::Regex;
use serde::Deserialize;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Which CLI teaches recipes. `household_settings.assistant`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    None,
    Codex,
    Claude,
    Gemini,
}

impl Provider {
    pub const ALL: [Provider; 4] = [
        Provider::Codex,
        Provider::Claude,
        Provider::Gemini,
        Provider::None,
    ];

    pub fn parse(s: &str) -> Option<Provider> {
        Some(match s {
            "none" => Provider::None,
            "codex" => Provider::Codex,
            "claude" => Provider::Claude,
            "gemini" => Provider::Gemini,
            _ => return None,
        })
    }

    pub fn code(self) -> &'static str {
        match self {
            Provider::None => "none",
            Provider::Codex => "codex",
            Provider::Claude => "claude",
            Provider::Gemini => "gemini",
        }
    }

    fn program(self) -> Option<&'static str> {
        match self {
            Provider::None => None,
            Provider::Codex => Some("codex"),
            Provider::Claude => Some("claude"),
            Provider::Gemini => Some("gemini"),
        }
    }
}

/// What the assistant answered — every field as the bill prints it, and a
/// pattern for each that should find it again next month.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Proposal {
    pub issuer_name: Option<String>,
    pub cnpj: Option<String>,
    pub amount: Option<String>,
    pub due_date: Option<String>,
    pub client_code: Option<String>,
    pub card_last4: Option<String>,
    pub amount_regex: Option<String>,
    pub due_regex: Option<String>,
    pub client_regex: Option<String>,
    pub card_regex: Option<String>,
}

/// A proposal that survived [`verify`]: what it read, and only the patterns
/// that read it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recipe {
    pub issuer_name: Option<String>,
    /// 14 digits.
    pub cnpj: Option<String>,
    pub amount_regex: Option<String>,
    pub due_regex: Option<String>,
    pub client_regex: Option<String>,
    pub card_regex: Option<String>,
}

impl Recipe {
    pub fn is_empty(&self) -> bool {
        self.cnpj.is_none()
            && self.amount_regex.is_none()
            && self.due_regex.is_none()
            && self.client_regex.is_none()
            && self.card_regex.is_none()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AssistantError {
    /// The household chose none.
    Off,
    /// The CLI is not on this server's PATH.
    NotFound,
    TimedOut,
    /// It ran and failed — not logged in, out of quota. Its own words, for
    /// the log.
    Failed(String),
    /// It answered something that is not the JSON asked for.
    Unreadable(String),
}

impl std::fmt::Display for AssistantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssistantError::Off => f.write_str("no assistant is configured"),
            AssistantError::NotFound => {
                f.write_str("the assistant is not installed on this server")
            }
            AssistantError::TimedOut => f.write_str("the assistant did not answer in time"),
            AssistantError::Failed(e) => write!(f, "the assistant failed: {e}"),
            AssistantError::Unreadable(e) => {
                write!(f, "the assistant's answer was unreadable: {e}")
            }
        }
    }
}

/// How long a bill may take to read. A first page takes 15–20 s; past this
/// the person has waited long enough, and the bill is attached with what the
/// barcode said.
const TIMEOUT: Duration = Duration::from_secs(120);

const PROMPT: &str = "\
You read the FIRST PAGE of a Brazilian bill (a utility bill, a bank slip or a \
credit-card statement), in Portuguese, as text extracted from its PDF. Some \
numbers are masked on purpose. Answer only with the JSON object described.

Every value exactly as the page prints it, or null when the page does not \
show it:
- issuer_name: the company billing (not a merchant on a statement).
- cnpj: that company's CNPJ.
- amount: the total to pay this period — on a card statement the full \
  statement total, never the credit limit, the minimum payment or an \
  installment offer.
- due_date: the due date.
- client_code: the payer's account, contract or installation number, if the \
  page names one.
- card_last4: the last four digits of the holder's card, if printed.

And for each of amount, due_date, client_code and card_last4, a regex \
(Rust `regex` syntax) with EXACTLY ONE capture group that finds that value in \
this page and would find it again next month: anchor it on the label printed \
next to the value (e.g. `Total da fatura\\s+R\\$\\s*([\\d.]+,\\d{2})`), never \
on a bare `R\\$`, because the page holds other amounts. null when there is no \
value.

The page:
";

/// The JSON Schema every provider is held to. Every property present, `null`
/// allowed — the strict shape `codex --output-schema` requires.
fn schema() -> String {
    let fields = [
        "issuer_name",
        "cnpj",
        "amount",
        "due_date",
        "client_code",
        "card_last4",
        "amount_regex",
        "due_regex",
        "client_regex",
        "card_regex",
    ];
    let props: serde_json::Map<String, serde_json::Value> = fields
        .iter()
        .map(|f| {
            (
                f.to_string(),
                serde_json::json!({ "type": ["string", "null"] }),
            )
        })
        .collect();
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": props,
        "required": fields,
    })
    .to_string()
}

/// Asks the household's assistant to read a bill's first page. Blocking — run
/// it off the async runtime.
pub fn ask(provider: Provider, page_one: &str) -> Result<Proposal, AssistantError> {
    let program = provider.program().ok_or(AssistantError::Off)?;
    let dir = scratch_dir().map_err(|e| AssistantError::Failed(e.to_string()))?;
    let input = format!("{PROMPT}{}", redact(page_one));
    let schema = schema();
    let schema_file = dir.path.join("schema.json");
    let out_file = dir.path.join("answer.json");
    std::fs::write(&schema_file, &schema).map_err(|e| AssistantError::Failed(e.to_string()))?;

    let mut cmd = Command::new(program);
    cmd.current_dir(&dir.path);
    match provider {
        Provider::Codex => {
            cmd.args([
                "exec",
                "--skip-git-repo-check",
                "--ephemeral",
                "--color",
                "never",
            ])
            .args(["-s", "read-only", "--output-schema"])
            .arg(&schema_file)
            .arg("-o")
            .arg(&out_file)
            .arg("-");
        }
        Provider::Claude => {
            // Never `--bare`: it turns off the subscription login.
            cmd.args(["-p", "--output-format", "json", "--no-session-persistence"])
                .args(["--tools", ""])
                .arg("--json-schema")
                .arg(&schema);
        }
        Provider::Gemini => {
            cmd.args(["-o", "json", "-p", "Answer as instructed on stdin."]);
        }
        Provider::None => return Err(AssistantError::Off),
    }
    let out = run(cmd, &input)?;

    let answer = match provider {
        Provider::Codex => std::fs::read_to_string(&out_file)
            .map_err(|e| AssistantError::Unreadable(e.to_string()))?,
        Provider::Claude => {
            let v: serde_json::Value = serde_json::from_str(&out)
                .map_err(|e| AssistantError::Unreadable(e.to_string()))?;
            if v["is_error"].as_bool() == Some(true) {
                return Err(AssistantError::Failed(
                    v["result"].as_str().unwrap_or("error").to_string(),
                ));
            }
            match &v["structured_output"] {
                serde_json::Value::Null => v["result"].as_str().unwrap_or_default().to_string(),
                o => o.to_string(),
            }
        }
        Provider::Gemini => {
            let v: serde_json::Value = serde_json::from_str(&out)
                .map_err(|e| AssistantError::Unreadable(e.to_string()))?;
            v["response"].as_str().unwrap_or_default().to_string()
        }
        Provider::None => unreachable!(),
    };
    parse_answer(&answer)
}

/// The JSON object in an answer — a model asked for bare JSON sometimes wraps
/// it in a code fence anyway.
fn parse_answer(answer: &str) -> Result<Proposal, AssistantError> {
    let start = answer.find('{');
    let end = answer.rfind('}');
    let json = match (start, end) {
        (Some(s), Some(e)) if e > s => &answer[s..=e],
        _ => return Err(AssistantError::Unreadable("no JSON object".into())),
    };
    let mut p: Proposal =
        serde_json::from_str(json).map_err(|e| AssistantError::Unreadable(e.to_string()))?;
    // "" and null mean the same thing here.
    for f in [
        &mut p.issuer_name,
        &mut p.cnpj,
        &mut p.amount,
        &mut p.due_date,
        &mut p.client_code,
        &mut p.card_last4,
        &mut p.amount_regex,
        &mut p.due_regex,
        &mut p.client_regex,
        &mut p.card_regex,
    ] {
        if f.as_deref().is_some_and(|s| s.trim().is_empty()) {
            *f = None;
        }
    }
    Ok(p)
}

/// Runs the CLI with `input` on stdin, killing it at [`TIMEOUT`].
fn run(mut cmd: Command, input: &str) -> Result<String, AssistantError> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AssistantError::NotFound
            } else {
                AssistantError::Failed(e.to_string())
            }
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    // Read the pipes on their own threads, so a chatty CLI cannot fill one
    // and stall while this loop waits for it to exit.
    let stdout = child.stdout.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut b = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut b);
            b
        })
    });
    let stderr = child.stderr.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut b = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut b);
            b
        })
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AssistantError::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(AssistantError::Failed(e.to_string())),
        }
    };
    let out = stdout.and_then(|h| h.join().ok()).unwrap_or_default();
    let err = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
    if !status.success() {
        let why = if err.trim().is_empty() { &out } else { &err };
        return Err(AssistantError::Failed(
            why.trim().chars().take(400).collect(),
        ));
    }
    Ok(out)
}

/// Whether the household's assistant can be used on this server — for the
/// line under the setting in `:household`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Off,
    NotFound,
    /// Installed, but its own login check says no.
    LoggedOut,
    Ready,
    /// Installed; it has no login check to ask.
    Installed,
}

pub fn status(provider: Provider) -> Status {
    let Some(program) = provider.program() else {
        return Status::Off;
    };
    let quiet = |args: &[&str]| {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };
    if quiet(&["--version"]).is_err() {
        return Status::NotFound;
    }
    let login = match provider {
        Provider::Codex => quiet(&["login", "status"]),
        Provider::Claude => quiet(&["auth", "status"]),
        _ => return Status::Installed,
    };
    match login {
        Ok(s) if s.success() => Status::Ready,
        _ => Status::LoggedOut,
    }
}

/// Masks what identifies a person before a page leaves the machine: CPF-shaped
/// numbers, and full card numbers (all but their last four). A CNPJ — a
/// company's — is left as it is.
pub fn redact(text: &str) -> String {
    use std::sync::LazyLock;
    static CPF: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b\d{3}\.?\d{3}\.?\d{3}-?\d{2}\b").unwrap());
    static CARD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b(?:\d[ -]?){9,15}(\d{4})\b").unwrap());
    let text = CPF.replace_all(text, "***.***.***-**");
    CARD.replace_all(&text, "**** **** **** $1").into_owned()
}

/// A bill's first page as text, for the assistant: `pdftotext -f 1 -l 1`, or
/// the head of a text file.
pub fn page_one(path: &Path) -> Option<String> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("txt"))
    {
        let text = std::fs::read_to_string(path).ok()?;
        return Some(text.chars().take(6000).collect());
    }
    let out = Command::new("pdftotext")
        .args(["-f", "1", "-l", "1", "-layout"])
        .arg(path)
        .arg("-")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        .filter(|t| !t.trim().is_empty())
}

/// Keeps only what holds up: each pattern must compile, have exactly one
/// capture group, and find — first, in the bill's whole text — the very value
/// the assistant said it would. The CNPJ must be printed in the text. A field
/// that fails is dropped on its own; the rest stand.
pub fn verify(p: &Proposal, text: &str) -> Recipe {
    let finds = |pattern: &Option<String>, same: &dyn Fn(&str) -> bool| -> Option<String> {
        let pattern = pattern.as_deref()?;
        let re = Regex::new(pattern).ok()?;
        if re.captures_len() != 2 {
            return None;
        }
        let found = re.captures(text)?.get(1)?.as_str().trim().to_string();
        same(&found).then(|| pattern.to_string())
    };
    let money = |claimed: &Option<String>| {
        let claimed = claimed.as_deref().and_then(Cents::parse);
        move |found: &str| claimed.is_some() && Cents::parse(found) == claimed
    };
    let date = |claimed: &Option<String>| {
        let claimed = claimed
            .as_deref()
            .and_then(crate::services::issuers::parse_bill_date);
        move |found: &str| {
            claimed.is_some() && crate::services::issuers::parse_bill_date(found) == claimed
        }
    };
    let text_eq = |claimed: &Option<String>| {
        let claimed = claimed.as_deref().map(str::trim).map(str::to_string);
        move |found: &str| claimed.as_deref() == Some(found)
    };
    let last4 = |claimed: &Option<String>| {
        let claimed = claimed.as_deref().map(digits);
        move |found: &str| {
            let d = digits(found);
            d.len() >= 4 && claimed.as_deref() == Some(&d[d.len() - 4..])
        }
    };
    Recipe {
        issuer_name: p.issuer_name.clone(),
        cnpj: p
            .cnpj
            .as_deref()
            .map(digits)
            .filter(|c| crate::services::issuers::cnpj_valid(c) && cnpj_in(text, c)),
        amount_regex: finds(&p.amount_regex, &money(&p.amount)),
        due_regex: finds(&p.due_regex, &date(&p.due_date)),
        client_regex: finds(&p.client_regex, &text_eq(&p.client_code)),
        card_regex: finds(&p.card_regex, &last4(&p.card_last4)),
    }
}

fn digits(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}

/// Whether the 14 digits of a CNPJ are printed in the text, in any of the
/// ways a bill punctuates one — `18.236.120/0001-58`, `00.416.968\0001-01`,
/// or bare.
pub fn cnpj_in(text: &str, cnpj: &str) -> bool {
    let d: Vec<char> = cnpj.chars().collect();
    if d.len() != 14 {
        return false;
    }
    let s = |r: std::ops::Range<usize>| d[r].iter().collect::<String>();
    let pattern = format!(
        r"(?:^|\D){}\.?{}\.?{}[/\\]?{}-?{}(?:\D|$)",
        s(0..2),
        s(2..5),
        s(5..8),
        s(8..12),
        s(12..14)
    );
    Regex::new(&pattern).is_ok_and(|re| re.is_match(text))
}

/// A directory for one call's schema and answer, removed afterwards.
struct ScratchDir {
    path: std::path::PathBuf,
}

fn scratch_dir() -> std::io::Result<ScratchDir> {
    let mut raw = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut raw);
    let tag: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let path = std::env::temp_dir().join(format!("ffm-assistant-{tag}"));
    std::fs::create_dir_all(&path)?;
    Ok(ScratchDir { path })
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A statement shaped like a real card fatura, made up: a credit limit
    /// printed BEFORE the total, which a careless pattern reads instead.
    const PAGE: &str = "Olá, Fulana.\n  Limite total: R$ 20.000,00\n\
        Esta é a sua fatura de janeiro, no valor de\nR$ 1.234,56\n\
        Data de vencimento: 09 JAN 2026\nCARTÃO 1111****2222\n\
        CPF 123.456.789-09   Exemplo Pagamentos S.A. CNPJ 11.222.333/0001-81\n";

    fn proposal() -> Proposal {
        Proposal {
            issuer_name: Some("Exemplo Pagamentos S.A.".into()),
            cnpj: Some("11.222.333/0001-81".into()),
            amount: Some("R$ 1.234,56".into()),
            due_date: Some("09 JAN 2026".into()),
            client_code: None,
            card_last4: Some("2222".into()),
            amount_regex: Some(r"no valor de\s+R\$\s*([\d.]+,\d{2})".into()),
            due_regex: Some(r"Data de vencimento:\s*(\d{2} \p{Lu}{3} \d{4})".into()),
            client_regex: None,
            card_regex: Some(r"CART[AÃ]O\s+\d{4}\*{4}(\d{4})".into()),
        }
    }

    #[test]
    fn a_proposal_that_reads_what_it_claims_is_kept_whole() {
        let r = verify(&proposal(), PAGE);
        assert_eq!(r.cnpj.as_deref(), Some("11222333000181"));
        assert!(r.amount_regex.is_some());
        assert!(r.due_regex.is_some());
        assert!(r.card_regex.is_some());
        assert_eq!(r.client_regex, None);
    }

    #[test]
    fn a_pattern_that_reads_the_credit_limit_is_dropped() {
        // The first R$ on the page is the limit: the pattern "works", and
        // reads the wrong number.
        let p = Proposal {
            amount_regex: Some(r"R\$\s*([\d.,]+)".into()),
            ..proposal()
        };
        let r = verify(&p, PAGE);
        assert_eq!(r.amount_regex, None, "never offered");
        assert!(r.due_regex.is_some(), "the rest still stands");
    }

    #[test]
    fn a_pattern_must_compile_and_capture_exactly_one_value() {
        let p = Proposal {
            due_regex: Some(r"(Data) de vencimento:\s*(\d{2} \p{Lu}{3} \d{4})".into()),
            amount_regex: Some(r"no valor de\s+R\$\s*([\d.]+,\d{2}".into()),
            ..proposal()
        };
        let r = verify(&p, PAGE);
        assert_eq!(r.due_regex, None, "two groups");
        assert_eq!(r.amount_regex, None, "does not compile");
    }

    #[test]
    fn a_cnpj_must_be_on_the_page() {
        let p = Proposal {
            cnpj: Some("99.999.999/0001-99".into()),
            ..proposal()
        };
        assert_eq!(verify(&p, PAGE).cnpj, None);
        assert!(cnpj_in(
            "BANCO INTER S/A CNPJ: 00.416.968\\0001-01",
            "00416968000101"
        ));
        assert!(cnpj_in("cnpj 00416968000101.", "00416968000101"));
        assert!(
            !cnpj_in("100416968000101", "00416968000101"),
            "inside a longer number"
        );
    }

    #[test]
    fn what_identifies_a_person_never_leaves_the_machine() {
        let out = redact(
            "CPF 123.456.789-09 · cpf 12345678909 · cartão 5364 1234 5678 7766 \
             · CNPJ 18.236.120/0001-58 · masked 5364****7766",
        );
        assert!(!out.contains("123.456.789-09") && !out.contains("12345678909"));
        assert!(out.contains("**** **** **** 7766"), "{out}");
        assert!(!out.contains("5364 1234 5678"));
        assert!(out.contains("18.236.120/0001-58"), "a company's CNPJ stays");
        assert!(out.contains("5364****7766"), "already masked stays");
    }

    #[test]
    fn an_answer_in_a_code_fence_still_reads_and_blank_means_none() {
        let p = parse_answer("```json\n{\"issuer_name\": \"X\", \"cnpj\": \"\"}\n```").unwrap();
        assert_eq!(p.issuer_name.as_deref(), Some("X"));
        assert_eq!(p.cnpj, None);
        assert!(parse_answer("sorry, I cannot help").is_err());
    }

    #[test]
    fn the_schema_holds_every_field_and_allows_null() {
        let s: serde_json::Value = serde_json::from_str(&schema()).unwrap();
        assert_eq!(s["required"].as_array().unwrap().len(), 10);
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["properties"]["amount"]["type"][1], "null");
    }

    /// The real CLIs, on the made-up page above — opt-in, since it spends the
    /// household's subscription: `FFM_ASSISTANT_LIVE=codex,claude cargo test
    /// assistant -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn the_real_assistants_read_a_made_up_page() {
        let wanted = std::env::var("FFM_ASSISTANT_LIVE").unwrap_or_default();
        for code in wanted.split(',').filter(|c| !c.is_empty()) {
            let provider = Provider::parse(code).expect("codex | claude | gemini");
            let started = Instant::now();
            let p = ask(provider, PAGE).unwrap_or_else(|e| panic!("{code}: {e}"));
            let r = verify(&p, PAGE);
            eprintln!(
                "{code} · {:.1}s · {p:?}\n  kept: {r:?}",
                started.elapsed().as_secs_f32()
            );
            assert_eq!(r.cnpj.as_deref(), Some("11222333000181"), "{code}");
            assert!(
                r.amount_regex.is_some(),
                "{code}: no amount pattern held up"
            );
        }
    }

    #[test]
    fn no_assistant_is_no_call() {
        assert_eq!(ask(Provider::None, PAGE), Err(AssistantError::Off));
        assert_eq!(status(Provider::None), Status::Off);
    }
}
