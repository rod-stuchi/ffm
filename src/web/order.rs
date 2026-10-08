//! `:order` / `gs` — this pane's manual order, copied to other months.
//!
//! A bottom drawer, like `:trash`: one field, the months of the year on
//! screen to copy to, in the *Only some months* grammar (`1-9`, `^10`), and a
//! read-back the server renders on every keystroke saying what each month
//! would do. The matching and the writes are `db::order`; why one copy is
//! one `u` is there too.
//!
//! Refused under a sort that is not manual, and on somebody else's ledger —
//! where `J`/`K` refuse, because copying an order is reordering, many rows at
//! once. A grouped pane is allowed: see [`refused`].

use crate::db::changes::Actor;
use crate::db::{ledger, order};
use crate::domain::months::Months;
use crate::domain::period::Period;
use crate::state::AppState;
use crate::ui::i18n::Catalog;
use crate::web::months::{INITIALS, month_names, months_error};
use crate::web::session::Auth;
use crate::web::template::Html;
use crate::web::tx::{may_write, refuse, trigger};
use askama::Template;
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

/// What the drawer, its read-back and its submit all send: the pane — its
/// member and the month on screen — and the months expression.
#[derive(Deserialize)]
pub struct OrderParams {
    member: i64,
    period: String,
    #[serde(default)]
    months: String,
}

#[derive(Template)]
#[template(path = "partials/order_pane.html")]
pub struct OrderPane {
    t: Catalog,
    member: i64,
    /// The month copied from, `YYYY-MM`, carried back on every request.
    period: String,
    /// Whose pane, and which month, as people read them — the heading.
    who: String,
    month: String,
    year: String,
    readback: OrderReadback,
}

/// One month's mark: its initial, the state it would end in, and that state
/// in words for its tooltip.
pub struct MarkView {
    initial: &'static str,
    class: &'static str,
    title: String,
}

#[derive(Template)]
#[template(path = "partials/order_readback.html")]
pub struct OrderReadback {
    marks: Vec<MarkView>,
    line: String,
    bad: bool,
}

/// `GET /order` — the drawer.
pub async fn view(
    State(st): State<AppState>,
    auth: Auth,
    Query(p): Query<OrderParams>,
) -> Response {
    let t = auth.catalog();
    let Some(source) = Period::parse(&p.period) else {
        return refuse(t.error_not_written(), true);
    };
    if let Some(no) = refused(&st, &auth, p.member).await {
        return no;
    }
    let readback = match readback(&st, &t, p.member, source, "").await {
        Ok(r) => r,
        Err(no) => return no,
    };
    let who = ledger::members(&st.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.id == p.member)
        .map(|m| crate::ui::fmt::name(&m.name))
        .unwrap_or_default();
    Html(OrderPane {
        member: p.member,
        period: source.to_string(),
        who,
        month: crate::ui::fmt::period(source),
        year: source.year().to_string(),
        readback,
        t,
    })
    .into_response()
}

/// `GET /order/months` — the read-back, on every keystroke.
pub async fn preview(
    State(st): State<AppState>,
    auth: Auth,
    Query(p): Query<OrderParams>,
) -> Response {
    let t = auth.catalog();
    let Some(source) = Period::parse(&p.period) else {
        return refuse(t.error_not_written(), true);
    };
    if !auth.viewer.may_write(p.member) {
        return refuse("", false);
    }
    match readback(&st, &t, p.member, source, &p.months).await {
        Ok(r) => Html(r).into_response(),
        Err(no) => no,
    }
}

/// `POST /order/copy` — the copy. Closes the drawer and says what moved; one
/// `u` in this pane and month puts it all back.
pub async fn copy(State(st): State<AppState>, auth: Auth, Form(p): Form<OrderParams>) -> Response {
    let t = auth.catalog();
    let Some(source) = Period::parse(&p.period) else {
        return refuse(t.error_not_written(), true);
    };
    if let Some(no) = refused(&st, &auth, p.member).await {
        return no;
    }
    let targets = match targets(&t, source, &p.months) {
        Ok(Some(targets)) => targets,
        Ok(None) => return refuse(t.order_pick(), true),
        Err(bad) => return refuse(&bad, true),
    };
    // What the read-back promised about months that do not exist yet is
    // said again afterwards — read before the copy, which changes nothing
    // about which months have rows.
    let Ok(mut conn) = st.pool.acquire().await else {
        return refuse(t.error_not_written(), true);
    };
    let Ok((source_rows, plans)) = order::plan(&mut conn, p.member, source, &targets).await else {
        return refuse(t.error_not_written(), true);
    };
    if source_rows == 0 {
        return refuse(t.order_empty_source(), true);
    }
    let Ok(with_rows) = order::months_with_rows(&mut conn, p.member, source.year()).await else {
        return refuse(t.error_not_written(), true);
    };
    drop(conn);
    let ahead = ahead_line(&t, source, &plans, &with_rows);

    let Ok(done) = order::copy(
        &st.pool,
        Actor::User(auth.viewer.member_id),
        p.member,
        source,
        &targets,
    )
    .await
    else {
        return refuse(t.error_not_written(), true);
    };

    let mut parts = Vec::new();
    if done.anchor.is_some() {
        let months: Vec<u32> = done.months.iter().map(|(p, _)| p.month()).collect();
        parts.push(t.order_copied(&month_names(&t, &months).join(", "), done.rows() as i64));
    } else {
        parts.push(t.order_nothing_moved().to_string());
    }
    parts.extend(ahead);
    if done.anchor.is_some() {
        parts.push(t.redo_undo_hint().to_string());
    }

    // Nothing on screen moved — the copy was made FROM this month — so the
    // answer is the statusline, and the drawer closes: the job is done. The
    // months it reached hear it over the live feed.
    let mut res = (StatusCode::OK, HeaderMap::new(), String::new()).into_response();
    res.headers_mut()
        .insert("HX-Reswap", HeaderValue::from_static("none"));
    trigger(
        res,
        serde_json::json!({
            "ffm:say": { "message": parts.join(" · "), "bad": false },
            "ffm:split-close": {},
        }),
    )
}

/// Why this pane's order cannot be copied from here, if it cannot: somebody
/// else's ledger, or a sort that is not the manual order — in the words `J`
/// and `K` use.
///
/// **A grouped pane is allowed**, where `J`/`K` refuse. A move by one place
/// means nothing when the rows on either side on screen are not the ones on
/// either side in the manual order; a copy has no such question — it copies
/// the manual order whole, and the folds are derived from it (first seen,
/// and manual inside each), so the months it reaches read the same grouped
/// as this one does.
async fn refused(st: &AppState, auth: &Auth, member: i64) -> Option<Response> {
    if let Err(no) = may_write(st, auth, member).await {
        return Some(no);
    }
    let t = auth.catalog();
    let sort = ledger::Sort::parse(&auth.settings.ledger_sort);
    if sort.is_derived() {
        return Some(refuse(&t.sort_refuses_reorder(sort.code()), true));
    }
    None
}

/// The months of the source's year an expression picks, the source itself
/// left out. `Ok(None)` for an empty field: unlike a rule's, where empty
/// means every month, here it would be a bulk write nobody typed.
fn targets(t: &Catalog, source: Period, expr: &str) -> Result<Option<Vec<Period>>, String> {
    if expr.trim().is_empty() {
        return Ok(None);
    }
    let months = Months::parse(expr).map_err(|e| months_error(t, &e))?;
    Ok(Some(
        months
            .as_slice()
            .iter()
            .filter_map(|m| Period::new(source.year(), *m))
            .filter(|p| *p != source)
            .collect(),
    ))
}

/// The read-back for `expr`: one mark per month of the year, and a line.
async fn readback(
    st: &AppState,
    t: &Catalog,
    member: i64,
    source: Period,
    expr: &str,
) -> Result<OrderReadback, Response> {
    let year = source.year();
    let only_source = |line: String, bad: bool| OrderReadback {
        marks: (1..=12)
            .map(|m| MarkView {
                initial: INITIALS[(m - 1) as usize],
                class: if m == source.month() { "is-src" } else { "" },
                title: if m == source.month() {
                    t.order_mark_source().to_string()
                } else {
                    t.month_short(m as i64).to_string()
                },
            })
            .collect(),
        line,
        bad,
    };
    let targets = match targets(t, source, expr) {
        Ok(Some(targets)) => targets,
        Ok(None) => return Ok(only_source(t.order_pick().to_string(), false)),
        Err(bad) => return Ok(only_source(bad, true)),
    };

    let fail = || refuse(t.error_not_written(), true);
    let mut conn = st.pool.acquire().await.map_err(|_| fail())?;
    let (source_rows, plans) = order::plan(&mut conn, member, source, &targets)
        .await
        .map_err(|_| fail())?;
    if source_rows == 0 {
        return Ok(only_source(t.order_empty_source().to_string(), true));
    }
    let with_rows = order::months_with_rows(&mut conn, member, year)
        .await
        .map_err(|_| fail())?;

    let today = Period::of(crate::clock::today());
    let ordered: Vec<Period> = plans
        .iter()
        .filter(|p| p.rows > 0)
        .map(|p| p.period)
        .collect();
    let mut marks = Vec::with_capacity(12);
    for m in 1..=12u32 {
        let initial = INITIALS[(m - 1) as usize];
        let name = t.month_short(m as i64).to_string();
        let Some(p) = Period::new(year, m) else {
            continue;
        };
        let (class, title) = if p == source {
            ("is-src", t.order_mark_source().to_string())
        } else if let Some(plan) = plans.iter().find(|x| x.period == p) {
            if plan.rows > 0 && !plan.moves.is_empty() {
                (
                    "is-on",
                    format!("{name} · {}", t.order_mark_moves(plan.moves.len() as i64)),
                )
            } else if plan.rows > 0 {
                ("is-same", format!("{name} · {}", t.order_mark_same()))
            } else if p > today {
                let latest = latest_before(&with_rows, p);
                if crate::domain::order::follows(latest, source, &ordered) {
                    ("is-follow", format!("{name} · {}", t.order_mark_follows()))
                } else {
                    let from = latest.map(crate::ui::fmt::period).unwrap_or_default();
                    (
                        "is-nofollow",
                        format!("{name} · {}", t.order_mark_not(&from)),
                    )
                }
            } else {
                ("is-empty", format!("{name} · {}", t.order_mark_empty()))
            }
        } else {
            ("", name)
        };
        marks.push(MarkView {
            initial,
            class,
            title,
        });
    }

    let moving: Vec<&order::MonthPlan> = plans.iter().filter(|p| !p.moves.is_empty()).collect();
    let same: Vec<u32> = plans
        .iter()
        .filter(|p| p.rows > 0 && p.moves.is_empty())
        .map(|p| p.period.month())
        .collect();
    let empty: Vec<u32> = plans
        .iter()
        .filter(|p| p.rows == 0 && p.period <= today)
        .map(|p| p.period.month())
        .collect();

    let mut parts = Vec::new();
    if moving.is_empty() {
        parts.push(t.order_nothing_to_move().to_string());
    } else {
        let rows: usize = moving.iter().map(|p| p.moves.len()).sum();
        let months: Vec<u32> = moving.iter().map(|p| p.period.month()).collect();
        parts.push(t.order_line_moves(rows as i64, &month_names(t, &months).join(", ")));
    }
    if !same.is_empty() {
        parts.push(t.order_line_same(&month_names(t, &same).join(", ")));
    }
    parts.extend(ahead_line(t, source, &plans, &with_rows));
    if !empty.is_empty() {
        parts.push(t.order_line_empty(&month_names(t, &empty).join(", ")));
    }
    Ok(OrderReadback {
        marks,
        line: parts.join(" · "),
        bad: false,
    })
}

/// The latest month before `p` that has rows — what `p` will inherit from.
fn latest_before(with_rows: &[Period], p: Period) -> Option<Period> {
    with_rows.iter().copied().filter(|m| *m < p).max()
}

/// What happens to the picked months that have no rows yet and are still to
/// come: they arrive in the order of the month before them, which is this
/// one's only if that month is this one or a month the copy puts in order.
fn ahead_line(
    t: &Catalog,
    source: Period,
    plans: &[order::MonthPlan],
    with_rows: &[Period],
) -> Vec<String> {
    let today = Period::of(crate::clock::today());
    let ordered: Vec<Period> = plans
        .iter()
        .filter(|p| p.rows > 0)
        .map(|p| p.period)
        .collect();
    let mut follow = Vec::new();
    let mut not: Vec<(u32, Option<Period>)> = Vec::new();
    for p in plans.iter().filter(|p| p.rows == 0 && p.period > today) {
        let latest = latest_before(with_rows, p.period);
        if crate::domain::order::follows(latest, source, &ordered) {
            follow.push(p.period.month());
        } else {
            not.push((p.period.month(), latest));
        }
    }
    let mut out = Vec::new();
    if !follow.is_empty() {
        out.push(t.order_line_follows(&month_names(t, &follow).join(", ")));
    }
    if !not.is_empty() {
        let months: Vec<u32> = not.iter().map(|(m, _)| *m).collect();
        let from = not
            .iter()
            .find_map(|(_, l)| *l)
            .map(crate::ui::fmt::period)
            .unwrap_or_default();
        out.push(t.order_line_not(&month_names(t, &months).join(", "), &from));
    }
    out
}
