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
    /// Why this still needs a person. Empty means it does not.
    pub unresolved: Vec<&'static str>,
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
        unresolved,
    }
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
    let found = find_first_group(pattern, text)?;
    // dd/mm/yyyy is the only shape a Brazilian bill prints, and the only one
    // this app writes.
    NaiveDate::parse_from_str(&found, "%d/%m/%Y").ok()
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
