//! FEBRABAN barcode parsing — the one place a bill's amount comes from.
//!
//! **For value and due date this does not parse text, does not OCR and does
//! not infer.** A Brazilian bill carries a barcode that *encodes* both as
//! digits, protected by check digits: a deterministic parse with built-in
//! error detection rather than a heuristic extraction. For something that
//! writes money into a ledger that property is the entire argument — a failed
//! check digit is a hard stop, never a cue to go and guess from the text.
//!
//! What the barcode never carries is **who issued it**. That always comes from
//! the text layer, matched against an issuer profile.
//!
//! Pure integer arithmetic, no I/O, no dependencies.

use crate::domain::money::Cents;
use chrono::{Duration, NaiveDate};

/// What the value digits actually mean.
///
/// A sum type rather than a number, because position 3 of an arrecadação
/// barcode *declares* what its eleven value digits are — and a `7`/`9` bill
/// read as reais posts a plausible-looking **wrong number** to the ledger,
/// which is worse than failing to parse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Amount {
    /// A real amount in centavos.
    Fixed(Cents),
    /// All zeros — the payer chooses. On a credit-card fatura this is the
    /// normal case, not a malformed bill: the choice between the full total,
    /// the minimum and anything between is the payer's.
    Open,
    /// Quantidade de moeda / an indexed value. **Not currency.** Never coerce
    /// this into `Cents`.
    Reference(u64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DueDate {
    /// Cobrança, from the fator de vencimento.
    Encoded(NaiveDate),
    /// Arrecadação, free-field positions 20–27 as AAAAMMDD.
    FreeField(NaiveDate),
    /// Not carried by this barcode — fall back to the issuer profile.
    Absent,
}

impl DueDate {
    pub fn date(self) -> Option<NaiveDate> {
        match self {
            DueDate::Encoded(d) | DueDate::FreeField(d) => Some(d),
            DueDate::Absent => None,
        }
    }

    /// Which field a stored `due_date_source` should record.
    pub fn source(self) -> Option<&'static str> {
        match self {
            DueDate::Encoded(_) => Some("fator"),
            DueDate::FreeField(_) => Some("free_field"),
            DueDate::Absent => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Banks, card faturas, condomínio, school fees, financing.
    Cobranca,
    /// Energia, água, gás, telecom, tributos — most household bills.
    Arrecadacao,
}

impl Kind {
    pub fn code(self) -> &'static str {
        match self {
            Kind::Cobranca => "cobranca",
            Kind::Arrecadacao => "consumo",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Barcode {
    pub kind: Kind,
    /// The 44 digits, normalized — this is what gets stored, and what makes
    /// re-importing the same bill idempotent.
    pub raw: String,
    pub amount: Amount,
    pub due: DueDate,
    /// Arrecadação only: the four digits identifying the company or body.
    pub company: Option<String>,
    /// Cobrança only.
    pub bank: Option<String>,
}

impl Barcode {
    /// Whether this barcode names **one bill**, or only an account.
    ///
    /// A cobrança whose due factor and amount are both zero
    /// (`… 3 00000000000000`) carries neither a date nor a value: it is the
    /// card's own payment slip, and an issuer like Inter prints the very same
    /// digits on every month's fatura — the payer chooses how much to pay,
    /// whenever. Such a barcode cannot be the "same bill twice" key, so it is
    /// exempt from the one-barcode-one-row rule. Mirrored in SQL by
    /// `idx_tx_barcode` (migration 0013) and `trash::bill_held_by` — keep the
    /// three in step.
    pub fn names_one_bill(&self) -> bool {
        !names_an_account(self.kind, &self.raw)
    }
}

/// `raw` (the 44 digits) is a cobrança with no due factor and no amount —
/// see [`Barcode::names_one_bill`].
pub fn names_an_account(kind: Kind, raw: &str) -> bool {
    kind == Kind::Cobranca && raw.get(5..19).is_some_and(|f| f.bytes().all(|b| b == b'0'))
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Error {
    /// Not 44, 47 or 48 digits after separators are stripped.
    Length(usize),
    NotDigits,
    /// A check digit did not match. **A hard stop** — the amount on this bill
    /// cannot be trusted, and falling back to reading the text would be
    /// trusting a number that already failed its own checksum.
    CheckDigit {
        at: &'static str,
    },
    /// Position 3 of an arrecadação barcode is outside 6–9.
    UnknownValueKind(char),
    /// A cobrança whose currency digit (position 4) is not `9`, the real.
    /// FEBRABAN has no other currency in use, so this is not a bill — it is a
    /// number that happens to be 44 digits long. A QR code drawn as rows of
    /// 0/1 characters in a bill's text is exactly that, and one window in
    /// eleven of it passes the mod-11 digit by chance (Vivo, v0.0.6).
    Currency(u32),
    /// An arrecadação whose product digit (position 1) is not `8`, or whose
    /// segment (position 2) is `0`. Same reasoning: the standard fixes them.
    NotArrecadacao,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Length(n) => write!(f, "a barcode is 44, 47 or 48 digits — this has {n}"),
            Error::NotDigits => write!(f, "that is not a barcode"),
            Error::CheckDigit { at } => write!(f, "the check digit for {at} does not match"),
            Error::UnknownValueKind(c) => {
                write!(f, "position 3 says `{c}`, which is not a value kind")
            }
            Error::Currency(d) => {
                write!(f, "position 4 says `{d}`; a bank slip in reais says 9")
            }
            Error::NotArrecadacao => write!(f, "that is not a utility payment code"),
        }
    }
}

// ---------------------------------------------------------------------------
// the fator de vencimento, and its rollover
// ---------------------------------------------------------------------------

/// Base for factors issued until 21/02/2025, when the counter reached 9999.
const LEGACY_BASE: (i32, u32, u32) = (1997, 10, 7);
/// Base from 22/02/2025, where factor 1000 lands.
const NEW_BASE: (i32, u32, u32) = (2022, 5, 29);
/// The day the counter reset. A legacy factor cannot mean a date on or after
/// this, because the legacy base was retired here.
const RESET_DAY: (i32, u32, u32) = (2025, 2, 22);

fn base(d: (i32, u32, u32)) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.0, d.1, d.2).expect("a constant date")
}

/// Turns a 4-digit factor into a date.
///
/// The factor is a day count that **overflowed and was reset**, so the same
/// number means two different dates: one under each base. Get it wrong and
/// every current bill lands about 27 years out.
///
/// **The disambiguation is by proximity to `reference`**, not by a threshold.
/// The spec suggested "prefer the new base when the resulting date is ≥
/// 22/02/2025, else legacy", and that rule does not survive its own test
/// vectors: factor 9649 gives 2024-03-08 under the legacy base and 2048 under
/// the new one, and the threshold picks the absurd answer. Nearest-to-now is
/// what a human does when they look at a bill, and it gets both vectors right.
pub fn due_from_fator(fator: u16, reference: NaiveDate) -> Option<NaiveDate> {
    // `0000` means no due date encoded, which is a fact rather than a failure.
    if fator == 0 {
        return None;
    }
    let days = Duration::days(fator as i64);
    let legacy = base(LEGACY_BASE) + days;
    let new = base(NEW_BASE) + days;

    // A legacy factor cannot mean a date past the reset: that range belongs to
    // the new base now.
    if legacy >= base(RESET_DAY) {
        return Some(new);
    }
    let near = |d: NaiveDate| (d - reference).num_days().abs();
    Some(if near(new) <= near(legacy) {
        new
    } else {
        legacy
    })
}

// ---------------------------------------------------------------------------
// check digits
// ---------------------------------------------------------------------------

fn digits(s: &str) -> Option<Vec<u32>> {
    s.chars().map(|c| c.to_digit(10)).collect()
}

/// Weights 2 and 1 alternating from the right; anything over 9 has 9
/// subtracted. Used for every linha digitável block and for an arrecadação
/// barcode whose position 3 is 6 or 7.
pub fn mod10(digits: &[u32]) -> u32 {
    let mut sum = 0;
    for (i, d) in digits.iter().rev().enumerate() {
        let weight = if i % 2 == 0 { 2 } else { 1 };
        let mut v = d * weight;
        if v > 9 {
            v -= 9;
        }
        sum += v;
    }
    (10 - sum % 10) % 10
}

/// Weights 2 through 9 cycling right to left. A cobrança's DV geral; an
/// arrecadação's mod 11 is [`mod11_arrecadacao`], which writes the edge cases
/// differently.
pub fn mod11(digits: &[u32]) -> u32 {
    let mut sum = 0;
    let mut weight = 2;
    for d in digits.iter().rev() {
        sum += d * weight;
        weight = if weight == 9 { 2 } else { weight + 1 };
    }
    let dv = 11 - (sum % 11);
    // 0, 10 and 11 are all written as 1.
    if dv == 0 || dv > 9 { 1 } else { dv }
}

/// An arrecadação's mod 11, for value kinds 8 and 9: the same weights as
/// [`mod11`], but a remainder of 0 or 1 is written as `0`, not `1`. Reading it
/// with the cobrança's rule refused every DAS whose DV is 0 (Simples
/// Nacional, v0.0.19).
pub fn mod11_arrecadacao(digits: &[u32]) -> u32 {
    let mut sum = 0;
    let mut weight = 2;
    for d in digits.iter().rev() {
        sum += d * weight;
        weight = if weight == 9 { 2 } else { weight + 1 };
    }
    match sum % 11 {
        0 | 1 => 0,
        r => 11 - r,
    }
}

/// An arrecadação's check-digit algorithm, decided by its value kind
/// (position 3): mod 10 for 6 and 7, mod 11 for 8 and 9 — for the DV geral
/// and for each block of the linha digitável alike.
fn arrecadacao_dv(value_kind: u32) -> fn(&[u32]) -> u32 {
    if matches!(value_kind, 8 | 9) { mod11_arrecadacao } else { mod10 }
}

// ---------------------------------------------------------------------------
// parsing
// ---------------------------------------------------------------------------

/// Parses whatever was read off a bill: the 44-digit barcode, or the 47/48
/// digit linha digitável a person can actually type.
///
/// Separators are stripped first — a linha digitável is printed with dots and
/// spaces, and refusing it for that would be refusing the form people have in
/// front of them.
pub fn parse(input: &str, reference: NaiveDate) -> Result<Barcode, Error> {
    let cleaned: String = input
        .chars()
        .filter(|c| !matches!(c, ' ' | '.' | '-'))
        .collect();
    let Some(ds) = digits(&cleaned) else {
        return Err(Error::NotDigits);
    };

    match ds.len() {
        44 if ds[0] == 8 => arrecadacao(&ds),
        44 => cobranca(&ds, reference),
        47 => cobranca(&from_linha_47(&ds)?, reference),
        48 => arrecadacao(&from_linha_48(&ds)?),
        n => Err(Error::Length(n)),
    }
}

/// Rebuilds the 44-digit barcode from the 47-digit linha digitável, checking
/// each block's own mod-10 digit on the way through.
fn from_linha_47(d: &[u32]) -> Result<Vec<u32>, Error> {
    let block = |from: usize, len: usize, at: &'static str| -> Result<&[u32], Error> {
        let body = &d[from..from + len - 1];
        if mod10(body) != d[from + len - 1] {
            return Err(Error::CheckDigit { at });
        }
        Ok(body)
    };

    let c1 = block(0, 10, "campo 1")?;
    let c2 = block(10, 11, "campo 2")?;
    let c3 = block(21, 11, "campo 3")?;
    let dv_geral = d[32];
    let c5 = &d[33..47];

    // banco+moeda, then the DV, then fator+valor, then the free field split
    // across the three campos.
    let mut out = Vec::with_capacity(44);
    out.extend_from_slice(&c1[0..4]);
    out.push(dv_geral);
    out.extend_from_slice(c5);
    out.extend_from_slice(&c1[4..9]);
    out.extend_from_slice(c2);
    out.extend_from_slice(c3);
    Ok(out)
}

/// Four blocks of eleven digits, each followed by its own check digit — mod 10,
/// or mod 11 when the value kind (the third digit) is 8 or 9. Always mod 10
/// refused every DAS, whose value kind is 8.
fn from_linha_48(d: &[u32]) -> Result<Vec<u32>, Error> {
    let dv = arrecadacao_dv(d[2]);
    let mut out = Vec::with_capacity(44);
    for (i, at) in ["block 1", "block 2", "block 3", "block 4"]
        .iter()
        .enumerate()
    {
        let from = i * 12;
        let body = &d[from..from + 11];
        if dv(body) != d[from + 11] {
            return Err(Error::CheckDigit { at });
        }
        out.extend_from_slice(body);
    }
    Ok(out)
}

fn to_string(d: &[u32]) -> String {
    d.iter()
        .map(|x| char::from_digit(*x, 10).unwrap_or('0'))
        .collect()
}

fn number(d: &[u32]) -> u64 {
    d.iter().fold(0u64, |acc, x| acc * 10 + *x as u64)
}

fn cobranca(d: &[u32], reference: NaiveDate) -> Result<Barcode, Error> {
    // The currency first: it is fixed by the standard, so a number without
    // it is not a bank slip at all — checked before the DV, which a random
    // run passes one time in eleven.
    if d[3] != 9 {
        return Err(Error::Currency(d[3]));
    }
    // The DV geral covers every digit except itself.
    let mut without_dv: Vec<u32> = Vec::with_capacity(43);
    without_dv.extend_from_slice(&d[0..4]);
    without_dv.extend_from_slice(&d[5..44]);
    if mod11(&without_dv) != d[4] {
        return Err(Error::CheckDigit { at: "DV geral" });
    }

    let fator = number(&d[5..9]) as u16;
    let valor = number(&d[9..19]);

    Ok(Barcode {
        kind: Kind::Cobranca,
        raw: to_string(d),
        // A cobrança's value is always reais; only its absence is special.
        amount: if valor == 0 {
            Amount::Open
        } else {
            Amount::Fixed(Cents(valor as i64))
        },
        due: match due_from_fator(fator, reference) {
            Some(date) => DueDate::Encoded(date),
            None => DueDate::Absent,
        },
        company: None,
        bank: Some(to_string(&d[0..3])),
    })
}

fn arrecadacao(d: &[u32]) -> Result<Barcode, Error> {
    // Product 8, and a segment 1–9: fixed by the standard. A 48-digit linha
    // reaches here through `from_linha_48`, which checks only its blocks.
    if d[0] != 8 || d[1] == 0 {
        return Err(Error::NotArrecadacao);
    }
    let value_kind = char::from_digit(d[2], 10).unwrap_or('?');

    // Position 3 decides BOTH what the value means and which algorithm
    // protects it. Reading one without the other is the trap this whole sum
    // type exists for.
    let is_reference = match value_kind {
        '6' | '8' => false,
        '7' | '9' => true,
        other => return Err(Error::UnknownValueKind(other)),
    };

    let mut without_dv: Vec<u32> = Vec::with_capacity(43);
    without_dv.extend_from_slice(&d[0..3]);
    without_dv.extend_from_slice(&d[4..44]);
    if arrecadacao_dv(d[2])(&without_dv) != d[3] {
        return Err(Error::CheckDigit { at: "DV geral" });
    }

    let valor = number(&d[4..15]);
    let livre = &d[19..44];

    Ok(Barcode {
        kind: Kind::Arrecadacao,
        raw: to_string(d),
        amount: match (is_reference, valor) {
            // Zeros are legal, and mean the payer decides.
            (_, 0) => Amount::Open,
            (true, v) => Amount::Reference(v),
            (false, v) => Amount::Fixed(Cents(v as i64)),
        },
        // "In case due date is used (AAAAMMDD), include it in the first 8
        // positions of the free field." When the convention is not followed
        // those positions hold arbitrary issuer data, so the date has to parse
        // before it is believed.
        due: match free_field_date(&livre[0..8]) {
            Some(date) => DueDate::FreeField(date),
            None => DueDate::Absent,
        },
        company: Some(to_string(&d[15..19])),
        bank: None,
    })
}

fn free_field_date(d: &[u32]) -> Option<NaiveDate> {
    let year = number(&d[0..4]) as i32;
    let month = number(&d[4..6]) as u32;
    let day = number(&d[6..8]) as u32;
    // A plausibility window as well as a valid date: `20226071` parses as
    // nothing, but `19990101` parses fine and is still not a due date.
    if !(2000..=2099).contains(&year) {
        return None;
    }
    NaiveDate::from_ymd_opt(year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 21).unwrap()
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // -- the seven verified vectors -------------------------------------
    // Generated by the reference implementation in design/spec-bill-import.md;
    // encode/decode round-trips cleanly there.

    #[test]
    fn vector_1_cobranca_with_a_real_amount() {
        let b = parse("34191790010104351004779102015009815700000034362", today()).unwrap();
        assert_eq!(b.kind, Kind::Cobranca);
        assert_eq!(b.amount, Amount::Fixed(Cents(34362)));
        assert_eq!(b.due, DueDate::Encoded(d(2026, 9, 15)));
        assert_eq!(b.bank.as_deref(), Some("341"));
    }

    #[test]
    fn a_number_without_the_real_s_currency_digit_is_not_a_bank_slip() {
        // A row of Vivo's QR code, drawn in its text. Its mod-11 digit passes
        // by chance — one window in eleven does — and it read as R$ 10
        // million, due 30/05/2022. Position 4 is the currency, and the real is
        // 9: nothing else is a bank slip.
        assert_eq!(
            parse("00001000110111000000100011011100000110111101", today()),
            Err(Error::Currency(0))
        );
    }

    #[test]
    fn a_48_digit_linha_that_is_not_product_8_is_not_a_utility_payment() {
        // Valid blocks, so only the product digit can refuse it.
        let mut d = vec![
            3, 6, 6, 0, 0, 0, 0, 0, 0, 1, 6, 0, 9, 9, 0, 0, 8, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1,
            2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6,
        ];
        let mut body = d[0..3].to_vec();
        body.extend_from_slice(&d[4..]);
        d[3] = mod10(&body);
        let mut linha = String::new();
        for block in d.chunks(11) {
            linha.extend(block.iter().map(|x| char::from_digit(*x, 10).unwrap()));
            linha.push(char::from_digit(mod10(block), 10).unwrap());
        }
        assert_eq!(parse(&linha, today()), Err(Error::NotArrecadacao));
    }

    #[test]
    fn vector_2_a_card_fatura_leaves_the_amount_open() {
        // Not a malformed bill: the payer chooses between the full total, the
        // minimum, and anything in between.
        let b = parse("26091234560000123450900012345005115950000000000", today()).unwrap();
        assert_eq!(b.amount, Amount::Open);
        assert_eq!(b.due, DueDate::Encoded(d(2026, 10, 10)));
    }

    #[test]
    fn a_card_slip_with_no_date_and_no_amount_names_an_account_not_a_bill() {
        // Synthetic, with valid check digits: bank 260, factor 0000, value
        // zero — the shape Inter prints identically on every month's fatura.
        let b = parse("26091234560001234500501234500005500000000000000", today()).unwrap();
        assert_eq!(b.amount, Amount::Open);
        assert_eq!(b.due, DueDate::Absent);
        assert_eq!(b.raw, "26095000000000000001234500012345000123450000");
        assert!(!b.names_one_bill());
    }

    #[test]
    fn an_open_amount_with_a_due_date_still_names_one_bill() {
        // Vector 2: the payer picks the amount, but the date is this month's.
        let b = parse("26091234560000123450900012345005115950000000000", today()).unwrap();
        assert!(b.names_one_bill());
        // And a dated or valued bill always does.
        let b = parse("34191790010104351004779102015009815700000034362", today()).unwrap();
        assert!(b.names_one_bill());
        // Arrecadação is never exempt: its zeros sit elsewhere.
        let b = parse("846600000000000056780000000000000000000000000000", today()).unwrap();
        assert!(b.names_one_bill());
    }

    #[test]
    fn vector_3_an_archived_bill_uses_the_legacy_base() {
        // The factor overflowed and was reset in February 2025, so the same
        // number means two dates. Rodrigo's archive has pre-2025 bills, which
        // is why this path is not dead code.
        let b = parse("03390000102345678901723456789017996490000114800", today()).unwrap();
        assert_eq!(b.amount, Amount::Fixed(Cents(114800)));
        assert_eq!(b.due, DueDate::Encoded(d(2024, 3, 8)));
    }

    #[test]
    fn vector_4_a_zero_factor_means_no_due_date_at_all() {
        let b = parse("23799999868887777666165555444432100000000005990", today()).unwrap();
        assert_eq!(b.amount, Amount::Fixed(Cents(5990)));
        assert_eq!(b.due, DueDate::Absent, "absent is a fact, not a failure");
    }

    #[test]
    fn vector_5_consumo_type_6_carries_its_date_in_the_free_field() {
        let b = parse("836600000035436212342022607150000000000000000000", today()).unwrap();
        assert_eq!(b.kind, Kind::Arrecadacao);
        assert_eq!(b.amount, Amount::Fixed(Cents(34362)));
        assert_eq!(b.due, DueDate::FreeField(d(2026, 7, 15)));
        assert_eq!(b.company.as_deref(), Some("1234"));
    }

    #[test]
    fn vector_6_type_7_is_quantidade_de_moeda_and_must_not_become_money() {
        // The trap this sum type exists for: read as reais, this posts
        // R$ 343,62 to the ledger — a plausible-looking wrong number, which is
        // worse than failing to parse.
        let b = parse("837200000037436212340000000000000000000000000000", today()).unwrap();
        assert_eq!(b.amount, Amount::Reference(34362));
        assert!(!matches!(b.amount, Amount::Fixed(_)));
    }

    #[test]
    fn vector_8_a_das_is_mod_11_in_every_block_and_its_dv_can_be_0() {
        // A Simples Nacional DAS as printed (value kind 8, Receita Federal's
        // 0328): its block digits are mod 11, and its DV geral is 0 — a
        // remainder the cobrança's mod 11 would have written as 1. Refused
        // as misprinted until both were read the arrecadação's way.
        let printed = "85800000013 5 14470328262 8 93072026274 6 37737486168 1";
        let b = parse(printed, today()).unwrap();
        assert_eq!(b.kind, Kind::Arrecadacao);
        assert_eq!(b.amount, Amount::Fixed(Cents(131447)));
        assert_eq!(b.company.as_deref(), Some("0328"));
        // No date in its free field: the DAS prints it only as text.
        assert_eq!(b.due, DueDate::Absent);
        assert_eq!(parse(&b.raw, today()).unwrap(), b);
        // And a block's digit read as mod 10 is still a misprint.
        let mod10_block = "85800000013 4 14470328262 8 93072026274 6 37737486168 1";
        assert!(matches!(parse(mod10_block, today()), Err(Error::CheckDigit { .. })));
    }

    #[test]
    fn vector_7_consumo_with_zeros_is_open() {
        let b = parse("846600000000000056780000000000000000000000000000", today()).unwrap();
        assert_eq!(b.amount, Amount::Open);
        assert_eq!(b.company.as_deref(), Some("5678"));
    }

    // -- the property that justifies the whole approach ------------------

    #[test]
    fn every_single_digit_corruption_is_detected() {
        // 47 positions × 9 other digits = 423 mutations, and not one of them
        // may parse to a different amount. This is the property that makes a
        // barcode worth preferring over OCR for something that writes money
        // into a ledger; if a refactor breaks it, the ledger starts accepting
        // wrong numbers silently.
        let good = "34191790010104351004779102015009815700000034362";
        let original = parse(good, today()).unwrap();

        let mut checked = 0;
        for i in 0..good.len() {
            for replacement in b'0'..=b'9' {
                let mut bytes = good.as_bytes().to_vec();
                if bytes[i] == replacement {
                    continue;
                }
                bytes[i] = replacement;
                let mutated = String::from_utf8(bytes).unwrap();
                checked += 1;

                match parse(&mutated, today()) {
                    Err(_) => {}
                    Ok(b) => panic!(
                        "digit {i} changed to {} parsed anyway: {:?} (was {:?})",
                        replacement as char, b.amount, original.amount
                    ),
                }
            }
        }
        assert_eq!(checked, 423, "47 positions × 9 replacements");
    }

    // -- the rollover ----------------------------------------------------

    #[test]
    fn the_factor_rollover_is_resolved_by_proximity_not_a_threshold() {
        // The spec suggested a threshold, and it does not survive these two:
        // 9649 is 2024-03-08 under the legacy base and 2048 under the new one,
        // and "prefer new when ≥ 22/02/2025" picks the absurd answer.
        assert_eq!(due_from_fator(1570, today()), Some(d(2026, 9, 15)));
        assert_eq!(due_from_fator(9649, today()), Some(d(2024, 3, 8)));

        // The published anchors, both ends of the reset.
        assert_eq!(due_from_fator(1000, d(2025, 3, 1)), Some(d(2025, 2, 22)));
        assert_eq!(due_from_fator(0, today()), None);
    }

    #[test]
    fn a_legacy_factor_cannot_mean_a_date_after_the_reset() {
        // That range belongs to the new base now, whatever proximity says.
        let late = due_from_fator(9999, d(2049, 1, 1)).unwrap();
        assert!(late >= base(RESET_DAY), "{late}");
    }

    // -- the check digits themselves -------------------------------------

    #[test]
    fn the_two_algorithms_match_their_published_definitions() {
        // A cobrança's mod 11 writes 0, 10 and 11 all as 1; an
        // arrecadação's writes a remainder of 0 or 1 as 0.
        assert_eq!(mod11(&[0; 43]), 1);
        assert_eq!(mod11_arrecadacao(&[0; 43]), 0);
        assert_eq!(mod11_arrecadacao(&[1]), 9);
        // A worked mod-10: 1+2 → 1*2=2, sum 2+1=3 → dv 7.
        assert_eq!(mod10(&[1, 1]), 7);
        assert_eq!(mod10(&[0]), 0);
    }

    // -- input shapes ----------------------------------------------------

    #[test]
    fn a_linha_digitavel_is_accepted_as_printed() {
        // People read it off the bill with its dots and spaces; refusing that
        // is refusing the form they have in front of them.
        let printed = "34191.79001 01043.510047 79102.015009 8 15700000034362";
        assert_eq!(
            parse(printed, today()).unwrap().amount,
            Amount::Fixed(Cents(34362))
        );
    }

    #[test]
    fn the_44_digit_barcode_itself_parses_too() {
        let linha = "836600000035436212342022607150000000000000000000";
        let from_linha = parse(linha, today()).unwrap();
        // Whatever was scanned, the stored digits are the same 44 — which is
        // what makes re-importing the same bill idempotent.
        assert_eq!(from_linha.raw.len(), 44);
        let again = parse(&from_linha.raw, today()).unwrap();
        assert_eq!(again, from_linha);
    }

    #[test]
    fn anything_that_is_not_a_barcode_says_so_rather_than_guessing() {
        assert_eq!(parse("", today()), Err(Error::Length(0)));
        assert_eq!(parse("hello", today()), Err(Error::NotDigits));
        assert_eq!(parse("123", today()), Err(Error::Length(3)));
        // 44 digits starting with 8, but position 3 is not a value kind.
        let bad = format!("81{}", "0".repeat(42));
        assert_eq!(parse(&bad, today()), Err(Error::UnknownValueKind('0')));
    }

    #[test]
    fn a_failed_check_digit_is_a_hard_stop() {
        // Never a cue to fall back to reading the text: that would be trusting
        // a number which already failed its own checksum.
        let mut bad = "34191790010104351004779102015009815700000034362".to_string();
        bad.replace_range(32..33, "9");
        assert!(matches!(
            parse(&bad, today()),
            Err(Error::CheckDigit { .. })
        ));
    }
}
