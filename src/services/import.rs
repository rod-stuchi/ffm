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
/// **Fill what is empty; flag what disagrees; never overwrite.** The one thing
/// worse than failing to import is silently replacing a right answer with a
/// wrong one — and the household typed theirs on purpose, while the barcode is
/// only *probably* about this period.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fill<T> {
    /// Nothing was there; write it.
    Write(T),
    /// Both present and equal — nothing to do, nothing to say.
    Agrees,
    /// Both present and different. The row keeps what it had, and the person
    /// is told.
    Differs { found: T, kept: T },
    /// The bill did not carry it.
    Missing,
}

pub fn reconcile<T: PartialEq + Copy>(found: Option<T>, existing: Option<T>) -> Fill<T> {
    match (found, existing) {
        (Some(f), None) => Fill::Write(f),
        (Some(f), Some(k)) if f != k => Fill::Differs { found: f, kept: k },
        (Some(_), Some(_)) => Fill::Agrees,
        (None, _) => Fill::Missing,
    }
}

impl<T> Fill<T> {
    /// The value to write, if any. `Differs` deliberately writes nothing.
    pub fn to_write(self) -> Option<T> {
        match self {
            Fill::Write(v) => Some(v),
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
            reconcile(Some(Cents(34362)), None),
            Fill::Write(Cents(34362))
        );
        assert_eq!(
            reconcile(Some(Cents(34362)), None).to_write(),
            Some(Cents(34362))
        );
    }

    #[test]
    fn a_disagreement_keeps_what_the_household_typed() {
        // The one thing worse than failing to import is silently replacing a
        // right answer with a wrong one.
        let got = reconcile(Some(Cents(34362)), Some(Cents(35000)));
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
        assert_eq!(reconcile(Some(Cents(1)), Some(Cents(1))), Fill::Agrees);
        assert_eq!(reconcile(None::<Cents>, Some(Cents(1))), Fill::Missing);
        assert_eq!(reconcile(None::<Cents>, None), Fill::Missing);
    }
}
