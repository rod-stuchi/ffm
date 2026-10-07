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
    /// `None` is a **text-only bill**: no barcode at all, from an issuer whose
    /// profile says it sends those, read entirely from the page
    /// (`issuers::resolve_text`).
    pub barcode: Option<Barcode>,
    pub import: Import,
    /// The bill's text, reduced to a fingerprint ([`fingerprint`]) — what
    /// tells the same bill attached twice apart when no barcode can.
    pub fingerprint: String,
}

#[derive(Debug)]
pub enum ImportError {
    /// No text at all — a scan or a photo. This is where OCR would go.
    NoText,
    /// Text, but nothing in it looks like a barcode — and no profile that
    /// reads this issuer's bills without one. `issuer` names who it was, when
    /// a profile recognized it.
    NoBarcode {
        issuer: Option<String>,
    },
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
            ImportError::NoBarcode { .. } => write!(f, "no barcode found in this bill"),
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
    let mut out: Vec<String> = Vec::new();
    let mut offer = |c: String| {
        if !out.contains(&c) {
            out.push(c);
        }
    };

    for m in re.find_iter(text) {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        // A QR code (or a barcode) drawn in a font comes out of `pdftotext` as
        // rows of 0s and 1s — Vivo's bill has thirty of them, 65 digits each.
        // No real code is binary, and cut into windows those rows are dozens
        // of chances for a check digit to pass by luck.
        if digits.chars().all(|c| c == '0' || c == '1') {
            continue;
        }
        // The run as printed first: a linha that is exactly a linha beats a
        // window cut out of a longer number.
        if matches!(digits.len(), 44 | 47 | 48) {
            offer(digits.clone());
        }
        for len in [44, 47, 48] {
            if digits.len() < len {
                continue;
            }
            // Junk can sit at either end of the run — a protocol number
            // before it, a page number after — so both ends are offered and
            // the check digits decide.
            offer(digits[..len].to_string());
            offer(digits[digits.len() - len..].to_string());
        }
        offer(digits);
    }

    // In the order the bill prints them, NOT sorted. The first candidate that
    // validates wins, and sorting put every `0000…` run ahead of a real code
    // starting `8466…` — which is how a QR row became a R$ 10 million bill.
    out
}

/// Tiers 1–4.
///
/// Every candidate is tried, and the **first one whose check digits pass**
/// wins: a bill's text contains plenty of long numbers, and a checksum is
/// exactly what tells a barcode apart from an account number that happens to
/// be the same length.
///
/// With no barcode at all, a bill is still read when its issuer's profile says
/// it sends those (`barcode = "optional"`) — a **text-only bill**. A misread is
/// never worked around that way: a number that failed its own checksum is a
/// misread, not a missing barcode.
pub fn parse_text(
    text: &str,
    profiles: &Profiles,
    reference: NaiveDate,
) -> Result<Parsed, ImportError> {
    let fingerprint = fingerprint(text);
    match first_valid_barcode(text, reference) {
        Ok(barcode) => Ok(Parsed {
            import: issuers::resolve(&barcode, text, profiles),
            barcode: Some(barcode),
            fingerprint,
        }),
        Err(ImportError::NoBarcode { .. }) => match issuers::resolve_text(text, profiles) {
            Ok(import) => Ok(Parsed {
                barcode: None,
                import,
                fingerprint,
            }),
            Err(issuer) => Err(ImportError::NoBarcode { issuer }),
        },
        Err(e) => Err(e),
    }
}

/// The first candidate whose check digits pass.
fn first_valid_barcode(text: &str, reference: NaiveDate) -> Result<Barcode, ImportError> {
    let found = candidates(text);
    if found.is_empty() {
        return Err(ImportError::NoBarcode { issuer: None });
    }
    let mut last = None;
    for candidate in found {
        match boleto::parse(&candidate, reference) {
            Ok(barcode) => return Ok(barcode),
            Err(e) => last = Some(e),
        }
    }
    Err(match last {
        Some(e @ boleto::Error::CheckDigit { .. }) => ImportError::Misread(e),
        _ => ImportError::NoBarcode { issuer: None },
    })
}

/// A bill's text reduced to what identifies it: its letters and digits,
/// lowercased, hashed with blake3 (as `:prune` hashes files).
///
/// What is left out is what shrinking or re-saving a PDF changes without
/// changing the bill — spacing, line breaks, page breaks, punctuation the
/// text layer renders differently — so a trimmed or shrunk copy of a bill
/// still fingerprints the same. It is for comparison only: two live rows
/// holding the same fingerprint hold the same bill twice.
pub fn fingerprint(text: &str) -> String {
    let mut h = blake3::Hasher::new();
    let mut buf = [0u8; 4];
    for c in text.chars().filter(|c| c.is_alphanumeric()) {
        for l in c.to_lowercase() {
            h.update(l.encode_utf8(&mut buf).as_bytes());
        }
    }
    h.finalize().to_hex().to_string()
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
        assert_eq!(parsed.barcode.unwrap().amount, Amount::Fixed(Cents(34362)));
        assert_eq!(parsed.import.issuer.as_deref(), Some("enel-sp"));
        assert_eq!(parsed.import.client_code.as_deref(), Some("987654-3"));
        assert_eq!(parsed.import.amount_source, Some("barcode"));
        assert!(parsed.import.unresolved.is_empty());
    }

    #[test]
    fn a_bill_with_no_barcode_says_so_rather_than_reading_the_total() {
        let text = "ENEL\nCNPJ 12.345.678/0001-99\nTotal a pagar R$ 343,62";
        // Enel's profile never opted in to bills without a barcode, so its
        // total is not guessed from the page — and the refusal says who it was.
        match parse_text(text, &profiles(), today()) {
            Err(ImportError::NoBarcode { issuer }) => {
                assert_eq!(issuer.as_deref(), Some("Energia · Enel SP"))
            }
            other => panic!("{other:?}"),
        }
    }

    /// The shape of a Nubank fatura with no boleto, made up: no barcode, the
    /// issuer recognized by its CNPJ, which opts in to `barcode = "optional"`.
    const CARD_NO_BARCODE: &str = "\
Esta é a sua fatura de
janeiro, no valor de
R$ 2.345,67
Data de vencimento: 09 JAN 2026
CNPJ 11.222.333/0001-81
";

    fn card_profiles() -> Profiles {
        toml::from_str(
            r#"
[[issuer]]
id = "cartao-sem-codigo"
name = "Cartão · Exemplo"
detect = ['\b11\.?222\.?333/?0001-?81\b']
kind = "cobranca"
barcode = "optional"

[issuer.amount]
when = "open"
regex = 'no valor de\s+R\$\s*([\d.]+,\d{2})'

[issuer.due_date]
when = "absent"
regex = 'Data de vencimento:\s*(\d{2}\s+\p{Lu}{3}\s+\d{4})'
"#,
        )
        .unwrap()
    }

    #[test]
    fn a_bill_with_no_barcode_is_read_from_its_text_when_the_issuer_opted_in() {
        let parsed = parse_text(CARD_NO_BARCODE, &card_profiles(), today()).unwrap();
        assert!(parsed.barcode.is_none(), "a text-only bill");
        assert_eq!(parsed.import.amount, Some(Cents(234567)));
        assert_eq!(parsed.import.amount_source, Some("text"));
        assert_eq!(parsed.import.due, NaiveDate::from_ymd_opt(2026, 1, 9));
        assert_eq!(parsed.import.issuer.as_deref(), Some("cartao-sem-codigo"));
    }

    #[test]
    fn a_misread_barcode_is_never_read_as_a_text_only_bill() {
        // A number that failed its own checksum is a misread, not a missing
        // barcode — even from an issuer that opted in.
        let text =
            format!("{CARD_NO_BARCODE}\n83660000004-5 43621234202-2 60715000000-0 00000000000-0");
        assert!(matches!(
            parse_text(&text, &card_profiles(), today()),
            Err(ImportError::Misread(_))
        ));
    }

    #[test]
    fn a_fingerprint_ignores_what_shrinking_a_pdf_changes() {
        let a = fingerprint("Fatura  de janeiro\n\x0cR$ 2.345,67 — Vencimento: 09 JAN 2026");
        let b = fingerprint("fatura de\njaneiro R$2345,67 Vencimento 09 jan 2026");
        assert_eq!(a, b, "spacing, page breaks, punctuation and case");
        assert_ne!(
            a,
            fingerprint("fatura de fevereiro R$ 2.345,67 vencimento 09 fev 2026")
        );
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

    /// A valid 48-digit arrecadação linha for `cents`, built with the app's
    /// own check digits — segment 6 (telecom), value kind 6 (mod 10). Made
    /// up, so no household's account number lives in the repo.
    fn telecom_linha(cents: u64) -> String {
        use crate::services::boleto::mod10;
        let mut d: Vec<u32> = vec![8, 6, 6, 0];
        d.extend(
            format!("{cents:011}")
                .chars()
                .map(|c| c.to_digit(10).unwrap()),
        );
        d.extend([0, 0, 8, 0]);
        d.extend(
            "1234567890123456789012345"
                .chars()
                .map(|c| c.to_digit(10).unwrap()),
        );
        let mut body = d[0..3].to_vec();
        body.extend_from_slice(&d[4..]);
        d[3] = mod10(&body);
        let mut out = String::new();
        for block in d.chunks(11) {
            for x in block {
                out.push(char::from_digit(*x, 10).unwrap());
            }
            out.push(char::from_digit(mod10(block), 10).unwrap());
            out.push(' ');
        }
        out.trim_end().to_string()
    }

    /// The 44 digits a QR row on Vivo's bill offered in production: bank
    /// `000`, currency `0`, and a mod-11 digit that passes by luck.
    const QR_JUNK: &str = "00001000110111000000100011011100000110111101";

    #[test]
    fn a_qr_code_drawn_as_text_is_never_read_as_the_barcode() {
        // Vivo draws its Pix QR code as rows of 0/1 characters, and the rows
        // came out of `pdftotext` ahead of — and sorted ahead of — the real
        // linha. One of them passed mod-11 and the bill read as R$ 10
        // million, due 30/05/2022. Worst case here: the junk comes first.
        let qr_row = format!("{QR_JUNK}111001111110110110000");
        let text = format!(
            "TELEFONICA\n{qr}\n{qr}\n{qr}\n\n{linha} via Pix\n\nTotal a pagar 160,99\n",
            qr = qr_row,
            linha = telecom_linha(16099),
        );
        let parsed = parse_text(&text, &Profiles::default(), today()).unwrap();
        let barcode = parsed.barcode.expect("the linha, not the QR rows");
        assert_eq!(barcode.kind, crate::services::boleto::Kind::Arrecadacao);
        assert_eq!(barcode.amount, Amount::Fixed(Cents(16099)));
    }

    #[test]
    fn candidates_skip_binary_runs_and_keep_the_bills_order() {
        let linha = telecom_linha(16099);
        let found = candidates(&format!("{linha}\n{QR_JUNK}0000\n"));
        assert!(
            !found.iter().any(|c| c.starts_with(&QR_JUNK[..20])),
            "a run of 0s and 1s offers nothing: {found:?}"
        );
        let digits: String = linha.chars().filter(char::is_ascii_digit).collect();
        assert_eq!(
            found.first(),
            Some(&digits),
            "the run as printed comes first"
        );
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
    /// The issuer's CNPJ, 14 digits — on the RULE's side: what it is bound
    /// to (migrations 0014, 0016). A Nubank fatura on a rule bound to Inter is
    /// another card's bill, barcode or not.
    pub issuer: Option<&'a str>,
    /// Every CNPJ the BILL prints (`issuers::cnpjs_in`). The bound issuer
    /// agrees when it is among them; a bill that prints none says nothing.
    pub cnpjs: &'a [String],
    /// Cards' last four digits. A bill lists every card on its statement; a
    /// rule holds the one it is bound to (a one-element slice). They agree
    /// when the statement lists the bound card anywhere.
    pub cards: &'a [String],
}

/// One side of a disagreement, for the message: what the bill is for, or
/// what the rule is bound to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ident {
    Account(String),
    Company(String),
    Bank(String),
    Kind(String),
    /// A profile's id; the message shows its name.
    Issuer(String),
    /// A card's last four digits.
    Card(String),
}

impl Binding<'_> {
    /// The most specific thing this side can be named by.
    fn ident(&self) -> Option<Ident> {
        self.company
            .map(|c| Ident::Company(c.into()))
            .or(self.bank.map(|b| Ident::Bank(b.into())))
            .or(self.issuer.map(|i| Ident::Issuer(i.into())))
            .or(self.kind.map(|k| Ident::Kind(k.into())))
            .or(self.client_code.map(|c| Ident::Account(c.into())))
    }

    /// Whether anything is bound at all. An unbound rule accepts any bill —
    /// that first attach is what binds it.
    pub fn is_bound(&self) -> bool {
        self.client_code.is_some()
            || self.company.is_some()
            || self.kind.is_some()
            || self.issuer.is_some()
            || !self.cards.is_empty()
    }
}

/// Which identifier disagreed, and both sides — the message names them so a
/// person can see *why* this is the wrong row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub found: Ident,
    pub known: Ident,
}

/// What the identity check concluded, short of a disagreement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The rule has nothing bound yet — this bill will bind it.
    Unbound,
    /// Something the rule is bound to was compared, and it agreed.
    Agrees,
    /// The rule is bound, but the bill carried nothing to compare it with —
    /// a file with no barcode, from an issuer no profile recognizes.
    Unconfirmed,
}

/// Whether a bill belongs to a rule, before anything is written.
///
/// A **positive disagreement** is `Err`. The client code is consulted first
/// and, when both sides have one, decides alone: two houses with the same
/// water company share a company code and are still two different bills.
/// Then the issuer, then the card (the bound one listed anywhere on the
/// statement), then the barcode kind — a bank slip is never a utility bill —
/// then the company code. A value the bill simply did not carry proves
/// nothing; a bound rule that could compare nothing at all is `Unconfirmed`.
pub fn binding_check(bound: Binding<'_>, read: Binding<'_>) -> Result<Verdict, Mismatch> {
    let fail = |found: Ident, known: Ident| Err(Mismatch { found, known });
    if !bound.is_bound() {
        return Ok(Verdict::Unbound);
    }
    if let (Some(k), Some(f)) = (bound.client_code, read.client_code) {
        return if k == f {
            Ok(Verdict::Agrees)
        } else {
            fail(Ident::Account(f.into()), Ident::Account(k.into()))
        };
    }
    let mut compared = false;
    if let Some(k) = bound.issuer
        && let Some(first) = read.cnpjs.first()
    {
        compared = true;
        if !read.cnpjs.iter().any(|c| c == k) {
            return fail(Ident::Issuer(first.clone()), Ident::Issuer(k.into()));
        }
    }
    if let Some(k) = bound.cards.first()
        && let Some(first) = read.cards.first()
    {
        compared = true;
        if !read.cards.contains(k) {
            return fail(Ident::Card(first.clone()), Ident::Card(k.clone()));
        }
    }
    if let (Some(k), Some(f)) = (bound.kind, read.kind) {
        compared = true;
        if k != f {
            let found = read.ident().unwrap_or(Ident::Kind(f.into()));
            let known = bound.ident().unwrap_or(Ident::Kind(k.into()));
            return fail(found, known);
        }
    }
    if let (Some(k), Some(f)) = (bound.company, read.company) {
        compared = true;
        if k != f {
            return fail(Ident::Company(f.into()), Ident::Company(k.into()));
        }
    }
    Ok(if compared {
        Verdict::Agrees
    } else {
        Verdict::Unconfirmed
    })
}

/// What a person agreed to when they attached a bill anyway.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Override {
    /// Nothing could confirm it was this rule's bill.
    Unconfirmed,
    /// It names something else than the rule is bound to.
    Mismatch(Mismatch),
}

/// Whether an attach goes ahead, or a person is asked first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Go ahead — carrying what was overridden, if anything, for the log.
    Pass(Option<Override>),
    /// Ask before writing anything. `None` is "could not confirm".
    Ask(Option<Mismatch>),
}

/// **Never a dead end.** A bill that cannot be confirmed, or that contradicts
/// the rule, is a question for the person — not a refusal. `anyway` is their
/// `y`: it turns the question into a pass that remembers what was overridden.
/// A bill whose identity agrees ignores it, so nothing is noted that was not
/// actually overridden.
pub fn gate(verdict: Result<Verdict, Mismatch>, anyway: bool) -> Gate {
    match verdict {
        Ok(Verdict::Unbound | Verdict::Agrees) => Gate::Pass(None),
        Ok(Verdict::Unconfirmed) if anyway => Gate::Pass(Some(Override::Unconfirmed)),
        Ok(Verdict::Unconfirmed) => Gate::Ask(None),
        Err(m) if anyway => Gate::Pass(Some(Override::Mismatch(m))),
        Err(m) => Gate::Ask(Some(m)),
    }
}

/// Whether a bill is **this** rule's, firmly enough to send it there with no
/// row named — a file dropped anywhere on the page (`web::import::find`).
///
/// [`binding_check`] must agree, and something that names an account or an
/// issuer must actually have been compared: the client code, the issuer's
/// CNPJ printed on the bill, the bound card listed on it, or the arrecadação
/// company code. The barcode kind alone never does — every bank slip in the
/// country shares it — and neither does an unconfirmed or unbound rule:
/// recognizing a bill is not binding one.
pub fn names_rule(bound: Binding<'_>, read: Binding<'_>) -> bool {
    if binding_check(bound, read) != Ok(Verdict::Agrees) {
        return false;
    }
    let same = |a: Option<&str>, b: Option<&str>| matches!((a, b), (Some(x), Some(y)) if x == y);
    same(bound.client_code, read.client_code)
        || bound
            .issuer
            .is_some_and(|k| read.cnpjs.iter().any(|c| c == k))
        || bound.cards.first().is_some_and(|k| read.cards.contains(k))
        || same(bound.company, read.company)
}

#[cfg(test)]
mod binding_tests {
    use super::*;

    fn b<'a>(client: Option<&'a str>, company: Option<&'a str>) -> Binding<'a> {
        Binding {
            client_code: client,
            company,
            kind: company.map(|_| "consumo"),
            ..Default::default()
        }
    }

    /// A card rule bound to `issuer` (a CNPJ) and `cards`.
    fn card<'a>(issuer: Option<&'a str>, cards: &'a [String]) -> Binding<'a> {
        Binding {
            kind: Some("cobranca"),
            issuer,
            cards,
            ..Default::default()
        }
    }

    /// A card bill printing `cnpjs` and `cards`.
    fn bill<'a>(cnpjs: &'a [String], cards: &'a [String]) -> Binding<'a> {
        Binding {
            kind: Some("cobranca"),
            cnpjs,
            cards,
            ..Default::default()
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
        // Nothing contradicts — but nothing confirms either, so a person is
        // asked rather than the bill attached on faith.
        assert_eq!(
            binding_check(b(Some("1"), Some("0048")), Binding::default()),
            Ok(Verdict::Unconfirmed)
        );
    }

    #[test]
    fn an_unbound_rule_says_so_and_an_agreeing_bill_says_so() {
        assert_eq!(
            binding_check(Binding::default(), slip("077")),
            Ok(Verdict::Unbound)
        );
        assert_eq!(
            binding_check(b(None, Some("0048")), b(None, Some("0048"))),
            Ok(Verdict::Agrees)
        );
    }

    const NUBANK: &str = "18236120000158";
    const INTER: &str = "00416968000101";

    #[test]
    fn a_bill_that_does_not_print_the_bound_cnpj_is_another_bill() {
        let none: [String; 0] = [];
        let inter = [INTER.to_string()];
        let m = binding_check(card(Some(NUBANK), &none), bill(&inter, &none)).unwrap_err();
        assert_eq!(m.found, Ident::Issuer(INTER.into()));
        assert_eq!(m.known, Ident::Issuer(NUBANK.into()));
        // A statement prints merchants' CNPJs too: the bound one anywhere
        // among them agrees.
        let several = ["11222333000181".to_string(), NUBANK.to_string()];
        assert_eq!(
            binding_check(card(Some(NUBANK), &none), bill(&several, &none)),
            Ok(Verdict::Agrees)
        );
        // A bill that prints no CNPJ at all says nothing about the issuer.
        assert_eq!(
            binding_check(card(Some(NUBANK), &none), bill(&none, &none)),
            Ok(Verdict::Agrees),
            "the kind still agreed"
        );
    }

    #[test]
    fn the_bound_card_listed_anywhere_on_the_statement_agrees() {
        let bound = ["7766".to_string()];
        let both = ["3190".to_string(), "7766".to_string()];
        let other = ["1234".to_string()];
        let none: [String; 0] = [];
        assert_eq!(
            binding_check(card(None, &bound), bill(&none, &both)),
            Ok(Verdict::Agrees)
        );
        let m = binding_check(card(None, &bound), bill(&none, &other)).unwrap_err();
        assert_eq!(m.found, Ident::Card("1234".into()));
        assert_eq!(m.known, Ident::Card("7766".into()));
        // A statement that prints no card digits proves nothing about them.
        assert!(binding_check(card(None, &bound), bill(&none, &none)).is_ok());
    }

    #[test]
    fn the_client_code_still_decides_alone() {
        let none: [String; 0] = [];
        let inter = [INTER.to_string()];
        let bound = Binding {
            client_code: Some("1"),
            ..card(Some(NUBANK), &none)
        };
        let read = Binding {
            client_code: Some("1"),
            ..bill(&inter, &none)
        };
        assert_eq!(binding_check(bound, read), Ok(Verdict::Agrees));
    }

    #[test]
    fn a_text_only_card_bill_on_a_utility_rule_is_the_wrong_row() {
        // A bill with no barcode takes its profile's kind, so a card fatura
        // still disagrees with a rule bound to utility company 0162.
        let none: [String; 0] = [];
        let nubank = [NUBANK.to_string()];
        assert!(binding_check(b(None, Some("0162")), bill(&nubank, &none)).is_err());
    }

    #[test]
    fn the_gate_asks_unless_told_to_go_ahead() {
        let m = Mismatch {
            found: Ident::Issuer(INTER.into()),
            known: Ident::Issuer(NUBANK.into()),
        };
        assert_eq!(gate(Ok(Verdict::Unbound), false), Gate::Pass(None));
        assert_eq!(
            gate(Ok(Verdict::Agrees), true),
            Gate::Pass(None),
            "nothing overridden"
        );
        assert_eq!(gate(Ok(Verdict::Unconfirmed), false), Gate::Ask(None));
        assert_eq!(
            gate(Ok(Verdict::Unconfirmed), true),
            Gate::Pass(Some(Override::Unconfirmed))
        );
        assert_eq!(gate(Err(m.clone()), false), Gate::Ask(Some(m.clone())));
        assert_eq!(
            gate(Err(m.clone()), true),
            Gate::Pass(Some(Override::Mismatch(m)))
        );
    }

    #[test]
    fn a_drop_finds_a_rule_only_by_what_names_it() {
        let cnpj = ["11222333000181".to_string()];
        let card7766 = ["7766".to_string()];
        let none: [String; 0] = [];
        // The kind alone agrees with every bank slip — never enough.
        let kind_only = Binding {
            kind: Some("cobranca"),
            ..Default::default()
        };
        assert!(!names_rule(kind_only, slip("077")));
        // The client code decides, both ways.
        assert!(names_rule(b(Some("1"), None), b(Some("1"), None)));
        assert!(!names_rule(b(Some("1"), None), b(Some("2"), None)));
        // The issuer's CNPJ printed on the bill.
        assert!(names_rule(card(Some(&cnpj[0]), &none), bill(&cnpj, &none)));
        // The bound card listed on the statement.
        assert!(names_rule(card(None, &card7766), bill(&none, &card7766)));
        // The company code.
        assert!(names_rule(b(None, Some("0048")), b(None, Some("0048"))));
        // Nothing to compare: unconfirmed, never sent anywhere.
        assert!(!names_rule(card(Some(&cnpj[0]), &none), bill(&none, &none)));
        // An unbound rule is never a candidate.
        assert!(!names_rule(Binding::default(), b(Some("1"), Some("0048"))));
    }
}
