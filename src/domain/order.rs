//! A pane's manual order, carried from one month to another.
//!
//! Months are views, so a row in September and "the same row" in October are
//! two transactions with nothing in common but what they say. Two things need
//! to line them up anyway:
//!
//! - **`:order`** copies the month on screen's manual order to other months:
//!   each target month's rows are matched to this month's and put in the same
//!   order.
//! - **The materializer** puts a new month's occurrence where its rule sat the
//!   month before, so a month arrives in the order the last one was left in.
//!
//! Both match the same way, here — **the rule first, then the exact title**:
//! the same "same line" the cursor uses to follow a row to another month, and
//! the MoM delta uses for a one-off. A rule has at most one row per month
//! (`UNIQUE(rule_id, period)`); titles repeat (four "Mercado" rows), so the
//! k-th row with a title matches the k-th.
//!
//! Pure: no SQL, no I/O. A [`Line`] is what a pane query hands over.

use crate::domain::period::Period;
use crate::domain::sortkey;
use std::collections::HashMap;

/// One live row of a pane, in the order the pane shows it (`sort_key`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub id: i64,
    pub rule_id: Option<i64>,
    pub title: String,
    pub key: String,
}

/// For each `target` row, the index of the `source` row it is the same line
/// as, if any. Rule first, then the k-th unmatched row of a title ↔ the k-th.
pub fn counterparts(source: &[Line], target: &[Line]) -> Vec<Option<usize>> {
    let mut matched: Vec<Option<usize>> = vec![None; target.len()];
    let mut taken = vec![false; source.len()];

    let mut by_rule: HashMap<i64, usize> = HashMap::new();
    for (i, s) in source.iter().enumerate() {
        if let Some(r) = s.rule_id {
            by_rule.entry(r).or_insert(i);
        }
    }
    for (j, t) in target.iter().enumerate() {
        if let Some(&i) = t.rule_id.and_then(|r| by_rule.get(&r))
            && !taken[i]
        {
            matched[j] = Some(i);
            taken[i] = true;
        }
    }

    // What the rule could not match, the title does — in order on both sides,
    // so the second "Mercado" here is the second "Mercado" there.
    let mut by_title: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, s) in source.iter().enumerate().rev() {
        if !taken[i] {
            by_title.entry(s.title.as_str()).or_default().push(i);
        }
    }
    for (j, t) in target.iter().enumerate() {
        if matched[j].is_none()
            && let Some(i) = by_title.get_mut(t.title.as_str()).and_then(Vec::pop)
        {
            matched[j] = Some(i);
        }
    }
    matched
}

/// The order `target` should be in to follow `source`: indices into
/// `target`. The rows that have a counterpart come first, in the source's
/// order; the rest follow them, in the order they are already in.
pub fn desired(source: &[Line], target: &[Line]) -> Vec<usize> {
    let pairs = counterparts(source, target);
    let mut matched: Vec<(usize, usize)> = pairs
        .iter()
        .enumerate()
        .filter_map(|(j, i)| i.map(|i| (i, j)))
        .collect();
    matched.sort_unstable();
    matched
        .into_iter()
        .map(|(_, j)| j)
        .chain((0..target.len()).filter(|j| pairs[*j].is_none()))
        .collect()
}

/// Which of `keys` (already in the wanted order) can keep the key they have:
/// the longest run that is **strictly** increasing. Everything else gets a
/// new key between the kept ones around it — so a month already nearly in
/// order costs a couple of writes, not one per row.
///
/// Strictly, because two live rows can share a key (a re-attribution appends
/// with a key made in another pane), and two kept rows with one key leave no
/// room between them.
pub fn stay(keys: &[&str]) -> Vec<bool> {
    let n = keys.len();
    // length of the longest strictly increasing run ending at i, and where it
    // came from — n is a pane's worth of rows, so the square is nothing.
    let mut len = vec![1usize; n];
    let mut prev = vec![usize::MAX; n];
    for i in 0..n {
        for j in 0..i {
            if keys[j] < keys[i] && len[j] + 1 > len[i] {
                len[i] = len[j] + 1;
                prev[i] = j;
            }
        }
    }
    let mut keep = vec![false; n];
    let Some(mut at) = (0..n).max_by_key(|&i| (len[i], std::cmp::Reverse(i))) else {
        return keep;
    };
    loop {
        keep[at] = true;
        if prev[at] == usize::MAX {
            break;
        }
        at = prev[at];
    }
    keep
}

/// New keys for `keys`, already in the wanted order: `None` where the row
/// keeps its key, a key spread between its kept neighbors where it does not.
/// The result is strictly increasing.
pub fn rekey(keys: &[&str]) -> Vec<Option<String>> {
    let keep = stay(keys);
    let mut out: Vec<Option<String>> = vec![None; keys.len()];
    let mut i = 0;
    while i < keys.len() {
        if keep[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < keys.len() && !keep[i] {
            i += 1;
        }
        let lo = start.checked_sub(1).map(|k| keys[k]);
        let hi = keys.get(i).copied();
        for (k, key) in sortkey::spread(lo, hi, i - start).into_iter().enumerate() {
            out[start + k] = Some(key);
        }
    }
    out
}

/// One row the copy moves: its id, the key it had and the one it gets, and
/// where it sat and sits (1-based), which is what its log entry says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Move {
    pub id: i64,
    pub rule_id: Option<i64>,
    pub old_key: String,
    pub new_key: String,
    pub old_pos: usize,
    pub new_pos: usize,
}

/// What it takes to put `target` in `source`'s order: the rows to rewrite.
/// Empty when the month already reads that way.
pub fn moves(source: &[Line], target: &[Line]) -> Vec<Move> {
    let order = desired(source, target);
    let keys: Vec<&str> = order.iter().map(|&j| target[j].key.as_str()).collect();
    rekey(&keys)
        .into_iter()
        .enumerate()
        .filter_map(|(pos, new)| {
            let j = order[pos];
            new.map(|new_key| Move {
                id: target[j].id,
                rule_id: target[j].rule_id,
                old_key: target[j].key.clone(),
                new_key,
                old_pos: j + 1,
                new_pos: pos + 1,
            })
        })
        .collect()
}

/// A key for a new row that is the same line as `source[at]`, so it lands in
/// `target` where that row sat in `source` — right after the counterpart of
/// the nearest row above it that has one, or else right before the
/// counterpart of the nearest row below. `None` when no neighbor has a
/// counterpart: the caller's own rule decides (income on top, spend last).
///
/// `target` is in key order and does not hold the new row yet. Neighbors
/// sharing a key are stepped over, since there is nothing between equals.
pub fn placement(source: &[Line], at: usize, target: &[Line]) -> Option<String> {
    let pairs = counterparts(source, target);
    let mut of_source: Vec<Option<usize>> = vec![None; source.len()];
    for (j, i) in pairs.iter().enumerate() {
        if let Some(i) = *i {
            of_source[i] = Some(j);
        }
    }

    if let Some(j) = (0..at).rev().find_map(|i| of_source[i]) {
        let here = target[j].key.as_str();
        let next = target[j + 1..]
            .iter()
            .map(|t| t.key.as_str())
            .find(|k| *k > here);
        return Some(sortkey::between(Some(here), next));
    }
    if let Some(j) = (at + 1..source.len()).find_map(|i| of_source[i]) {
        let here = target[j].key.as_str();
        let prev = target[..j]
            .iter()
            .rev()
            .map(|t| t.key.as_str())
            .find(|k| *k < here);
        return Some(sortkey::between(prev, Some(here)));
    }
    None
}

/// Whether a month with no rows yet will arrive in `source`'s order. A new
/// occurrence takes its place from the latest month before it that has rows
/// (`latest`), so it follows when that month is the source or one the copy
/// puts in the source's order.
pub fn follows(latest: Option<Period>, source: Period, ordered: &[Period]) -> bool {
    latest.is_some_and(|p| p == source || ordered.contains(&p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: i64, rule: Option<i64>, title: &str, key: &str) -> Line {
        Line {
            id,
            rule_id: rule,
            title: title.into(),
            key: key.into(),
        }
    }

    /// Applies `moves` to `target` and reads the titles back in key order.
    fn after(target: &[Line], moves: &[Move]) -> Vec<String> {
        let mut rows: Vec<Line> = target.to_vec();
        for m in moves {
            let r = rows.iter_mut().find(|r| r.id == m.id).unwrap();
            assert_eq!(r.key, m.old_key);
            r.key = m.new_key.clone();
        }
        rows.sort_by(|a, b| a.key.cmp(&b.key));
        rows.into_iter().map(|r| r.title).collect()
    }

    #[test]
    fn the_rule_matches_before_the_title_does() {
        // October renamed the bill; the rule still knows it is the same line.
        let source = [
            line(1, Some(7), "Luz · Enel", "a"),
            line(2, None, "Luz · CPFL", "b"),
        ];
        let target = [
            line(11, None, "Luz · CPFL", "a"),
            line(12, Some(7), "Luz · CPFL", "b"),
        ];
        assert_eq!(counterparts(&source, &target), [Some(1), Some(0)]);
    }

    #[test]
    fn the_kth_row_of_a_title_matches_the_kth() {
        let source = [
            line(1, None, "Mercado", "a"),
            line(2, None, "Aluguel", "b"),
            line(3, None, "Mercado", "c"),
        ];
        let target = [
            line(11, None, "Mercado", "a"),
            line(12, None, "Mercado", "b"),
            line(13, None, "Mercado", "c"),
            line(14, None, "Aluguel", "d"),
        ];
        assert_eq!(
            counterparts(&source, &target),
            [Some(0), Some(2), None, Some(1)]
        );
        // The third Mercado has no counterpart, so it goes last.
        assert_eq!(desired(&source, &target), [0, 3, 1, 2]);
    }

    #[test]
    fn rows_with_no_counterpart_follow_the_matched_ones_in_their_own_order() {
        let source = [
            line(1, Some(1), "Salário", "a"),
            line(2, Some(2), "Luz", "b"),
            line(3, Some(3), "Aluguel", "c"),
        ];
        let target = [
            line(10, None, "Padaria", "a"),
            line(11, Some(3), "Aluguel", "b"),
            line(12, None, "Farmácia", "c"),
            line(13, Some(2), "Luz", "d"),
            line(14, Some(1), "Salário", "e"),
        ];
        let m = moves(&source, &target);
        assert_eq!(
            after(&target, &m),
            ["Salário", "Luz", "Aluguel", "Padaria", "Farmácia"]
        );
    }

    #[test]
    fn a_month_already_in_order_is_not_written() {
        let source = [line(1, Some(1), "A", "a"), line(2, Some(2), "B", "b")];
        let target = [line(11, Some(1), "A", "m"), line(12, Some(2), "B", "n")];
        assert!(moves(&source, &target).is_empty());
    }

    #[test]
    fn only_the_rows_out_of_place_get_a_new_key() {
        let source: Vec<Line> = (1..=6)
            .map(|i| line(i, Some(i), &format!("r{i}"), &format!("{i}")))
            .collect();
        // One row moved to the top by hand in the target: one write puts it back.
        let mut target: Vec<Line> = source
            .iter()
            .map(|s| line(s.id + 10, s.rule_id, &s.title, &s.key))
            .collect();
        target.rotate_right(1);
        target[0].key = "0V".into();
        let m = moves(&source, &target);
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m[0].id, 16);
        assert_eq!((m[0].old_pos, m[0].new_pos), (1, 6));
        assert_eq!(after(&target, &m), ["r1", "r2", "r3", "r4", "r5", "r6"]);
    }

    #[test]
    fn rows_sharing_a_key_still_come_out_strictly_ordered() {
        // A re-attribution can leave two live rows on one key; both cannot be
        // kept, or there would be nothing between them.
        let keys = ["b", "b", "b", "c"];
        let new = rekey(&keys);
        let fin: Vec<String> = keys
            .iter()
            .zip(&new)
            .map(|(k, n)| n.clone().unwrap_or_else(|| k.to_string()))
            .collect();
        assert!(fin.windows(2).all(|w| w[0] < w[1]), "{fin:?}");
        assert_eq!(new.iter().filter(|n| n.is_none()).count(), 2);
    }

    #[test]
    fn a_reversed_month_comes_out_in_the_source_order() {
        let source: Vec<Line> = (1..=8)
            .map(|i| line(i, Some(i), &format!("r{i}"), &format!("{i}")))
            .collect();
        let target: Vec<Line> = (1..=8)
            .rev()
            .zip(1..)
            .map(|(r, k)| line(r + 10, Some(r), &format!("r{r}"), &format!("{k}")))
            .collect();
        let m = moves(&source, &target);
        assert_eq!(m.len(), 7);
        assert_eq!(
            after(&target, &m),
            (1..=8).map(|i| format!("r{i}")).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_new_row_lands_after_its_predecessor_or_before_its_successor() {
        let source = [
            line(1, Some(1), "A", "a"),
            line(2, Some(2), "B", "b"),
            line(3, Some(3), "C", "c"),
        ];
        // B arriving, A and C already there: between them.
        let target = [line(11, Some(1), "A", "m"), line(13, Some(3), "C", "p")];
        let k = placement(&source, 1, &target).unwrap();
        assert!("m" < k.as_str() && k.as_str() < "p", "{k}");
        // A arriving with only C there: before it.
        let target = [line(13, Some(3), "C", "p")];
        assert!(placement(&source, 0, &target).unwrap().as_str() < "p");
        // Nothing to go by: the caller decides.
        let target = [line(20, None, "Padaria", "p")];
        assert_eq!(placement(&source, 1, &target), None);
    }

    #[test]
    fn a_new_row_steps_over_neighbors_that_share_a_key() {
        let source = [line(1, Some(1), "A", "a"), line(2, Some(2), "B", "b")];
        let target = [
            line(11, Some(1), "A", "m"),
            line(12, None, "Padaria", "m"),
            line(13, None, "Farmácia", "q"),
        ];
        let k = placement(&source, 1, &target).unwrap();
        assert!("m" < k.as_str() && k.as_str() < "q", "{k}");
    }

    #[test]
    fn every_arrival_order_rebuilds_the_previous_months_order() {
        // The materializer inserts one rule at a time, in whatever order it
        // walks them. Whatever that order, the month comes out as the last one.
        fn permutations(items: Vec<usize>) -> Vec<Vec<usize>> {
            if items.len() <= 1 {
                return vec![items];
            }
            let mut out = Vec::new();
            for i in 0..items.len() {
                let mut rest = items.clone();
                let x = rest.remove(i);
                for mut p in permutations(rest) {
                    p.insert(0, x);
                    out.push(p);
                }
            }
            out
        }
        for n in 1..=5usize {
            let source: Vec<Line> = (0..n)
                .map(|i| {
                    line(
                        i as i64,
                        Some(i as i64),
                        &format!("r{i}"),
                        &format!("{}", i + 1),
                    )
                })
                .collect();
            for arrival in permutations((0..n).collect()) {
                let mut target: Vec<Line> = Vec::new();
                for &at in &arrival {
                    let key = placement(&source, at, &target)
                        .unwrap_or_else(|| sortkey::between(None, None));
                    target.push(line(
                        100 + at as i64,
                        Some(at as i64),
                        &format!("r{at}"),
                        &key,
                    ));
                    target.sort_by(|a, b| a.key.cmp(&b.key));
                }
                let titles: Vec<&str> = target.iter().map(|t| t.title.as_str()).collect();
                let want: Vec<String> = (0..n).map(|i| format!("r{i}")).collect();
                assert_eq!(titles, want, "arrival {arrival:?}");
            }
        }
    }

    #[test]
    fn a_month_follows_when_the_one_it_inherits_from_was_ordered() {
        let p = |m| Period::new(2026, m).unwrap();
        assert!(follows(Some(p(10)), p(10), &[]));
        assert!(follows(Some(p(10)), p(7), &[p(10)]));
        assert!(!follows(Some(p(10)), p(7), &[p(8)]));
        assert!(!follows(None, p(7), &[p(8)]));
    }
}
