//! The selection's numbers, in the statusline — `x`, the way a spreadsheet
//! shows the sum of the cells you picked.
//!
//! **Counted within the focused pane.** `2 of 5 selected`, where 5 is the live
//! rows of the pane the cursor is in. A selection can span panes (it is a
//! marker for a batch operation), but the statusline speaks about the pane in
//! front of you, the same way its `4/7` position cell does.
//!
//! **Summed like the pane header**, because that is how this ledger already
//! writes money in and money out: only spends read `Σ R$ 587,30`, only
//! incomes `Σ + R$ 3.800,00`, and a mix gets the header's own in / out / net.
//! A projected income carries the `~`, as it does everywhere it reaches.
//!
//! **Computed and formatted here, never in Alpine.** The page only lists the
//! ids it already holds. The rows are read through the same
//! `ledger::pane_with_vault` call the header totals use, so privacy and the
//! numbers cannot come apart — an id the viewer may not see is simply not in
//! the pane — and money is formatted by the one helper the header uses.

use crate::db::ledger;
use crate::domain::money::{Cents, Currency};
use crate::domain::period::Period;
use crate::state::AppState;
use crate::ui::i18n::Catalog;
use crate::ui::row::{LineNumbers, RowCtx};
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use std::collections::HashSet;

/// What one pane row contributes: all `summarize` needs, and small enough to
/// build in a test.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub id: i64,
    pub income: bool,
    /// What the row is worth right now — the projection for an unconfirmed
    /// varying income (`LedgerRow::effective_cents`). `None` = no amount yet.
    pub cents: Option<Cents>,
    pub estimated: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub selected: usize,
    /// The pane's live rows — what "of N" counts.
    pub total: usize,
    pub ins: Cents,
    pub outs: Cents,
    /// How many selected rows fed each side, so a side is shown because it
    /// was selected, not because its sum happens to be non-zero.
    pub in_rows: usize,
    pub out_rows: usize,
    /// Any selected income is a projection.
    pub estimated: bool,
    /// Selected rows with no amount yet: counted, not summed, and said so.
    pub no_amount: usize,
}

/// Counts and sums the selected rows of one pane.
///
/// An id that is not one of `items` — a row in another pane, a deleted row,
/// a private row this viewer cannot see — is ignored: the pane is the truth
/// about what is in front of you.
pub fn summarize(items: &[Item], ids: &HashSet<i64>) -> Summary {
    let mut s = Summary {
        total: items.len(),
        ..Summary::default()
    };
    for it in items.iter().filter(|it| ids.contains(&it.id)) {
        s.selected += 1;
        let Some(c) = it.cents else {
            s.no_amount += 1;
            continue;
        };
        if it.income {
            s.ins += c;
            s.in_rows += 1;
            s.estimated |= it.estimated;
        } else {
            s.outs += c;
            s.out_rows += 1;
        }
    }
    s
}

/// One figure in the cell: an optional label (`in`, `out`, `net`), the money,
/// and the color class the header gives the same figure.
pub struct Part {
    label: String,
    value: String,
    class: &'static str,
}

#[derive(Template)]
#[template(path = "partials/selection.html")]
pub struct SelectionCell {
    /// Empty when nothing selected is in this pane: the cell then renders as
    /// an empty element, which the stylesheet hides.
    count: Option<String>,
    parts: Vec<Part>,
    no_amount: Option<String>,
}

impl SelectionCell {
    fn empty() -> SelectionCell {
        SelectionCell {
            count: None,
            parts: Vec::new(),
            no_amount: None,
        }
    }

    fn of(t: Catalog, s: Summary, currency: Currency) -> SelectionCell {
        if s.selected == 0 {
            return SelectionCell::empty();
        }
        let money = |c: Cents, est: bool| crate::web::ledger::money(c, currency, est);
        let parts = match (s.in_rows > 0, s.out_rows > 0) {
            (true, true) => {
                let net = s.ins - s.outs;
                vec![
                    Part {
                        label: t.totals_in().to_string(),
                        value: format!("+ {}", money(s.ins, s.estimated)),
                        class: "pos",
                    },
                    Part {
                        label: t.totals_out().to_string(),
                        value: money(s.outs, false),
                        class: "neg",
                    },
                    Part {
                        label: t.totals_net().to_string(),
                        value: format!(
                            "{}{}",
                            if net.is_negative() { "−" } else { "" },
                            money(net.abs(), s.estimated)
                        ),
                        class: if net.is_negative() { "neg" } else { "pos" },
                    },
                ]
            }
            (true, false) => vec![Part {
                label: "Σ".into(),
                value: format!("+ {}", money(s.ins, s.estimated)),
                class: "pos",
            }],
            (false, true) => vec![Part {
                label: "Σ".into(),
                value: money(s.outs, false),
                class: "neg",
            }],
            // Every selected row is still waiting for its amount.
            (false, false) => Vec::new(),
        };
        SelectionCell {
            count: Some(t.selection_count(s.selected as i64, &s.total.to_string())),
            no_amount: (s.no_amount > 0).then(|| t.selection_no_amount(s.no_amount as i64)),
            parts,
        }
    }
}

#[derive(Deserialize)]
pub struct Params {
    member: i64,
    period: String,
    /// Comma-separated row ids — the ones the page has marked with `x`.
    #[serde(default)]
    ids: String,
}

/// `GET /tx/selection?member=…&period=…&ids=…` — the statusline cell.
pub async fn cell(State(st): State<AppState>, auth: Auth, Query(p): Query<Params>) -> Response {
    let t = auth.catalog();
    let ids: HashSet<i64> = p
        .ids
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let (Some(period), false) = (Period::parse(&p.period), ids.is_empty()) else {
        return Html(SelectionCell::empty()).into_response();
    };
    let Some(member) = ledger::members(&st.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.id == p.member)
    else {
        return Html(SelectionCell::empty()).into_response();
    };

    let house = ledger::household(&st.pool).await.ok().flatten();
    let currency = house.as_ref().map(|h| h.currency).unwrap_or_default();
    let ctx = RowCtx {
        today: crate::clock::today(),
        sort: ledger::Sort::parse(&auth.settings.ledger_sort),
        currency,
        show_recurring: house.as_ref().is_none_or(|h| h.show_recurring),
        show_delta: auth.settings.ledger_show_delta,
        narrow: false,
        line_numbers: LineNumbers::parse(&auth.settings.line_numbers),
    };
    let Ok(pane) = ledger::pane_with_vault(
        &st.pool,
        &member,
        period,
        ctx,
        auth.viewer,
        Some(&st.cfg.receipts_root),
    )
    .await
    else {
        return Html(SelectionCell::empty()).into_response();
    };

    let items: Vec<Item> = pane
        .rows
        .iter()
        .map(|r| Item {
            id: r.id,
            income: r.kind.is_income(),
            cents: r.effective_cents(),
            estimated: r.is_estimated(),
        })
        .collect();
    Html(SelectionCell::of(t, summarize(&items, &ids), currency)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(id: i64, c: i64) -> Item {
        Item {
            id,
            income: false,
            cents: Some(Cents(c)),
            estimated: false,
        }
    }

    fn inc(id: i64, c: i64, estimated: bool) -> Item {
        Item {
            id,
            income: true,
            cents: Some(Cents(c)),
            estimated,
        }
    }

    fn ids(v: &[i64]) -> HashSet<i64> {
        v.iter().copied().collect()
    }

    fn pane() -> Vec<Item> {
        vec![
            inc(1, 380_000, true),
            out(2, 9_990),
            out(3, 8_740),
            out(4, 40_090),
            Item {
                id: 5,
                income: false,
                cents: None,
                estimated: false,
            },
        ]
    }

    #[test]
    fn spends_only_sum_one_side() {
        let s = summarize(&pane(), &ids(&[2, 3, 4]));
        assert_eq!((s.selected, s.total), (3, 5));
        assert_eq!(s.outs, Cents(58_820));
        assert_eq!((s.in_rows, s.out_rows), (0, 3));
        assert!(!s.estimated);
    }

    #[test]
    fn a_projected_income_marks_the_sum_estimated() {
        let s = summarize(&pane(), &ids(&[1]));
        assert_eq!(s.ins, Cents(380_000));
        assert!(s.estimated);
    }

    #[test]
    fn a_mix_keeps_both_sides() {
        let s = summarize(&pane(), &ids(&[1, 2]));
        assert_eq!((s.ins, s.outs), (Cents(380_000), Cents(9_990)));
        assert_eq!((s.in_rows, s.out_rows), (1, 1));
    }

    #[test]
    fn a_row_without_an_amount_is_counted_not_summed() {
        let s = summarize(&pane(), &ids(&[4, 5]));
        assert_eq!(s.selected, 2);
        assert_eq!(s.no_amount, 1);
        assert_eq!(s.outs, Cents(40_090));
    }

    /// An id from another pane, a deleted row or a private row the viewer
    /// cannot see is simply not among the pane's rows.
    #[test]
    fn ids_outside_the_pane_are_ignored() {
        let s = summarize(&pane(), &ids(&[2, 99, 100]));
        assert_eq!((s.selected, s.total), (1, 5));
        assert_eq!(s.outs, Cents(9_990));
    }
}
