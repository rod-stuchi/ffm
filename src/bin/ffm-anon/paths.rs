//! Rewriting stored vault paths after the names they are built from changed.
//!
//! A stored path is `{slug}/{bucket}/{bill}[/{instance}]/{year}/{file}`, every
//! folder a slug of a value this tool replaced. So it is rewritten segment by
//! segment — the member's folder by the member's new slug, each taxonomy slug
//! by the slug of its replacement, a receipt's `_brl550.00` by its mapped
//! amount — rather than rebuilt: the path is whatever the app wrote, legacy
//! layouts included, and the app's own startup pass (`vault::refile_all`)
//! moves anything this leaves out of place.

use ffm::domain::money::{Cents, Currency};
use ffm::domain::slug::slugify;
use ffm::ui::fmt::money_filename;
use std::collections::HashMap;

#[derive(Default)]
pub struct PathMap {
    /// Old slug → new slug, for folders below the member's.
    pub segments: HashMap<String, String>,
    /// Old member folder → new.
    pub members: HashMap<String, String>,
}

impl PathMap {
    pub fn learn(&mut self, old: &str, new: &str) {
        let (o, n) = (slugify(old), slugify(new));
        if !o.is_empty() && !n.is_empty() {
            self.segments.entry(o).or_insert(n);
        }
    }

    /// `amount` maps a filename's amount (in cents) to the new one.
    /// `unknown` names a folder nothing taught us — an old rule's bucket.
    pub fn map(
        &self,
        path: &str,
        currency: Currency,
        amount: impl Fn(i64) -> i64,
        mut unknown: impl FnMut(&str) -> String,
    ) -> String {
        let parts: Vec<&str> = path.split('/').collect();
        let last = parts.len().saturating_sub(1);
        parts
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                if i == 0 {
                    self.members.get(*seg).cloned().unwrap_or_else(|| unknown(seg))
                } else if i == last {
                    filename(seg, currency, &amount)
                } else if *seg == ".delete" || is_year(seg) {
                    seg.to_string()
                } else {
                    self.segments.get(*seg).cloned().unwrap_or_else(|| unknown(seg))
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn is_year(s: &str) -> bool {
    s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit())
}

/// `09_brl550.00.r42.pdf` → `09_brl{new}.r42.pdf`. Anything else is the
/// app's own name (`09.pdf`, `09_barcode.pdf`) and stays.
pub fn filename(name: &str, currency: Currency, amount: impl Fn(i64) -> i64) -> String {
    let tag = currency.filename_tag();
    let Some(rest) = name.get(2..).and_then(|r| r.strip_prefix('_')).and_then(|r| r.strip_prefix(tag)) else {
        return name.to_string();
    };
    if !name[..2].bytes().all(|b| b.is_ascii_digit()) {
        return name.to_string();
    }
    let units: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let after_units = &rest[units.len()..];
    let Some(after_dot) = after_units.strip_prefix('.') else {
        return name.to_string();
    };
    let frac: String = after_dot.chars().take_while(|c| c.is_ascii_digit()).collect();
    let tail = &after_dot[frac.len()..];
    let decimals = currency.format().decimals;
    let (Ok(u), Ok(f)) = (units.parse::<i64>(), frac.parse::<i64>()) else {
        return name.to_string();
    };
    if frac.len() != decimals as usize {
        return name.to_string();
    }
    let cents = u * 10_i64.pow(decimals) + f;
    format!("{}_{}{tail}", &name[..2], money_filename(Cents(amount(cents)), currency))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_segment_follows_its_value() {
        let mut m = PathMap::default();
        m.members.insert("rodrigo".into(), "ana".into());
        m.learn("Cartão", "Moradia");
        m.learn("Nu Bank", "Luz do Vale");
        let p = m.map(
            "rodrigo/.delete/cartão/nu_bank/2026/09_brl550.00.r4.pdf",
            Currency::Brl,
            |c| c * 2,
            |s| panic!("{s} is known"),
        );
        assert_eq!(p, "ana/.delete/moradia/luz_do_vale/2026/09_brl1100.00.r4.pdf");
    }

    #[test]
    fn the_apps_own_names_stay() {
        assert_eq!(filename("09_barcode.pdf", Currency::Brl, |c| c), "09_barcode.pdf");
        assert_eq!(filename("09.png", Currency::Brl, |c| c), "09.png");
    }
}
