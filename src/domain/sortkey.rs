//! Fractional sort keys — where a row sits, as a string between two others.
//!
//! Not an integer `position`. With two people entering transactions, moving a
//! row would renumber every row after it: N writes for one gesture, and two
//! people doing it at once produce a conflict that SQLite cannot help with.
//!
//! A fractional index writes **one row**. A key is a string over an
//! ASCII-ordered alphabet, and there is always another string strictly between
//! any two — so a move is `sort_key = between(prev, next)` and `ORDER BY
//! sort_key` stays a plain index scan on a BINARY-collated column.
//!
//! **A key never ends in the lowest character (`0`).** That is the invariant
//! the whole scheme rests on: `"a0"` would have nothing below it inside the
//! `a` prefix, so "insert above this row" would have nowhere to go. Nothing
//! here ever emits such a key, and [`between`] asserts it of its inputs in
//! debug builds — a hand-written key in a fixture is the only way one gets in.
//!
//! The alphabet is ordered exactly as ASCII orders it (`0` < `A` < `a`), which
//! is what makes SQLite's default string comparison agree with this module. Do
//! not reorder it to put lowercase first "for tidiness" — every stored key in
//! the household would change meaning at once.

const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn index_of(c: u8) -> usize {
    ALPHABET.iter().position(|&a| a == c).unwrap_or(0)
}

fn char_at(i: usize) -> char {
    ALPHABET[i.min(ALPHABET.len() - 1)] as char
}

/// A key strictly between `prev` and `next`, either of which may be absent —
/// `None` meaning "nothing above" or "nothing below".
///
/// Both ends absent is the first row in an empty pane, which gets a key from
/// the middle of the alphabet so the first insert in either direction is as
/// cheap as any other.
pub fn between(prev: Option<&str>, next: Option<&str>) -> String {
    debug_assert!(
        prev.is_none_or(valid) && next.is_none_or(valid),
        "a sort key must not end in {:?}: {prev:?} / {next:?}",
        ALPHABET[0] as char
    );
    match (prev, next) {
        (None, None) => char_at(ALPHABET.len() / 2).to_string(),
        (None, Some(b)) => before(b),
        (Some(a), None) => after(a),
        (Some(a), Some(b)) => {
            // A caller that has the two neighbors the wrong way round would
            // otherwise get a key that sorts outside them, which is a bug that
            // only shows up as rows drifting later.
            debug_assert!(a < b, "sort keys out of order: {a:?} >= {b:?}");
            if a >= b {
                return after(a);
            }
            midpoint(a, Some(b))
        }
    }
}

/// A key after `a` and after everything else — appending, which is what a new
/// row and a re-attribution both do.
pub fn after(a: &str) -> String {
    midpoint(a, None)
}

/// A key before `b`. Income lands at the top of its pane at creation, which is
/// the one place this is used.
pub fn before(b: &str) -> String {
    midpoint("", Some(b))
}

/// A key nothing here would ever produce: empty, or ending in the lowest
/// character, which leaves no room above it inside its own prefix.
pub fn valid(key: &str) -> bool {
    !key.is_empty() && !key.ends_with(ALPHABET[0] as char)
}

/// The recursive half. `a` is the lower bound (possibly empty, meaning "the
/// very start"), `b` the upper bound or `None` for "no upper bound".
fn midpoint(a: &str, b: Option<&str>) -> String {
    if let Some(b) = b {
        // Share the common prefix and solve the shorter problem underneath it.
        // Without this, `between("aa", "ab")` has nowhere to go: the first
        // characters are equal, so the digits carry no information.
        let common = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
        if common > 0 {
            return format!(
                "{}{}",
                &b[..common],
                midpoint(&a[common..], Some(&b[common..]))
            );
        }
    }

    let digit_a = a.bytes().next().map(index_of).unwrap_or(0);
    let digit_b = match b.and_then(|b| b.bytes().next()) {
        Some(c) => index_of(c),
        // No upper bound: pretend there is one just past the end of the
        // alphabet, so appending walks up through it.
        None => ALPHABET.len(),
    };

    if digit_b - digit_a > 1 {
        // Room for a whole character between them; take the middle.
        return char_at((digit_a + digit_b) / 2).to_string();
    }

    match b {
        // Adjacent digits with an upper bound that has more to give: keep its
        // first character and recurse into what follows.
        Some(b) if b.len() > 1 => format!("{}{}", &b[..1], midpoint("", Some(&b[1..]))),
        // Otherwise grow `a` by a character — the string gets one longer, which
        // is the price of an insert into a gap that has run out of room.
        _ => format!(
            "{}{}",
            char_at(digit_a),
            midpoint(a.get(1..).unwrap_or(""), None)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_pane_gets_a_key_with_room_on_both_sides() {
        let first = between(None, None);
        assert!(!first.is_empty());
        // Room above and below, so neither direction is the expensive one.
        assert!(between(None, Some(&first)) < first);
        assert!(between(Some(&first), None) > first);
    }

    #[test]
    fn a_key_always_lands_strictly_between_its_neighbors() {
        let a = between(None, None);
        let b = after(&a);
        let mid = between(Some(&a), Some(&b));
        assert!(a < mid && mid < b, "{a} < {mid} < {b}");
    }

    #[test]
    fn inserting_into_the_same_gap_a_thousand_times_keeps_working() {
        // The whole reason for a fractional index: a gap never runs out. The
        // key grows a character when it has to, and never renumbers a neighbor.
        let low = between(None, None);
        let high = after(&low);
        let mut upper = high.clone();

        for i in 0..1000 {
            let mid = between(Some(&low), Some(&upper));
            assert!(low < mid, "iteration {i}: {low} < {mid}");
            assert!(mid < upper, "iteration {i}: {mid} < {upper}");
            upper = mid;
        }
    }

    #[test]
    fn appending_a_thousand_rows_stays_ordered() {
        let mut keys = vec![between(None, None)];
        for _ in 0..1000 {
            let next = after(keys.last().unwrap());
            keys.push(next);
        }
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "append must produce ascending keys");
    }

    #[test]
    fn prepending_a_thousand_rows_stays_ordered() {
        // Income lands at the top of its pane, so this direction is used as
        // much as appending is.
        let mut keys = vec![between(None, None)];
        for _ in 0..1000 {
            let prev = before(keys.first().unwrap());
            keys.insert(0, prev);
        }
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "prepend must produce descending keys");
    }

    #[test]
    fn the_alphabet_sorts_the_way_sqlite_compares_strings() {
        // SQLite's default BINARY collation compares bytes. If this alphabet
        // were reordered, every stored key in the household would change
        // meaning at once — so it is asserted rather than assumed.
        let mut bytes = ALPHABET.to_vec();
        bytes.sort_unstable();
        assert_eq!(
            bytes, ALPHABET,
            "the alphabet must already be in byte order"
        );
        assert_eq!(ALPHABET.len(), 62);
    }

    #[test]
    fn keys_from_the_seed_still_interleave() {
        // The seed hand-writes single letters — a new row between two of them
        // must not need either of them rewritten.
        let mid = between(Some("a"), Some("b"));
        assert!("a" < mid.as_str() && mid.as_str() < "b", "{mid}");
        let again = between(Some("a"), Some(&mid));
        assert!("a" < again.as_str() && again < mid, "{again} / {mid}");
    }

    #[test]
    fn a_key_ending_in_the_lowest_character_is_not_one_this_module_makes() {
        // "a0" has nothing below it inside the `a` prefix, so inserting above
        // that row would have nowhere to go. Nothing here emits such a key.
        assert!(!valid("a0"));
        assert!(!valid(""));
        assert!(valid("a"));

        let mut key = between(None, None);
        for _ in 0..200 {
            assert!(valid(&key), "{key}");
            key = before(&key);
        }
    }

    #[test]
    #[should_panic(expected = "out of order")]
    fn backwards_neighbors_are_a_bug_and_say_so() {
        // In release this falls back to appending rather than corrupting the
        // ordering around it; in dev it is loud, because a caller holding its
        // neighbors the wrong way round only shows up later as rows drifting.
        between(Some("b"), Some("a"));
    }
}
