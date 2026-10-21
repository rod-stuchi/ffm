//! Fake identifiers that pass the same checks the real ones do: a CNPJ with
//! valid check digits, an account number in its original shape, a uuid.
//! Each map is consistent — one original, one fake, everywhere it appears.

use ffm::services::issuers::cnpj_valid;
use rand::Rng;
use rand::rngs::StdRng;
use std::collections::{HashMap, HashSet};

/// One original → one fake, never a fake that is also an original, never two
/// originals → one fake.
#[derive(Default)]
pub struct IdMap {
    map: HashMap<String, String>,
    used: HashSet<String>,
}

impl IdMap {
    pub fn get_or(
        &mut self,
        original: &str,
        forbidden: &HashSet<String>,
        mut make: impl FnMut() -> String,
    ) -> String {
        if let Some(v) = self.map.get(original) {
            return v.clone();
        }
        let fake = loop {
            let c = make();
            if c != original && !forbidden.contains(&c) && !self.used.contains(&c) {
                break c;
            }
        };
        self.used.insert(fake.clone());
        self.map.insert(original.to_string(), fake.clone());
        fake
    }

    pub fn pairs(&self) -> impl Iterator<Item = (&String, &String)> {
        self.map.iter()
    }
}

/// 14 digits, a head office (`0001`), and its two check digits.
pub fn cnpj(rng: &mut StdRng) -> String {
    loop {
        let mut d: Vec<u32> = (0..8).map(|_| rng.random_range(0..10)).collect();
        d.extend([0, 0, 0, 1]);
        for len in [12, 13] {
            let weights = [6u32, 5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
            let w = &weights[13 - len..];
            let sum: u32 = d[..len].iter().zip(w).map(|(a, b)| a * b).sum();
            let r = sum % 11;
            d.push(if r < 2 { 0 } else { 11 - r });
        }
        let s: String = d
            .iter()
            .map(|x| char::from_digit(*x, 10).unwrap())
            .collect();
        if cnpj_valid(&s) {
            return s;
        }
    }
}

/// Every digit replaced, every separator kept: `24.323.019.00` stays that
/// shape, so whatever reads it still reads it.
pub fn same_shape(original: &str, rng: &mut StdRng) -> String {
    original
        .chars()
        .map(|c| {
            if c.is_ascii_digit() {
                char::from_digit(rng.random_range(0..10), 10).unwrap()
            } else if c.is_ascii_alphabetic() {
                let base = if c.is_ascii_uppercase() { b'A' } else { b'a' };
                (base + rng.random_range(0..26u8)) as char
            } else {
                c
            }
        })
        .collect()
}

pub fn digits(n: usize, rng: &mut StdRng) -> String {
    (0..n)
        .map(|_| char::from_digit(rng.random_range(0..10), 10).unwrap())
        .collect()
}

/// A version-4 uuid, from the seeded generator so a copy is reproducible.
pub fn uuid(rng: &mut StdRng) -> String {
    let mut b: [u8; 16] = rng.random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn a_fake_cnpj_passes_the_real_check() {
        let mut rng = StdRng::seed_from_u64(9);
        for _ in 0..200 {
            assert!(cnpj_valid(&cnpj(&mut rng)));
        }
    }

    #[test]
    fn the_shape_is_kept() {
        let mut rng = StdRng::seed_from_u64(9);
        let s = same_shape("24.323.019.00", &mut rng);
        assert_eq!(s.len(), 13);
        assert_eq!(s.chars().filter(|c| *c == '.').count(), 3);
    }

    #[test]
    fn one_original_one_fake() {
        let mut rng = StdRng::seed_from_u64(9);
        let mut m = IdMap::default();
        let none = HashSet::new();
        let a = m.get_or("x", &none, || digits(4, &mut rng));
        assert_eq!(m.get_or("x", &none, || digits(4, &mut rng)), a);
    }
}
