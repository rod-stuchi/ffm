//! One file, several months' bills: which code belongs to which month.
//!
//! A city's IPTU arrives as one PDF holding eleven codes on three pages — the
//! single payment (*ÚNICA*, in full with a discount) and ten installments —
//! and a loan's carnê the same way. "Page N is month N" cannot describe that,
//! so a rule whose bill covers several months (`spans_period`) maps codes to
//! months by **the due date each code carries**: a utility code in its free
//! field, a bank slip in its date factor. No page count, no first month to
//! type.
//!
//! - Two codes due in one month: the one **whose value repeats in the file**
//!   is the installment, and wins — the ÚNICA is due with the first
//!   installment and paid instead of all of them, not with them.
//! - Codes that carry no date at all go, in the order the file prints them,
//!   to the rule's active months of the year (the same repeating-value filter
//!   first, so a single payment does not take January's place).
//!
//! Pure: text in, pairs out. `web::import` stores and stamps them.

use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::services::boleto::{self, Amount, Barcode};
use chrono::{Datelike, NaiveDate};

/// One valid code in a bill, and the page it is printed on (1-based).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundCode {
    pub barcode: Barcode,
    pub page: u32,
}

/// Every valid code in a bill's text, in the order it prints them, each once.
/// `pdftotext` separates pages with a form feed, which is how a code knows
/// its page.
pub fn codes(text: &str, reference: NaiveDate) -> Vec<FoundCode> {
    let mut out: Vec<FoundCode> = Vec::new();
    for (i, page) in text.split('\x0c').enumerate() {
        for candidate in crate::services::import::candidates(page) {
            let Ok(barcode) = boleto::parse(&candidate, reference) else {
                continue;
            };
            if out.iter().any(|f| f.barcode.raw == barcode.raw) {
                continue;
            }
            out.push(FoundCode {
                barcode,
                page: i as u32 + 1,
            });
        }
    }
    out
}

fn fixed(b: &Barcode) -> Option<Cents> {
    match b.amount {
        Amount::Fixed(c) => Some(c),
        _ => None,
    }
}

/// How many codes in the file share this code's value — an installment's is
/// more than one, a single payment's is one.
fn repeats(code: &FoundCode, all: &[FoundCode]) -> usize {
    match fixed(&code.barcode) {
        Some(v) => all.iter().filter(|c| fixed(&c.barcode) == Some(v)).count(),
        None => 1,
    }
}

/// One code per month. `active` is the rule's months of the year (`None` for
/// every month) and `year` the cycle a dateless file is for — both only for
/// the fallback.
pub fn installments(
    all: &[FoundCode],
    active: Option<&[u32]>,
    year: i32,
) -> Vec<(Period, FoundCode)> {
    let dated: Vec<(Period, &FoundCode)> = all
        .iter()
        .filter_map(|c| {
            let d = c.barcode.due.date()?;
            Some((Period::new(d.year(), d.month())?, c))
        })
        .collect();

    if !dated.is_empty() {
        let mut out: Vec<(Period, FoundCode)> = Vec::new();
        for (p, c) in dated {
            match out.iter_mut().find(|(q, _)| *q == p) {
                // Two in one month: the installment, whose value repeats,
                // over the single payment, whose value does not.
                Some((_, kept)) if repeats(c, all) > repeats(kept, all) => *kept = c.clone(),
                Some(_) => {}
                None => out.push((p, c.clone())),
            }
        }
        out.sort_by_key(|(p, _)| *p);
        return out;
    }

    // No dates: printed order, onto the active months. A value that appears
    // once is a single payment when others repeat, and is left out.
    let any_repeat = all.iter().any(|c| repeats(c, all) > 1);
    let codes: Vec<&FoundCode> = all
        .iter()
        .filter(|c| !any_repeat || repeats(c, all) > 1)
        .collect();
    let months: Vec<u32> = match active {
        Some(m) => m.to_vec(),
        None => (1..=12).collect(),
    };
    months
        .into_iter()
        .zip(codes)
        .filter_map(|(m, c)| Some((Period::new(year, m)?, c.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::boleto::mod10;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 7).unwrap()
    }

    /// A 48-digit utility linha (segment 1, prefeitura; value kind 6) for
    /// `cents`, due `yyyymmdd` in the free field — or a dateless one when
    /// `due` is `None`. Made up: built with the app's own check digits.
    fn linha(cents: u64, due: Option<&str>, serial: u32) -> String {
        let mut d: Vec<u32> = vec![8, 1, 6, 0];
        d.extend(
            format!("{cents:011}")
                .chars()
                .map(|c| c.to_digit(10).unwrap()),
        );
        d.extend([0, 3, 0, 6]);
        let free = match due {
            Some(ymd) => format!("{ymd}{serial:017}"),
            None => format!("9{serial:024}"),
        };
        d.extend(free.chars().map(|c| c.to_digit(10).unwrap()));
        let mut body = d[0..3].to_vec();
        body.extend_from_slice(&d[4..]);
        d[3] = mod10(&body);
        d.chunks(11)
            .map(|b| {
                let s: String = b
                    .iter()
                    .map(|x| char::from_digit(*x, 10).unwrap())
                    .collect();
                format!("{s} {}", mod10(b))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The shape of a city's IPTU: the single payment and ten installments,
    /// over three pages.
    fn iptu() -> String {
        let mut pages = [String::new(), String::new(), String::new()];
        pages[0].push_str(&format!("ÚNICA\n{}\n", linha(117969, Some("20260112"), 0)));
        for i in 1..=10u32 {
            let day = if i == 1 { "12" } else { "10" };
            let line = format!(
                "PARCELA {i}\n{}\n",
                linha(12162, Some(&format!("2026{i:02}{day}")), i)
            );
            pages[((i as usize) / 4).min(2)].push_str(&line);
        }
        pages.join("\x0c")
    }

    #[test]
    fn every_code_in_the_file_is_found_with_its_page() {
        let found = codes(&iptu(), today());
        assert_eq!(found.len(), 11);
        assert_eq!(found[0].page, 1);
        assert_eq!(found.last().unwrap().page, 3);
    }

    #[test]
    fn each_month_takes_the_installment_due_in_it() {
        let plan = installments(&codes(&iptu(), today()), None, 2026);
        assert_eq!(plan.len(), 10, "Jan–Oct");
        assert_eq!(plan[0].0, Period::new(2026, 1).unwrap());
        assert_eq!(plan[9].0, Period::new(2026, 10).unwrap());
        // January has the single payment AND installment 1: the installment.
        assert!(
            plan.iter()
                .all(|(_, c)| c.barcode.amount == Amount::Fixed(Cents(12162)))
        );
        assert_eq!(
            plan[9].1.barcode.due.date(),
            NaiveDate::from_ymd_opt(2026, 10, 10)
        );
    }

    #[test]
    fn dateless_codes_go_to_the_active_months_in_order_without_the_single_payment() {
        let mut text = format!("{}\n", linha(117969, None, 0));
        for i in 1..=4 {
            text.push_str(&format!("{}\n", linha(12162, None, i)));
        }
        let plan = installments(&codes(&text, today()), Some(&[2, 3, 5, 6]), 2026);
        let months: Vec<u32> = plan.iter().map(|(p, _)| p.month()).collect();
        assert_eq!(months, vec![2, 3, 5, 6]);
        assert!(
            plan.iter()
                .all(|(_, c)| c.barcode.amount == Amount::Fixed(Cents(12162)))
        );
    }
}
