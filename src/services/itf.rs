//! Interleaved 2 of 5 — the symbology of every Brazilian bill's barcode,
//! bank slip and utility payment alike.
//!
//! Digits are encoded in pairs: the first digit of a pair on the five bars,
//! the second on the five spaces between them. Each digit is five elements,
//! two of them wide. A start pattern (`narrow bar, narrow space` twice) and a
//! stop pattern (`wide bar, narrow space, narrow bar`) frame the pairs. Wide
//! is three times narrow, the FEBRABAN ratio, so a 44-digit barcode is 405
//! narrow units wide: 22 pairs of 18, plus 4 for the start and 5 for the stop.
//!
//! Pure, so it is tested by decoding its own output — there is no scanner on
//! the build machine, and a round trip is what a scanner does.

/// A wide element is this many narrow ones.
pub const WIDE: u32 = 3;

/// One bar or space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Element {
    /// A bar (ink) or a space (paper).
    pub bar: bool,
    pub wide: bool,
}

impl Element {
    /// Width in narrow units.
    pub fn units(self) -> u32 {
        if self.wide { WIDE } else { 1 }
    }
}

/// Each digit's five elements, `true` for wide.
const PATTERNS: [[bool; 5]; 10] = [
    [false, false, true, true, false], // 0  NNWWN
    [true, false, false, false, true], // 1  WNNNW
    [false, true, false, false, true], // 2  NWNNW
    [true, true, false, false, false], // 3  WWNNN
    [false, false, true, false, true], // 4  NNWNW
    [true, false, true, false, false], // 5  WNWNN
    [false, true, true, false, false], // 6  NWWNN
    [false, false, false, true, true], // 7  NNNWW
    [true, false, false, true, false], // 8  WNNWN
    [false, true, false, true, false], // 9  NWNWN
];

/// The bars and spaces for `digits`, start and stop included. `None` for an
/// odd number of digits or anything that is not a digit: ITF has no way to
/// say either.
pub fn encode(digits: &str) -> Option<Vec<Element>> {
    let ds: Vec<usize> = digits
        .chars()
        .map(|c| c.to_digit(10).map(|d| d as usize))
        .collect::<Option<_>>()?;
    if ds.is_empty() || !ds.len().is_multiple_of(2) {
        return None;
    }

    let narrow = |bar| Element { bar, wide: false };
    let mut out = vec![narrow(true), narrow(false), narrow(true), narrow(false)];
    for pair in ds.chunks(2) {
        let (a, b) = (PATTERNS[pair[0]], PATTERNS[pair[1]]);
        for i in 0..5 {
            out.push(Element {
                bar: true,
                wide: a[i],
            });
            out.push(Element {
                bar: false,
                wide: b[i],
            });
        }
    }
    out.push(Element {
        bar: true,
        wide: true,
    });
    out.push(narrow(false));
    out.push(narrow(true));
    Some(out)
}

/// The whole symbol's width in narrow units.
pub fn units(elements: &[Element]) -> u32 {
    elements.iter().map(|e| e.units()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a scanner does: strip the start and stop, read each run of ten
    /// elements back as a pair of digits.
    fn decode(elements: &[Element]) -> String {
        let body = &elements[4..elements.len() - 3];
        let digit = |wide: [bool; 5]| -> char {
            let d = PATTERNS.iter().position(|p| *p == wide).expect("a pattern");
            char::from_digit(d as u32, 10).unwrap()
        };
        let mut out = String::new();
        for pair in body.chunks(10) {
            let mut a = [false; 5];
            let mut b = [false; 5];
            for i in 0..5 {
                assert!(
                    pair[2 * i].bar && !pair[2 * i + 1].bar,
                    "bars and spaces alternate"
                );
                a[i] = pair[2 * i].wide;
                b[i] = pair[2 * i + 1].wide;
            }
            out.push(digit(a));
            out.push(digit(b));
        }
        out
    }

    #[test]
    fn every_digit_has_exactly_two_wide_elements() {
        for p in PATTERNS {
            assert_eq!(p.iter().filter(|w| **w).count(), 2);
        }
    }

    #[test]
    fn a_bills_44_digits_are_405_narrow_units() {
        let code = "82660000000053100240010009768220110202620003";
        let e = encode(code).unwrap();
        assert_eq!(units(&e), 405);
        // Bars and spaces alternate, starting and ending on a bar.
        assert!(e.first().unwrap().bar && e.last().unwrap().bar);
        assert!(e.windows(2).all(|w| w[0].bar != w[1].bar));
    }

    #[test]
    fn what_is_drawn_reads_back_as_the_code() {
        for code in [
            "82660000000053100240010009768220110202620003",
            "34191157900000343621790010104351004779102010",
            "00",
            "0123456789",
        ] {
            assert_eq!(decode(&encode(code).unwrap()), code);
        }
    }

    #[test]
    fn an_odd_count_or_a_letter_is_refused() {
        assert_eq!(encode("123"), None);
        assert_eq!(encode("12a4"), None);
        assert_eq!(encode(""), None);
    }
}
