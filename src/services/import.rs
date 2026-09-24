//! The extraction pipeline: a file in, a filled-in bill out.
//!
//! Run in order, stopping as soon as every field is resolved:
//!
//! | tier | step | confidence |
//! |---|---|---|
//! | 0 | `pdftotext -layout` | — |
//! | 1 | find 44/47/48-digit runs in the text | — |
//! | 2 | classify, validate the check digits, parse | **certain** |
//! | 3 | issuer `detect` against the text | high |
//! | 4 | profile regexes for what is left | medium |
//! | 5 | OCR, then retry 1–4 | low |
//!
//! Tier 5 is deliberately last and deliberately optional: most utility bills
//! are digital PDFs with a real text layer, and OCR is for scans and photos.
//! It is not implemented here — the hook is, so adding it later does not mean
//! rearranging the tiers.

use crate::services::boleto::{self, Barcode};
use crate::services::issuers::{self, Import, Profiles};
use chrono::NaiveDate;
use regex::Regex;
use std::path::Path;
use std::process::Command;

/// What came out, and how far it got.
#[derive(Debug)]
pub struct Parsed {
    pub barcode: Barcode,
    pub import: Import,
}

#[derive(Debug)]
pub enum ImportError {
    /// No text at all — a scan or a photo. This is where OCR would go.
    NoText,
    /// Text, but nothing in it looks like a barcode.
    NoBarcode,
    /// Every candidate failed its own check digits. **Not** a cue to read the
    /// amount off the page: a number that failed its checksum is a misread,
    /// and reporting it is the only honest move.
    Misread(boleto::Error),
    Tool(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::NoText => write!(f, "this file has no text to read — a scan needs OCR"),
            ImportError::NoBarcode => write!(f, "no barcode found in this bill"),
            ImportError::Misread(e) => write!(f, "{e}"),
            ImportError::Tool(e) => write!(f, "{e}"),
        }
    }
}

/// Tier 0. A subprocess, not a library: `pdftotext` is a binary that already
/// handles the common case, and a PDF parser in-process is a large dependency
/// for something the operator's machine almost certainly already has.
pub fn text_of(path: &Path) -> Result<String, ImportError> {
    // `.txt` is already text; a household filing a bill by hand may well have
    // one, and shelling out to read it would be silly.
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("txt"))
    {
        return std::fs::read_to_string(path).map_err(|e| ImportError::Tool(e.to_string()));
    }

    let out = Command::new("pdftotext")
        .arg("-layout")
        .arg(path)
        .arg("-")
        .output()
        .map_err(|e| ImportError::Tool(format!("pdftotext could not run: {e}")))?;

    if !out.status.success() {
        return Err(ImportError::Tool(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if text.trim().is_empty() {
        return Err(ImportError::NoText);
    }
    Ok(text)
}

/// Tier 1 — every run of digits long enough to be a barcode, with the
/// separators a bill prints removed.
///
/// A linha digitável is printed broken into blocks, so runs are joined across
/// the characters that do the breaking before the lengths are checked.
pub fn candidates(text: &str) -> Vec<String> {
    // Digits and the separators a bill prints between them — but **not** a
    // newline. A linha digitável is printed on one line, and letting a run
    // cross lines swallows whatever number sits above it: on a real Enel bill
    // that is the client code, and the joined digits then parse as nothing.
    let re = Regex::new(r"[0-9][0-9.\t -]{42,}[0-9]").expect("a constant pattern");
    let mut out = Vec::new();

    for m in re.find_iter(text) {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        for len in [44, 47, 48] {
            if digits.len() < len {
                continue;
            }
            // Junk can sit at either end of the run — a protocol number
            // before it, a page number after — so both ends are offered and
            // the check digits decide.
            out.push(digits[..len].to_string());
            out.push(digits[digits.len() - len..].to_string());
        }
        out.push(digits);
    }

    out.sort();
    out.dedup();
    out
}

/// Tiers 1–4.
///
/// Every candidate is tried, and the **first one whose check digits pass**
/// wins: a bill's text contains plenty of long numbers, and a checksum is
/// exactly what tells a barcode apart from an account number that happens to
/// be the same length.
pub fn parse_text(
    text: &str,
    profiles: &Profiles,
    reference: NaiveDate,
) -> Result<Parsed, ImportError> {
    let found = candidates(text);
    if found.is_empty() {
        return Err(ImportError::NoBarcode);
    }

    let mut last = None;
    for candidate in found {
        match boleto::parse(&candidate, reference) {
            Ok(barcode) => {
                let import = issuers::resolve(&barcode, text, profiles);
                return Ok(Parsed { barcode, import });
            }
            Err(e) => last = Some(e),
        }
    }
    Err(match last {
        Some(e @ boleto::Error::CheckDigit { .. }) => ImportError::Misread(e),
        _ => ImportError::NoBarcode,
    })
}

/// The whole pipeline, from a file on disk.
pub fn import(
    path: &Path,
    profiles: &Profiles,
    reference: NaiveDate,
) -> Result<Parsed, ImportError> {
    parse_text(&text_of(path)?, profiles, reference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::money::Cents;
    use crate::services::boleto::Amount;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 21).unwrap()
    }

    /// A bill as `pdftotext -layout` actually renders one: the linha digitável
    /// broken into blocks, surrounded by other long numbers.
    const BILL: &str = "
ENEL DISTRIBUIÇÃO SÃO PAULO
CNPJ 12.345.678/0001-99
Instalação 300123456789          Medidor 00987654321
Número do cliente: 987654-3

83660000003-5 43621234202-2 60715000000-0 00000000000-0

Total a pagar  R$ 343,62
";

    fn profiles() -> Profiles {
        toml::from_str(
            r#"
[[issuer]]
id = "enel-sp"
name = "Energia · Enel SP"
detect = ['\b12\.?345\.?678/?0001-?99\b']
kind = "consumo"

[issuer.client_code]
regex = 'N[uú]mero do cliente:?\s*(\d[\d-]+)'
"#,
        )
        .unwrap()
    }

    #[test]
    fn a_linha_digitavel_is_found_among_the_other_long_numbers_on_a_bill() {
        // A bill is full of long digit runs — installation numbers, meter
        // numbers, protocol codes. The checksum is what tells the barcode
        // apart from all of them.
        let parsed = parse_text(BILL, &profiles(), today()).unwrap();
        assert_eq!(parsed.barcode.amount, Amount::Fixed(Cents(34362)));
        assert_eq!(parsed.import.issuer.as_deref(), Some("enel-sp"));
        assert_eq!(parsed.import.client_code.as_deref(), Some("987654-3"));
        assert_eq!(parsed.import.amount_source, Some("barcode"));
        assert!(parsed.import.unresolved.is_empty());
    }

    #[test]
    fn a_bill_with_no_barcode_says_so_rather_than_reading_the_total() {
        let text = "ENEL\nCNPJ 12.345.678/0001-99\nTotal a pagar R$ 343,62";
        assert!(matches!(
            parse_text(text, &profiles(), today()),
            Err(ImportError::NoBarcode)
        ));
    }

    #[test]
    fn a_corrupted_barcode_is_reported_as_a_misread() {
        // Never a cue to fall back to the text: the printed total might agree
        // with the wrong number, and then nothing catches it.
        let text = BILL.replace("83660000003-5", "83660000004-5");
        match parse_text(&text, &profiles(), today()) {
            Err(ImportError::Misread(_)) => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn candidates_join_a_run_across_the_separators_a_bill_prints() {
        let found = candidates("83660000003-5 43621234202-2 60715000000-0 00000000000-0");
        assert!(found.iter().any(|c| c.len() == 48));
        assert!(
            found
                .iter()
                .all(|c| c.chars().all(|ch| ch.is_ascii_digit()))
        );
    }

    #[test]
    fn a_text_file_is_read_without_shelling_out() {
        // A household filing a bill by hand may well have one, and running
        // pdftotext over a .txt would be silly.
        let dir = std::env::temp_dir().join("ffm-import-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bill.txt");
        std::fs::write(&path, BILL).unwrap();
        assert!(text_of(&path).unwrap().contains("ENEL"));
    }
}

// ---------------------------------------------------------------------------
// what an import is allowed to write
// ---------------------------------------------------------------------------

/// What to do with one value the bill claims.
///
/// **Fill what is empty; replace what was only a seed; flag what a person
/// typed.** A rule's occurrence starts with an amount the materializer copied
/// in — the rule's own figure, or last period's — and that is a placeholder
/// waiting for the bill, not a decision anybody made about this period. The
/// barcode is checksummed; it *is* the amount owed, so it replaces a seed.
/// What it must never do is silently replace a number somebody typed on this
/// row with `e`: that one keeps, and the person is told.
///
/// This was "never overwrite" outright, which read a seed as a person's
/// choice: a rule created at R$ 883,34 kept that figure against its own bill
/// for R$ 131,45, and the row went on showing a number nobody had meant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fill<T> {
    /// Nothing was there; write it.
    Write(T),
    /// A seed was there, and the bill says otherwise: write the bill's, and
    /// say what it replaced.
    Replace { found: T, was: T },
    /// Both present and equal — nothing to do, nothing to say.
    Agrees,
    /// A person typed something else. The row keeps it, and the person is
    /// told.
    Differs { found: T, kept: T },
    /// The bill did not carry it.
    Missing,
}

/// `typed` is whether the value on the row was set by a person on this row
/// (`amount_source = 'user'`), as opposed to seeded or read off a bill.
pub fn reconcile<T: PartialEq + Copy>(
    found: Option<T>,
    existing: Option<T>,
    typed: bool,
) -> Fill<T> {
    match (found, existing) {
        (Some(f), None) => Fill::Write(f),
        (Some(f), Some(k)) if f == k => Fill::Agrees,
        (Some(f), Some(k)) if typed => Fill::Differs { found: f, kept: k },
        (Some(f), Some(k)) => Fill::Replace { found: f, was: k },
        (None, _) => Fill::Missing,
    }
}

impl<T> Fill<T> {
    /// The value to write, if any. `Differs` deliberately writes nothing.
    pub fn to_write(self) -> Option<T> {
        match self {
            Fill::Write(v) | Fill::Replace { found: v, .. } => Some(v),
            _ => None,
        }
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use crate::domain::money::Cents;

    #[test]
    fn an_empty_field_is_filled() {
        assert_eq!(
            reconcile(Some(Cents(34362)), None, false),
            Fill::Write(Cents(34362))
        );
        assert_eq!(
            reconcile(Some(Cents(34362)), None, true).to_write(),
            Some(Cents(34362))
        );
    }

    /// The report: a rule created at R$ 883,34, its bill for R$ 131,45. The
    /// row's figure was only ever the rule's seed, so the bill replaces it.
    #[test]
    fn a_seeded_value_is_replaced_by_the_bill() {
        let got = reconcile(Some(Cents(13145)), Some(Cents(88334)), false);
        assert_eq!(
            got,
            Fill::Replace {
                found: Cents(13145),
                was: Cents(88334)
            }
        );
        assert_eq!(got.to_write(), Some(Cents(13145)));
    }

    #[test]
    fn a_value_typed_on_the_row_is_kept_and_flagged() {
        // The one thing worse than failing to import is silently replacing a
        // right answer with a wrong one.
        let got = reconcile(Some(Cents(34362)), Some(Cents(35000)), true);
        assert_eq!(
            got,
            Fill::Differs {
                found: Cents(34362),
                kept: Cents(35000)
            }
        );
        assert_eq!(got.to_write(), None, "and writes nothing");
    }

    #[test]
    fn agreement_is_silent_and_a_missing_value_is_not_a_problem() {
        assert_eq!(
            reconcile(Some(Cents(1)), Some(Cents(1)), true),
            Fill::Agrees
        );
        assert_eq!(
            reconcile(None::<Cents>, Some(Cents(1)), false),
            Fill::Missing
        );
        assert_eq!(reconcile(None::<Cents>, None, false), Fill::Missing);
    }
}

// ---------------------------------------------------------------------------
// what a rule's bill is bound to
// ---------------------------------------------------------------------------

/// The identifiers a bill can be recognized by. The same shape serves both
/// sides: what a rule has on file, and what a freshly-read bill says.
#[derive(Clone, Copy, Debug, Default)]
pub struct Binding<'a> {
    /// The subscriber's account, from the text layer. The strongest identity
    /// there is — two bills from the same company to two houses differ here.
    pub client_code: Option<&'a str>,
    /// The arrecadação company code, from the barcode. Weaker (it names the
    /// company, not the account) but always there on a utility bill, which is
    /// what makes it the fallback when no profile could read a client code.
    pub company: Option<&'a str>,
    /// `consumo` | `cobranca`. Not an identity on its own, but a disagreement
    /// is decisive: a bank slip dropped on a rule bound to a utility company
    /// is the wrong bill, even though it carries no company code to compare.
    pub kind: Option<&'a str>,
    /// A cobrança's bank code. Read-side only — it is never a binding, since
    /// one bank issues slips for a hundred billers — and used only to *name*
    /// what a refused bill was.
    pub bank: Option<&'a str>,
}

/// One side of a disagreement, for the message: what the bill is for, or
/// what the rule is bound to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ident {
    Account(String),
    Company(String),
    Bank(String),
    Kind(String),
}

impl Binding<'_> {
    /// The most specific thing this side can be named by.
    fn ident(&self) -> Option<Ident> {
        self.company
            .map(|c| Ident::Company(c.into()))
            .or(self.bank.map(|b| Ident::Bank(b.into())))
            .or(self.kind.map(|k| Ident::Kind(k.into())))
            .or(self.client_code.map(|c| Ident::Account(c.into())))
    }

    /// Whether anything is bound at all. An unbound rule accepts any bill —
    /// that first attach is what binds it.
    pub fn is_bound(&self) -> bool {
        self.client_code.is_some() || self.company.is_some() || self.kind.is_some()
    }
}

/// Which identifier disagreed, and both sides — the message names them so a
/// person can see *why* this is the wrong row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub found: Ident,
    pub known: Ident,
}

/// Whether a bill belongs to a rule, before anything is written.
///
/// **Refuse on a positive disagreement.** The client code is consulted first
/// and, when both sides have one, decides alone: two houses with the same
/// water company share a company code and are still two different bills.
/// Then the barcode kind — a bank slip is never a utility bill. Then the
/// company code. A value the bill simply did not carry proves nothing, and a
/// rule with nothing bound yet accepts anything.
pub fn binding_check(bound: Binding<'_>, read: Binding<'_>) -> Result<(), Mismatch> {
    let fail = |found: Ident, known: Ident| Err(Mismatch { found, known });
    if let (Some(k), Some(f)) = (bound.client_code, read.client_code) {
        return if k == f {
            Ok(())
        } else {
            fail(Ident::Account(f.into()), Ident::Account(k.into()))
        };
    }
    if let (Some(k), Some(f)) = (bound.kind, read.kind)
        && k != f
    {
        let found = read.ident().unwrap_or(Ident::Kind(f.into()));
        let known = bound.ident().unwrap_or(Ident::Kind(k.into()));
        return fail(found, known);
    }
    if let (Some(k), Some(f)) = (bound.company, read.company)
        && k != f
    {
        return fail(Ident::Company(f.into()), Ident::Company(k.into()));
    }
    Ok(())
}

#[cfg(test)]
mod binding_tests {
    use super::*;

    fn b<'a>(client: Option<&'a str>, company: Option<&'a str>) -> Binding<'a> {
        Binding {
            client_code: client,
            company,
            kind: company.map(|_| "consumo"),
            bank: None,
        }
    }

    fn slip<'a>(bank: &'a str) -> Binding<'a> {
        Binding {
            kind: Some("cobranca"),
            bank: Some(bank),
            ..Default::default()
        }
    }

    #[test]
    fn an_unbound_rule_accepts_any_bill() {
        assert!(binding_check(Binding::default(), b(Some("123"), Some("0048"))).is_ok());
        assert!(binding_check(Binding::default(), slip("077")).is_ok());
    }

    #[test]
    fn a_different_client_code_is_the_wrong_row() {
        let m = binding_check(b(Some("5544332211"), None), b(Some("123"), None)).unwrap_err();
        assert_eq!(m.found, Ident::Account("123".into()));
        assert_eq!(m.known, Ident::Account("5544332211".into()));
    }

    #[test]
    fn the_client_code_decides_alone_when_both_sides_have_one() {
        assert!(binding_check(b(Some("1"), Some("0048")), b(Some("1"), Some("0099"))).is_ok());
    }

    #[test]
    fn the_company_code_is_the_fallback() {
        assert!(binding_check(b(None, Some("0048")), b(None, Some("0099"))).is_err());
        assert!(binding_check(b(Some("1"), Some("0048")), b(None, Some("0099"))).is_err());
        assert!(binding_check(b(None, Some("0048")), b(None, Some("0048"))).is_ok());
    }

    /// The case that got through: a credit-card statement — a bank slip with
    /// no company code — dropped on a rule bound to utility company 0162.
    #[test]
    fn a_bank_slip_on_a_rule_bound_to_a_company_is_the_wrong_row() {
        let m = binding_check(b(None, Some("0162")), slip("077")).unwrap_err();
        assert_eq!(m.found, Ident::Bank("077".into()));
        assert_eq!(m.known, Ident::Company("0162".into()));
    }

    #[test]
    fn a_value_the_bill_did_not_carry_proves_nothing() {
        assert!(binding_check(b(Some("1"), Some("0048")), Binding::default()).is_ok());
    }
}
