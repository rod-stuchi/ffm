//! Made-up amounts that still behave like a household's.
//!
//! White noise would make every month-over-month delta absurd and every fixed
//! bill wobble. So each **scope** — a rule, or a one-off's title — gets one
//! factor, and each distinct value inside it a small jitter, both derived by
//! hashing rather than drawn: the same original value always comes out the
//! same, so a fixed bill stays fixed, installments stay equal, a carried
//! amount still matches, and the change log's old → new agrees with the rows.

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Rule(i64),
    /// A one-off, by its original title, lowercased.
    Title(String),
}

pub struct AmountMap {
    seed: u64,
}

impl AmountMap {
    pub fn new(seed: u64) -> AmountMap {
        AmountMap { seed }
    }

    /// `salt` tells apart the copies `--scale` and `--years` make of a scope.
    pub fn map(&self, salt: u32, scope: &Scope, cents: i64) -> i64 {
        if cents == 0 {
            return 0;
        }
        let v = cents.unsigned_abs() as f64;
        let factor = 0.4 + 1.5 * self.unit(salt, scope, None);
        let jitter = (self.unit(salt, scope, Some(cents.unsigned_abs())) - 0.5) * 0.2;
        let raw = v * factor * (1.0 + jitter);
        // A value in whole reais stays in whole reais: rent is R$ 2.300,00,
        // not R$ 2.317,46.
        let out = if cents % 100 == 0 {
            ((raw / 100.0).round() * 100.0).max(100.0)
        } else {
            raw.round().max(1.0)
        } as i64;
        out * cents.signum()
    }

    /// A number in [0, 1), from the seed, the scope and (for the jitter) the
    /// value.
    fn unit(&self, salt: u32, scope: &Scope, value: Option<u64>) -> f64 {
        let mut h = blake3::Hasher::new();
        h.update(&self.seed.to_le_bytes());
        h.update(&salt.to_le_bytes());
        match scope {
            Scope::Rule(id) => {
                h.update(b"r");
                h.update(&id.to_le_bytes());
            }
            Scope::Title(t) => {
                h.update(b"t");
                h.update(t.as_bytes());
            }
        }
        if let Some(v) = value {
            h.update(b"v");
            h.update(&v.to_le_bytes());
        }
        let b = h.finalize();
        let n = u64::from_le_bytes(b.as_bytes()[..8].try_into().expect("8 bytes"));
        (n >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_value_in_the_same_scope_maps_the_same() {
        let m = AmountMap::new(7);
        let s = Scope::Rule(3);
        assert_eq!(m.map(0, &s, 12_345), m.map(0, &s, 12_345));
        assert_eq!(AmountMap::new(7).map(0, &s, 999), m.map(0, &s, 999));
    }

    #[test]
    fn zero_and_sign_and_whole_reais_survive() {
        let m = AmountMap::new(1);
        let s = Scope::Title("padaria".into());
        assert_eq!(m.map(0, &s, 0), 0);
        assert!(m.map(0, &s, -5_000) < 0);
        assert_eq!(m.map(0, &s, 230_000) % 100, 0);
    }

    #[test]
    fn another_seed_or_salt_moves_it() {
        let s = Scope::Rule(1);
        let a = AmountMap::new(1).map(0, &s, 100_001);
        assert_ne!(a, AmountMap::new(2).map(0, &s, 100_001));
        assert_ne!(a, AmountMap::new(1).map(1, &s, 100_001));
    }
}
