//! Fake barcodes the app cannot tell from real ones.
//!
//! A shuffled amount has to be the amount *inside* the bill's barcode too, or
//! the first bill read in the copy disagrees with its row. So a code is
//! rebuilt rather than scrambled: the same kind, bank or segment and value
//! kind, the new amount in the value field, the due date where it was (or
//! moved, for `--years`), a fresh free field, and every check digit worked
//! out with `boleto`'s own `mod10`/`mod11`. A rebuild is kept only when
//! `boleto::parse` reads back exactly that kind, amount and due date.

use anyhow::bail;
use chrono::NaiveDate;
use ffm::services::boleto::{self, Amount, DueDate, Kind, mod10, mod11, names_an_account};
use rand::Rng;
use rand::rngs::StdRng;

/// What a rebuilt code must say.
pub struct Target {
    /// The new amount, for a code that carried one. Ignored for a code whose
    /// value is open or a reference count.
    pub cents: Option<i64>,
    /// Where the due date goes, when the code carries one: unchanged unless
    /// `--years` moved it.
    pub due: Option<NaiveDate>,
    /// The arrecadação company code, already mapped.
    pub company: Option<String>,
}

/// Rebuilds `raw` (44 digits) to say `target`, with a free field of its own.
pub fn rebuild(raw: &str, reference: NaiveDate, target: &Target, rng: &mut StdRng) -> anyhow::Result<String> {
    let old = boleto::parse(raw, reference).map_err(|e| anyhow::anyhow!("a stored barcode does not parse ({e}): {raw}"))?;
    let d: Vec<u32> = old.raw.chars().map(|c| c.to_digit(10).unwrap()).collect();
    for _ in 0..50 {
        let new = match old.kind {
            Kind::Cobranca => cobranca(&d, &old, target, rng),
            Kind::Arrecadacao => arrecadacao(&d, &old, target, rng),
        };
        let Some(new) = new else { continue };
        let s: String = new.iter().map(|x| char::from_digit(*x, 10).unwrap()).collect();
        if agrees(&s, &old, target, reference) {
            return Ok(s);
        }
    }
    bail!("could not rebuild barcode {raw} so that it reads back")
}

fn cobranca(d: &[u32], old: &boleto::Barcode, t: &Target, rng: &mut StdRng) -> Option<Vec<u32>> {
    let mut n = d.to_vec();
    // An account's own slip (zero factor, zero value) stays one: only its
    // free field changes, and it is the same code every month, as before.
    if !names_an_account(Kind::Cobranca, &old.raw) {
        if let (DueDate::Encoded(_), Some(due)) = (old.due, t.due) {
            put(&mut n[5..9], fator(due)? as u64);
        }
        if let (Amount::Fixed(_), Some(c)) = (old.amount, t.cents) {
            put(&mut n[9..19], u64::try_from(c).ok().filter(|c| *c < 10_000_000_000)?);
        }
    }
    random(&mut n[19..44], rng);
    let mut without: Vec<u32> = n[0..4].to_vec();
    without.extend_from_slice(&n[5..44]);
    n[4] = mod11(&without);
    Some(n)
}

fn arrecadacao(d: &[u32], old: &boleto::Barcode, t: &Target, rng: &mut StdRng) -> Option<Vec<u32>> {
    let mut n = d.to_vec();
    if let (Amount::Fixed(_), Some(c)) = (old.amount, t.cents)
        && matches!(n[2], 6 | 8)
    {
        put(&mut n[4..15], u64::try_from(c).ok().filter(|c| *c < 100_000_000_000)?);
    }
    if let Some(company) = &t.company {
        for (i, ch) in company.chars().take(4).enumerate() {
            n[15 + i] = ch.to_digit(10)?;
        }
    }
    random(&mut n[19..44], rng);
    // The date the free field opened with, moved only if `--years` asked.
    if let DueDate::FreeField(was) = old.due {
        let s = t.due.unwrap_or(was).format("%Y%m%d").to_string();
        for (i, ch) in s.chars().enumerate() {
            n[19 + i] = ch.to_digit(10)?;
        }
    }
    let mut without: Vec<u32> = n[0..3].to_vec();
    without.extend_from_slice(&n[4..44]);
    n[3] = if matches!(n[2], 8 | 9) { mod11(&without) } else { mod10(&without) };
    Some(n)
}

fn agrees(s: &str, old: &boleto::Barcode, t: &Target, reference: NaiveDate) -> bool {
    let Ok(b) = boleto::parse(s, reference) else {
        return false;
    };
    if b.kind != old.kind || names_an_account(b.kind, s) != names_an_account(old.kind, &old.raw) {
        return false;
    }
    let amount_ok = match (old.amount, b.amount) {
        (Amount::Fixed(_), Amount::Fixed(c)) => t.cents.is_none_or(|x| c.get() == x),
        (Amount::Open, Amount::Open) => true,
        (Amount::Reference(a), Amount::Reference(b)) => a == b,
        _ => false,
    };
    // A free field that is random must not happen to read as a date where
    // the original had none, nor the other way round.
    let due_ok = match (old.due, b.due) {
        (DueDate::Absent, DueDate::Absent) => true,
        (DueDate::Encoded(o), DueDate::Encoded(n)) | (DueDate::FreeField(o), DueDate::FreeField(n)) => {
            n == t.due.unwrap_or(o)
        }
        _ => false,
    };
    amount_ok && due_ok
}

/// The fator de vencimento for `due`, under whichever base was in force then.
fn fator(due: NaiveDate) -> Option<u16> {
    let reset = NaiveDate::from_ymd_opt(2025, 2, 22)?;
    let base = if due >= reset {
        NaiveDate::from_ymd_opt(2022, 5, 29)?
    } else {
        NaiveDate::from_ymd_opt(1997, 10, 7)?
    };
    let days = (due - base).num_days();
    u16::try_from(days).ok().filter(|f| (1000..=9999).contains(f))
}

fn put(slot: &mut [u32], mut v: u64) {
    for x in slot.iter_mut().rev() {
        *x = (v % 10) as u32;
        v /= 10;
    }
}

fn random(slot: &mut [u32], rng: &mut StdRng) {
    for x in slot.iter_mut() {
        *x = rng.random_range(0..10);
    }
}

/// The 44 digits in four groups of eleven, the way a page prints them.
pub fn typed(raw: &str) -> String {
    raw.as_bytes()
        .chunks(11)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// A cobrança from boleto.rs's own vectors (R$ 343,62, due 15/09/2026).
    const COBRANCA_LINHA: &str = "34191790010104351004779102015009815700000034362";

    fn cobranca_raw() -> String {
        boleto::parse(COBRANCA_LINHA, d(2026, 9, 21)).unwrap().raw
    }

    #[test]
    fn a_cobranca_carries_its_new_amount_and_keeps_its_date() {
        let mut rng = StdRng::seed_from_u64(1);
        let raw = cobranca_raw();
        let t = Target { cents: Some(51_234), due: None, company: None };
        let new = rebuild(&raw, d(2026, 9, 21), &t, &mut rng).unwrap();
        let b = boleto::parse(&new, d(2026, 9, 21)).unwrap();
        assert_ne!(new, raw);
        assert_eq!(b.amount, Amount::Fixed(ffm::domain::money::Cents(51_234)));
        assert_eq!(b.due, DueDate::Encoded(d(2026, 9, 15)));
    }

    #[test]
    fn a_cobranca_moves_a_year_back() {
        let mut rng = StdRng::seed_from_u64(1);
        let t = Target { cents: Some(100), due: Some(d(2025, 9, 15)), company: None };
        let new = rebuild(&cobranca_raw(), d(2025, 9, 15), &t, &mut rng).unwrap();
        assert_eq!(boleto::parse(&new, d(2025, 9, 15)).unwrap().due, DueDate::Encoded(d(2025, 9, 15)));
    }

    #[test]
    fn an_arrecadacao_keeps_its_free_field_date_and_takes_its_company() {
        // Built here rather than copied from a bill: segment 1, value kind 6,
        // R$ 121,62, company 0306, due 12/01/2026 in the free field.
        let mut rng = StdRng::seed_from_u64(2);
        let body: Vec<u32> = ["816", "00000012162", "0306", "2026011245678901234567890"]
            .concat()
            .chars()
            .map(|c| c.to_digit(10).unwrap())
            .collect();
        let mut n = body[0..3].to_vec();
        n.push(mod10(&body));
        n.extend_from_slice(&body[3..]);
        let raw: String = n.iter().map(|x| char::from_digit(*x, 10).unwrap()).collect();
        let reference = d(2026, 1, 1);
        assert!(boleto::parse(&raw, reference).is_ok(), "fixture parses");

        let t = Target { cents: Some(9_999), due: Some(d(2026, 1, 12)), company: Some("4711".into()) };
        let new = rebuild(&raw, reference, &t, &mut rng).unwrap();
        let b = boleto::parse(&new, reference).unwrap();
        assert_eq!(b.amount, Amount::Fixed(ffm::domain::money::Cents(9_999)));
        assert_eq!(b.due, DueDate::FreeField(d(2026, 1, 12)));
        assert_eq!(b.company.as_deref(), Some("4711"));
    }

    #[test]
    fn an_arrecadacao_keeps_its_date_when_nothing_moves_it() {
        let mut rng = StdRng::seed_from_u64(5);
        let body: Vec<u32> = ["816", "00000012162", "0306", "2026011245678901234567890"]
            .concat()
            .chars()
            .map(|c| c.to_digit(10).unwrap())
            .collect();
        let mut n = body[0..3].to_vec();
        n.push(mod10(&body));
        n.extend_from_slice(&body[3..]);
        let raw: String = n.iter().map(|x| char::from_digit(*x, 10).unwrap()).collect();
        let t = Target { cents: Some(5_000), due: None, company: None };
        let new = rebuild(&raw, d(2026, 1, 1), &t, &mut rng).unwrap();
        assert_eq!(boleto::parse(&new, d(2026, 1, 1)).unwrap().due, DueDate::FreeField(d(2026, 1, 12)));
    }

    #[test]
    fn an_accounts_own_slip_stays_one() {
        // Zero factor and zero value: an issuer's every-month slip.
        let body: Vec<u32> = ["0779", "0000", "0000000000", "1234567890123456789012345"]
            .concat()
            .chars()
            .map(|c| c.to_digit(10).unwrap())
            .collect();
        let mut n = body[0..4].to_vec();
        n.push(mod11(&body));
        n.extend_from_slice(&body[4..]);
        let raw: String = n.iter().map(|x| char::from_digit(*x, 10).unwrap()).collect();
        let reference = d(2026, 1, 1);
        assert!(names_an_account(Kind::Cobranca, &raw), "fixture names an account");

        let mut rng = StdRng::seed_from_u64(3);
        let t = Target { cents: Some(777), due: None, company: None };
        let new = rebuild(&raw, reference, &t, &mut rng).unwrap();
        assert!(names_an_account(Kind::Cobranca, &new));
        assert_eq!(boleto::parse(&new, reference).unwrap().amount, Amount::Open);
    }
}
