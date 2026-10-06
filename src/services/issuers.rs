//! Issuer profiles — the small amount of per-biller config that is genuinely
//! necessary.
//!
//! The barcode structure is universal, so a profile declares rules **only for
//! the holes the barcode leaves open**:
//!
//! 1. Who is this? — never in the barcode.
//! 2. The due date, *only* when the barcode carries none.
//! 3. The amount, *only* when it is `Open` or `Reference`.
//! 4. The client code — the subscriber's own account number, which FEBRABAN
//!    never encodes because the barcode's job is value and due date, not who
//!    is paying.
//!
//! **If the barcode answered it, the profile must not restate it.** That is
//! not tidiness: a profile that can override a checksummed number is a profile
//! that can quietly post the wrong one.
//!
//! **The one exception is a bill with no barcode at all**, from an issuer whose
//! profile says it sends those (`barcode = "optional"` — Nubank omits it to
//! push payment in its app). There is no checksummed number to defer to, so
//! [`resolve_text`] reads the amount and the due date off the page, and the
//! CNPJ that detected the issuer is what vouches for the bill.

use crate::domain::money::Cents;
use crate::services::boleto::{Amount, Barcode, DueDate, Kind};
use chrono::NaiveDate;
use regex::Regex;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Debug, Default)]
pub struct Profiles {
    #[serde(default, rename = "issuer")]
    pub issuers: Vec<Issuer>,
}

#[derive(Deserialize, Debug)]
pub struct Issuer {
    pub id: String,
    pub name: String,
    /// Matched against the bill's text. **CNPJ, not the trade name** —
    /// layouts get redesigned and brands get renamed; the number in the
    /// footer does not move.
    pub detect: Vec<String>,
    /// The barcode kind this issuer normally uses. A mismatch is a warning,
    /// not a failure: it means the layout changed, and the barcode still
    /// parsed.
    #[serde(default)]
    pub kind: Option<String>,
    /// A hint for the Bucket / Bill name of a rule created from a recognized
    /// issuer. Never used to bind a file to an existing rule — that binding is
    /// always explicit, in the rule's own editor.
    #[serde(default)]
    pub recurring: Option<String>,
    #[serde(default)]
    pub due_date: Option<Rule>,
    #[serde(default)]
    pub amount: Option<Rule>,
    #[serde(default)]
    pub client_code: Option<Rule>,
    /// Whether this issuer may send a bill with no barcode at all. Opt-in:
    /// for any other issuer a barcode-less file is still "no barcode", never
    /// a total guessed from the page.
    #[serde(default)]
    pub barcode: BarcodeNeed,
    /// The card's last four digits, read from **every** match: one statement
    /// can list the holder's card and an extra one. A rule binds the first and
    /// a later bill agrees when its statement lists it anywhere. Omitted for an
    /// issuer whose printed digits rotate (Nubank's per-purchase virtual
    /// cards), where it would only ever disagree.
    #[serde(default)]
    pub card: Option<Rule>,
}

/// Whether a profile's bills always carry a barcode.
#[derive(Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BarcodeNeed {
    #[default]
    Required,
    Optional,
}

/// One hole-filling rule: when it applies, and the pattern that fills it.
#[derive(Deserialize, Debug)]
pub struct Rule {
    /// `absent` / `open` — the condition under which the barcode left this
    /// hole. Omitted means "always", which is right only for the client code,
    /// since the barcode never carries one.
    #[serde(default)]
    pub when: Option<String>,
    pub regex: String,
}

#[derive(Debug)]
pub enum LoadError {
    Io(String),
    Toml(String),
    /// A pattern that does not compile is a config error the operator can fix,
    /// and it names the issuer so they know which line.
    Regex {
        issuer: String,
        problem: String,
    },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "✗ config/issuers.toml — {e}"),
            LoadError::Toml(e) => write!(f, "✗ config/issuers.toml is not valid TOML: {e}"),
            LoadError::Regex { issuer, problem } => {
                write!(
                    f,
                    "✗ issuer `{issuer}` has a pattern that does not compile: {problem}"
                )
            }
        }
    }
}

impl Profiles {
    /// Reads the file, or returns an empty set when there is none.
    ///
    /// A household with no profiles still gets every number the barcode
    /// carries — which is most of them. Profiles only ever add what the
    /// barcode could not answer.
    pub fn load(path: &Path) -> Result<Profiles, LoadError> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Profiles::default()),
            Err(e) => return Err(LoadError::Io(e.to_string())),
        };
        let profiles: Profiles =
            toml::from_str(&raw).map_err(|e| LoadError::Toml(e.to_string()))?;

        // Compiled once here rather than per bill, and a bad pattern is
        // reported with the issuer that owns it.
        for issuer in &profiles.issuers {
            for pattern in issuer
                .detect
                .iter()
                .chain(issuer.due_date.iter().map(|r| &r.regex))
                .chain(issuer.amount.iter().map(|r| &r.regex))
                .chain(issuer.client_code.iter().map(|r| &r.regex))
                .chain(issuer.card.iter().map(|r| &r.regex))
            {
                Regex::new(pattern).map_err(|e| LoadError::Regex {
                    issuer: issuer.id.clone(),
                    problem: e.to_string(),
                })?;
            }
        }
        Ok(profiles)
    }

    /// The first issuer whose `detect` matches the bill's text.
    pub fn detect(&self, text: &str) -> Option<&Issuer> {
        self.issuers.iter().find(|i| {
            i.detect
                .iter()
                .any(|p| Regex::new(p).is_ok_and(|re| re.is_match(text)))
        })
    }
}

/// What the import ended up knowing, and where each part came from.
///
/// The sources are recorded because a wrong number has to be auditable later:
/// "the barcode said so" and "a regex found it in the text" are different
/// kinds of confidence, and the row keeps which one it was.
#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    pub issuer: Option<String>,
    pub issuer_name: Option<String>,
    pub amount: Option<Cents>,
    pub amount_source: Option<&'static str>,
    pub due: Option<NaiveDate>,
    pub due_source: Option<&'static str>,
    pub client_code: Option<String>,
    /// The cards' last four digits, in the order the statement lists them,
    /// each once. Empty when the profile reads none.
    pub cards: Vec<String>,
    /// The barcode kind the issuer's profile declares — what a bill with no
    /// barcode is taken to be, for the binding check.
    pub issuer_kind: Option<String>,
    /// Every CNPJ printed in the bill's text, 14 digits each, in order, each
    /// once — read without any profile. A rule bound to an issuer's CNPJ
    /// agrees with a bill that prints it anywhere.
    pub cnpjs: Vec<String>,
    /// Why this still needs a person. Empty means it does not.
    pub unresolved: Vec<&'static str>,
}

/// Every CNPJ printed in a text — `18.236.120/0001-58`, `00.416.968\0001-01`
/// or bare — as 14 digits, in order, each once.
pub fn cnpjs_in(text: &str) -> Vec<String> {
    use std::sync::LazyLock;
    static CNPJ: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:^|\D)(\d{2}\.?\d{3}\.?\d{3}[/\\]?\d{4}-?\d{2})(?:\D|$)").unwrap()
    });
    let mut out: Vec<String> = Vec::new();
    for c in CNPJ.captures_iter(text) {
        let d: String = c[1].chars().filter(char::is_ascii_digit).collect();
        if cnpj_valid(&d) && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// A CNPJ's own two check digits (mod 11, weights 5…2 9…2 then 6…2 9…2).
/// What tells a company's number apart from any other run of 14 digits on a
/// bill — a barcode's fourteen zeros, an installation number.
pub fn cnpj_valid(d: &str) -> bool {
    let n: Vec<u32> = d.chars().filter_map(|c| c.to_digit(10)).collect();
    if n.len() != 14 || n.iter().all(|&x| x == n[0]) {
        return false;
    }
    let check = |len: usize| {
        let weights = [6u32, 5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
        let w = &weights[13 - len..];
        let sum: u32 = n[..len].iter().zip(w).map(|(a, b)| a * b).sum();
        let r = sum % 11;
        if r < 2 { 0 } else { 11 - r }
    };
    check(12) == n[12] && check(13) == n[13]
}

impl Issuer {
    /// A rule's own recipe as a one-issuer profile, so the pipeline that reads
    /// every bill reads this rule's with it. Detected by the recipe's CNPJ —
    /// or, with none, always: the rule is already the one the person chose.
    /// `optional`: a rule that learned to read a page can read one with no
    /// barcode on it.
    #[allow(clippy::too_many_arguments)]
    pub fn from_recipe(
        cnpj: Option<&str>,
        name: Option<&str>,
        kind: Option<&str>,
        amount: Option<&str>,
        due: Option<&str>,
        client: Option<&str>,
        card: Option<&str>,
    ) -> Issuer {
        let rule = |r: Option<&str>, when: Option<&str>| {
            r.map(|regex| Rule {
                when: when.map(str::to_string),
                regex: regex.to_string(),
            })
        };
        let detect = match cnpj.filter(|c| c.len() == 14) {
            Some(c) => vec![format!(
                r"(?:^|\D){}\.?{}\.?{}[/\\]?{}-?{}(?:\D|$)",
                &c[0..2],
                &c[2..5],
                &c[5..8],
                &c[8..12],
                &c[12..14]
            )],
            None => vec![r"(?s).".to_string()],
        };
        Issuer {
            id: cnpj.unwrap_or("rule").to_string(),
            name: name.unwrap_or_default().to_string(),
            detect,
            kind: kind.map(str::to_string),
            recurring: None,
            due_date: rule(due, Some("absent")),
            amount: rule(amount, Some("open")),
            client_code: rule(client, None),
            barcode: BarcodeNeed::Optional,
            card: rule(card, None),
        }
    }
}

/// Fills the holes the barcode left, and **only** those.
pub fn resolve(barcode: &Barcode, text: &str, profiles: &Profiles) -> Import {
    let issuer = profiles.detect(text);
    let mut unresolved = Vec::new();

    // The barcode is checksummed, so it wins outright wherever it spoke.
    let (amount, amount_source) = match barcode.amount {
        Amount::Fixed(c) => (Some(c), Some("barcode")),
        // The payer chooses, or the digits are not currency at all. Either way
        // the profile may know how to read the real number off the page.
        Amount::Open | Amount::Reference(_) => {
            match issuer
                .and_then(|i| i.amount.as_ref())
                .and_then(|r| find_money(&r.regex, text))
            {
                Some(c) => (Some(c), Some("text")),
                None => {
                    unresolved.push("amount");
                    (None, None)
                }
            }
        }
    };

    let (due, due_source) = match barcode.due {
        DueDate::Encoded(d) => (Some(d), Some("fator")),
        DueDate::FreeField(d) => (Some(d), Some("free_field")),
        DueDate::Absent => {
            match issuer
                .and_then(|i| i.due_date.as_ref())
                .and_then(|r| find_date(&r.regex, text))
            {
                Some(d) => (Some(d), Some("text")),
                None => {
                    unresolved.push("due date");
                    (None, None)
                }
            }
        }
    };

    if issuer.is_none() {
        unresolved.push("issuer");
    }
    // A layout change, not a failure: the barcode still parsed and its numbers
    // are still checksummed.
    if let Some(i) = issuer
        && let Some(expected) = i.kind.as_deref()
        && expected != barcode.kind.code()
    {
        unresolved.push("kind mismatch");
    }

    Import {
        issuer: issuer.map(|i| i.id.clone()),
        issuer_name: issuer.map(|i| i.name.clone()),
        amount,
        amount_source,
        due,
        due_source,
        // Always a text read: the barcode's job is value and due date, not who
        // is paying.
        client_code: issuer
            .and_then(|i| i.client_code.as_ref())
            .and_then(|r| find_first_group(&r.regex, text)),
        cards: issuer.map(|i| cards(i, text)).unwrap_or_default(),
        issuer_kind: issuer.and_then(|i| i.kind.clone()),
        cnpjs: cnpjs_in(text),
        unresolved,
    }
}

/// A bill with **no barcode**, read entirely from its text — only for an
/// issuer whose profile says it sends those (`barcode = "optional"`).
///
/// There is no checksummed number to defer to, so the amount and due-date
/// rules apply whatever their `when` says, and the sources are `text`. The
/// amount is required: a bill this app cannot put a figure on is not one it
/// can read. `Err` carries the detected issuer's name, if any, so the refusal
/// can say who it was.
pub fn resolve_text(text: &str, profiles: &Profiles) -> Result<Import, Option<String>> {
    let issuer = profiles.detect(text);
    let Some(i) = issuer.filter(|i| i.barcode == BarcodeNeed::Optional) else {
        return Err(issuer.map(|i| i.name.clone()));
    };
    let amount = i
        .amount
        .as_ref()
        .and_then(|r| find_money(&r.regex, text))
        .ok_or_else(|| Some(i.name.clone()))?;
    let due = i.due_date.as_ref().and_then(|r| find_date(&r.regex, text));
    let mut unresolved = Vec::new();
    if due.is_none() {
        unresolved.push("due date");
    }
    Ok(Import {
        issuer: Some(i.id.clone()),
        issuer_name: Some(i.name.clone()),
        amount: Some(amount),
        amount_source: Some("text"),
        due,
        due_source: due.map(|_| "text"),
        client_code: i
            .client_code
            .as_ref()
            .and_then(|r| find_first_group(&r.regex, text)),
        cards: cards(i, text),
        issuer_kind: i.kind.clone(),
        cnpjs: cnpjs_in(text),
        unresolved,
    })
}

/// Every card the statement lists, last four digits, in order, each once.
fn cards(issuer: &Issuer, text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for found in issuer
        .card
        .as_ref()
        .map(|r| find_all_groups(&r.regex, text))
        .unwrap_or_default()
    {
        let digits: String = found.chars().filter(char::is_ascii_digit).collect();
        if digits.len() < 4 {
            continue;
        }
        let last4 = digits[digits.len() - 4..].to_string();
        if !out.contains(&last4) {
            out.push(last4);
        }
    }
    out
}

fn find_all_groups(pattern: &str, text: &str) -> Vec<String> {
    let Ok(re) = Regex::new(pattern) else {
        return Vec::new();
    };
    re.captures_iter(text)
        .filter_map(|c| c.get(1).or_else(|| c.get(0)))
        .map(|m| m.as_str().trim().to_string())
        .collect()
}

fn find_first_group(pattern: &str, text: &str) -> Option<String> {
    let re = Regex::new(pattern).ok()?;
    let caps = re.captures(text)?;
    // The first capture group when there is one, the whole match otherwise —
    // so a profile can write either shape without a flag to say which.
    caps.get(1)
        .or_else(|| caps.get(0))
        .map(|m| m.as_str().trim().to_string())
}

fn find_date(pattern: &str, text: &str) -> Option<NaiveDate> {
    parse_bill_date(&find_first_group(pattern, text)?)
}

/// A date as a Brazilian bill prints it: `09/01/2026`, or `09 JAN 2026` with
/// the Portuguese month abbreviation (Nubank). This reads *input* and stays
/// pt-BR whatever the member's language — the app only ever *writes*
/// dd/mm/yyyy. By hand rather than through a locale: chrono carries none here,
/// on purpose.
pub fn parse_bill_date(s: &str) -> Option<NaiveDate> {
    if let Ok(d) = NaiveDate::parse_from_str(s.trim(), "%d/%m/%Y") {
        return Some(d);
    }
    const MONTHS: [&str; 12] = [
        "JAN", "FEV", "MAR", "ABR", "MAI", "JUN", "JUL", "AGO", "SET", "OUT", "NOV", "DEZ",
    ];
    let mut parts = s
        .split(|c: char| c.is_whitespace() || c == '/' || c == '-')
        .filter(|p| !p.is_empty());
    let day: u32 = parts.next()?.parse().ok()?;
    let month = parts.next()?.to_uppercase();
    let month = MONTHS.iter().position(|m| *m == month)? as u32 + 1;
    let year: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    NaiveDate::from_ymd_opt(year, month, day)
}

fn find_money(pattern: &str, text: &str) -> Option<Cents> {
    // Reuses the parser the amount field uses, so a pasted "R$ 1.234,56" and a
    // scraped one mean the same thing.
    Cents::parse(&find_first_group(pattern, text)?)
}

/// Whether a bill's kind matches what its issuer usually sends — for the
/// warning, not for a refusal.
pub fn kind_matches(issuer: &Issuer, kind: Kind) -> bool {
    issuer.kind.as_deref().is_none_or(|k| k == kind.code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::boleto;

    const TOML: &str = r#"
[[issuer]]
id      = "enel-sp"
name    = "Energia · Enel SP"
detect  = ['\b12\.?345\.?678/?0001-?99\b']
kind    = "consumo"
recurring = "energia"

[issuer.due_date]
when  = "absent"
regex = 'Vencimento\s*(\d{2}/\d{2}/\d{4})'

[issuer.client_code]
regex = 'N[uú]mero do cliente:?\s*(\d[\d-]+)'

[[issuer]]
id     = "cartao"
name   = "Cartão"
detect = ['\bFatura do cart[aã]o\b']

[issuer.amount]
when  = "open"
regex = 'Total da fatura\s*R\$\s*([\d.,]+)'
"#;

    fn profiles() -> Profiles {
        toml::from_str(TOML).unwrap()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 21).unwrap()
    }

    #[test]
    fn an_issuer_is_detected_by_its_cnpj_not_its_name() {
        // Layouts get redesigned and brands get renamed; the number in the
        // footer does not move.
        let p = profiles();
        assert_eq!(
            p.detect("CNPJ 12.345.678/0001-99 — ENEL DISTRIBUIÇÃO")
                .map(|i| i.id.as_str()),
            Some("enel-sp")
        );
        // The same company after a rebrand, same CNPJ.
        assert_eq!(
            p.detect("12345678000199 · Nova Marca S.A.")
                .map(|i| i.id.as_str()),
            Some("enel-sp")
        );
        assert!(p.detect("some other bill").is_none());
    }

    #[test]
    fn the_barcode_wins_wherever_it_spoke() {
        // A profile that can override a checksummed number is a profile that
        // can quietly post the wrong one.
        let b = boleto::parse("836600000035436212342022607150000000000000000000", today()).unwrap();
        let text = "CNPJ 12.345.678/0001-99\nVencimento 01/01/2001\nNúmero do cliente: 987654-3";
        let got = resolve(&b, text, &profiles());

        assert_eq!(got.amount, Some(Cents(34362)));
        assert_eq!(got.amount_source, Some("barcode"));
        // The profile's due-date regex is `when = "absent"` and the free field
        // carried one, so the text's 01/01/2001 must NOT win.
        assert_eq!(got.due, NaiveDate::from_ymd_opt(2026, 7, 15));
        assert_eq!(got.due_source, Some("free_field"));
        assert!(got.unresolved.is_empty());
    }

    #[test]
    fn the_client_code_is_always_a_text_read() {
        // FEBRABAN never encodes it: the barcode's job is value and due date,
        // not who is paying.
        let b = boleto::parse("836600000035436212342022607150000000000000000000", today()).unwrap();
        let text = "CNPJ 12.345.678/0001-99\nNúmero do cliente: 987654-3";
        assert_eq!(
            resolve(&b, text, &profiles()).client_code.as_deref(),
            Some("987654-3")
        );
    }

    #[test]
    fn an_open_amount_is_filled_from_the_page_when_the_profile_knows_how() {
        // The card fatura case: the barcode carries zeros because the payer
        // chooses, and the profile knows where the full total is printed.
        let b = boleto::parse("26091234560000123450900012345005115950000000000", today()).unwrap();
        let text = "Fatura do cartão\nTotal da fatura R$ 1.234,56";
        let got = resolve(&b, text, &profiles());
        assert_eq!(got.amount, Some(Cents(123456)));
        assert_eq!(got.amount_source, Some("text"), "and the row records WHICH");
        assert!(got.unresolved.is_empty());
    }

    #[test]
    fn what_nobody_can_answer_is_listed_rather_than_guessed() {
        let b = boleto::parse("26091234560000123450900012345005115950000000000", today()).unwrap();
        let got = resolve(&b, "a bill from nobody we know", &profiles());
        assert_eq!(got.amount, None);
        assert!(got.unresolved.contains(&"amount"));
        assert!(got.unresolved.contains(&"issuer"));
    }

    #[test]
    fn a_kind_mismatch_is_a_warning_not_a_refusal() {
        // The barcode still parsed and its numbers are still checksummed; what
        // changed is the layout.
        let b = boleto::parse("34191790010104351004779102015009815700000034362", today()).unwrap();
        let got = resolve(&b, "CNPJ 12.345.678/0001-99", &profiles());
        assert!(got.unresolved.contains(&"kind mismatch"));
        assert_eq!(
            got.amount,
            Some(Cents(34362)),
            "the amount is still trusted"
        );
    }

    #[test]
    fn only_a_real_cnpj_is_read_as_one() {
        assert!(cnpj_valid("18236120000158"), "Nu Pagamentos");
        assert!(cnpj_valid("00416968000101"), "Banco Inter");
        assert!(!cnpj_valid("00000000000000"), "a barcode's zeros");
        assert!(!cnpj_valid("18236120000159"), "one digit off");
        // A linha digitável's fourteen zeros sit right beside the real one.
        let text = "07790.00116 01001.305208 01098.430570 3 00000000000000\n\
                    BANCO INTER S/A CNPJ: 00.416.968\\0001-01";
        assert_eq!(cnpjs_in(text), vec!["00416968000101".to_string()]);
    }

    #[test]
    fn a_bill_date_reads_in_both_shapes_brazilian_bills_print() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day);
        assert_eq!(parse_bill_date("09/01/2026"), d(2026, 1, 9));
        assert_eq!(parse_bill_date("09 JAN 2026"), d(2026, 1, 9));
        assert_eq!(parse_bill_date("15 mai 2026"), d(2026, 5, 15));
        assert_eq!(parse_bill_date("31 DEZ 2025"), d(2025, 12, 31));
        assert_eq!(parse_bill_date("09 XYZ 2026"), None);
        assert_eq!(parse_bill_date("31 FEV 2026"), None, "no such day");
    }

    const OPTIONAL: &str = r#"
[[issuer]]
id      = "cartao-sem-codigo"
name    = "Cartão · Exemplo"
detect  = ['\b11\.?222\.?333/?0001-?81\b']
kind    = "cobranca"
barcode = "optional"

[issuer.amount]
when  = "open"
regex = 'no valor de\s+R\$\s*([\d.]+,\d{2})'

[issuer.due_date]
when  = "absent"
regex = 'Vencimento:\s*(\d{2}\s+\p{Lu}{3}\s+\d{4})'

[issuer.card]
regex = 'CART[AÃ]O\s+\d{4}\*{4}(\d{4})'
"#;

    #[test]
    fn a_bill_with_no_barcode_is_read_from_its_text_when_the_issuer_sends_those() {
        let p: Profiles = toml::from_str(OPTIONAL).unwrap();
        let text = "CNPJ 11.222.333/0001-81\nfatura no valor de R$ 1.234,56\n\
                    Vencimento: 09 JAN 2026\nCARTÃO 1111****2222\nCARTÃO 3333****4444\n\
                    CARTÃO 1111****2222";
        let got = resolve_text(text, &p).unwrap();
        assert_eq!(got.issuer.as_deref(), Some("cartao-sem-codigo"));
        // `when` is ignored: there is no barcode to defer to.
        assert_eq!(got.amount, Some(Cents(123456)));
        assert_eq!(got.amount_source, Some("text"));
        assert_eq!(got.due, NaiveDate::from_ymd_opt(2026, 1, 9));
        assert_eq!(got.due_source, Some("text"));
        // Every card, in order, each once.
        assert_eq!(got.cards, vec!["2222".to_string(), "4444".to_string()]);
        assert_eq!(got.issuer_kind.as_deref(), Some("cobranca"));
    }

    #[test]
    fn only_an_issuer_that_opted_in_is_read_without_a_barcode() {
        // Enel's profile says nothing about it, so its total is never guessed
        // from the page — the refusal names who it was.
        let text = "CNPJ 12.345.678/0001-99\nTotal da fatura R$ 343,62";
        assert_eq!(
            resolve_text(text, &profiles()).unwrap_err().as_deref(),
            Some("Energia · Enel SP")
        );
        assert_eq!(resolve_text("nobody we know", &profiles()), Err(None));
        // An opted-in issuer whose amount cannot be found is not a bill this
        // app can read either.
        let p: Profiles = toml::from_str(OPTIONAL).unwrap();
        assert!(resolve_text("CNPJ 11.222.333/0001-81, no total here", &p).is_err());
    }

    #[test]
    fn an_unknown_barcode_need_is_a_config_error() {
        let bad = "[[issuer]]\nid = \"x\"\nname = \"x\"\ndetect = ['x']\nbarcode = \"maybe\"\n";
        assert!(toml::from_str::<Profiles>(bad).is_err());
    }

    /// Nubank's and Inter's recipes as the conversion of the retired
    /// `config/issuers.toml` gives them — over statements shaped like the real
    /// ones: the issuers' real CNPJs, made-up amounts and cards.
    fn nubank() -> Profiles {
        Profiles {
            issuers: vec![Issuer::from_recipe(
                Some("18236120000158"),
                Some("Cartão · Nubank"),
                Some("cobranca"),
                Some(r"no valor de\s+R\$\s*([\d.]+,\d{2})"),
                Some(r"Data de vencimento:\s*(\d{2}\s+\p{Lu}{3}\s+\d{4})"),
                None,
                None,
            )],
        }
    }

    fn inter() -> Profiles {
        Profiles {
            issuers: vec![Issuer::from_recipe(
                Some("00416968000101"),
                Some("Cartão · Inter"),
                Some("cobranca"),
                Some(r"Fatura atual\s+R\$\s*([\d.]+,\d{2})"),
                Some(r"\d{4}\*{4}\d{4}\s+(\d{2}/\d{2}/\d{4})"),
                None,
                Some(r"CART[AÃ]O\s+\d{4}\*{4}(\d{4})"),
            )],
        }
    }

    #[test]
    fn a_nubank_recipe_reads_a_fatura_with_no_barcode() {
        let text = "Olá, Fulana.\nEsta é a sua fatura de\njaneiro, no valor de\nR$ 1.234,56\n\n\n\
                    Data de vencimento: 09 JAN 2026\n\x0c       FULANA DE TAL\n\
                    Nu Pagamentos S.A.     CNPJ 18.236.120/0001-58\n\
                    Total a pagar   R$ 9.999,99\n";
        let got = resolve_text(text, &nubank()).unwrap();
        assert_eq!(got.issuer.as_deref(), Some("18236120000158"));
        assert_eq!(got.amount, Some(Cents(123456)), "the headline figure");
        assert_eq!(got.due, NaiveDate::from_ymd_opt(2026, 1, 9));
        assert!(got.cards.is_empty(), "rotating virtual cards are not read");
    }

    #[test]
    fn an_inter_recipe_reads_the_total_not_the_limit() {
        let b = boleto::parse("26091234560001234500501234500005500000000000000", today()).unwrap();
        let text = "         Total da sua fatura\n                              R$ 23.500,00\n\
                    \n         R$ 321,00                          Data de Vencimento\n\
                    \x0c                 FULANA DE TAL\n\
                    1111****2222           07/01/2026              R$ 321,00\n\
                    Fatura atual                                   R$ 321,00\n\
                    CARTÃO 1111****2222\n CARTÃO 1111****3333\n\
                    BENEFICIÁRIO   BANCO INTER S/A CNPJ: 00.416.968\\0001-01\n";
        let got = resolve(&b, text, &inter());
        assert_eq!(
            got.issuer.as_deref(),
            Some("00416968000101"),
            "the backslash CNPJ"
        );
        assert_eq!(
            got.amount,
            Some(Cents(32100)),
            "never the R$ 23.500,00 limit"
        );
        assert_eq!(got.due, NaiveDate::from_ymd_opt(2026, 1, 7));
        assert_eq!(got.cards, vec!["2222".to_string(), "3333".to_string()]);
    }

    #[test]
    fn a_missing_config_file_is_not_an_error() {
        // A household with no profiles still gets every number the barcode
        // carries, which is most of them.
        let p = Profiles::load(Path::new("/nonexistent/issuers.toml")).unwrap();
        assert!(p.issuers.is_empty());
    }

    #[test]
    fn a_pattern_that_does_not_compile_names_the_issuer_that_owns_it() {
        let dir = std::env::temp_dir().join("ffm-issuers-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("issuers.toml");
        std::fs::write(
            &path,
            "[[issuer]]\nid = \"broken\"\nname = \"x\"\ndetect = ['(unclosed']\n",
        )
        .unwrap();

        let e = Profiles::load(&path).unwrap_err();
        assert!(e.to_string().contains("broken"), "{e}");
    }
}
