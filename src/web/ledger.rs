//! The ledger, read-only.
//!
//! A month is a *view*: one continuous transactions table, sliced by `period`.
//! There is no "copy last month" operation and never will be, so navigating
//! months is a different `WHERE`, not a different set of rows.
//!
//! This is deliberately not the full app shell yet — no command bar, no month
//! picker, no keymap. It renders real panes from real rows, which is the whole
//! point of the phase: the row component stopped being a fixture.

use crate::db::ledger::{self, Pane};
use crate::domain::period::Period;
use crate::state::AppState;
use crate::ui::fmt;
use crate::ui::i18n::Catalog;
use crate::ui::icons;
use crate::ui::row::{FlagView, LineNumbers, MarkView, RowCtx, RowView};
use crate::ui::theme;
use crate::web::session::Auth;
use crate::web::template::Html;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use chrono::NaiveDate;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Params {
    /// `YYYY-MM`. Absent means the current month — always, on every first load.
    /// Never the last-viewed month and never January.
    period: Option<String>,
    /// The focused member, by id. Absent means **your own** pane: landing on
    /// somebody else's ledger is the wrong first thing to see.
    ///
    /// It lives in the URL rather than the session on purpose. The focused
    /// member decides who a new transaction is attributed to, and a
    /// server-side idea of "current member" drifts the moment the app is open
    /// in two windows — whereas a URL is per-window by construction, and
    /// survives a refresh for free.
    pane: Option<i64>,
}

/// A pane's three numbers, and the household's four, live in their own structs
/// because a mutation has to send them back too: a row that lands without its
/// totals moving is a ledger that disagrees with itself on screen.
/// One cell in the month picker.
pub struct MonthCell {
    pub key: String,
    pub label: String,
    /// The month on screen — the accent outline, "you are here".
    pub current: bool,
    /// Today's month — the `--info` bar. A different fact from `current`: you
    /// can be looking at March in September.
    pub now: bool,
    /// Past today's month. Nothing is materialized there, so the cell says
    /// `—` rather than a NET of zero that nobody earned.
    pub future: bool,
    /// The month's NET, compact and signed. `None` on the page render — the
    /// numbers arrive when the picker opens (see [`month_pop`]).
    pub net: Option<String>,
    pub negative: bool,
    /// How many unresolved occurrences sit in this month. Zero means no dot —
    /// ambient like every other status in this app, rather than a toast that
    /// gets dismissed and then forgotten.
    pub unresolved: usize,
    pub title: String,
}

/// The picker's foot: the year so far, in facts. Absent for a year that has
/// not started, which has nothing to sum.
pub struct YearSummary {
    pub heading: String,
    pub ins: String,
    pub outs: String,
    pub net: String,
    pub negative: bool,
    pub avg: String,
    /// "2 months need a receipt" — what the dots mean, said once. Empty when
    /// nothing is flagged.
    pub unresolved: String,
}

/// The popup's body: the page renders it with labels and dots only, and
/// `GET /months` renders it again with the numbers. One partial, so the two
/// cannot come apart.
#[derive(Template)]
#[template(path = "partials/month_pop.html")]
pub struct MonthPop {
    pub t: Catalog,
    pub year: i32,
    pub months: Vec<MonthCell>,
    pub summary: Option<YearSummary>,
}

pub struct PaneTotals {
    pub id: i64,
    pub ins: String,
    pub outs: String,
    pub balance: String,
    pub negative: bool,
}

/// Money, with the tilde when any part of it is a projection.
///
/// The mark carries whether the money moved; the tilde carries whether the
/// number is a fact. One channel per fact, as everywhere else — and the two
/// are asked separately, so a confirmed income in an estimated pane still
/// reads as the fact it is.
pub(crate) fn money(
    amount: crate::domain::money::Cents,
    currency: crate::domain::money::Currency,
    estimated: bool,
) -> String {
    if estimated {
        fmt::money_estimated(amount, currency)
    } else {
        fmt::money(amount, currency)
    }
}

pub struct HouseTotals {
    pub ins: String,
    pub outs: String,
    pub net: String,
    pub negative: bool,
}

/// Every pane's totals for one period, plus the household's — read through the
/// same pane query a page load uses, so a fragment's numbers are computed by
/// exactly the same path as the page's.
pub async fn totals(st: &AppState, auth: &Auth, period: Period) -> (Vec<PaneTotals>, HouseTotals) {
    let ctx = session_ctx(st, auth).await;
    let currency = ctx.currency;

    let mut panes = Vec::new();
    let mut ins = crate::domain::money::Cents::ZERO;
    let mut outs = crate::domain::money::Cents::ZERO;
    let mut any_estimated = false;

    for pane in member_panes(st, auth, period, ctx).await {
        ins += pane.ins;
        outs += pane.outs;
        any_estimated |= pane.estimated;
        let balance = pane.balance();
        panes.push(PaneTotals {
            id: pane.member.id,
            ins: money(pane.ins, currency, pane.estimated),
            // Only income varies, so OUTs are never a projection.
            outs: fmt::money(pane.outs, currency),
            balance: money(balance.abs(), currency, pane.estimated),
            negative: balance.is_negative(),
        });
    }

    let net = ins - outs;
    (
        panes,
        HouseTotals {
            ins: money(ins, currency, any_estimated),
            outs: fmt::money(outs, currency),
            net: money(net.abs(), currency, any_estimated),
            negative: net.is_negative(),
        },
    )
}

/// The render context a total needs, from the session's settings and the
/// household row. Sort, gutter and delta do not move a sum, but the pane query
/// takes them all, and building it in one place keeps two callers from
/// disagreeing about the currency.
async fn session_ctx(st: &AppState, auth: &Auth) -> RowCtx {
    let house = ledger::household(&st.pool).await.ok().flatten();
    RowCtx {
        today: crate::clock::today(),
        sort: ledger::Sort::parse(&auth.settings.ledger_sort),
        currency: house.as_ref().map(|h| h.currency).unwrap_or_default(),
        show_recurring: house.as_ref().is_none_or(|h| h.show_recurring),
        show_delta: auth.settings.ledger_show_delta,
        narrow: false,
        line_numbers: LineNumbers::parse(&auth.settings.line_numbers),
    }
}

/// Every member's pane for one period, as this viewer sees it — the one loop
/// the header totals and the month picker both sum from. Privacy is applied
/// inside the pane query, so neither can count a row the ledger hides.
async fn member_panes(st: &AppState, auth: &Auth, period: Period, ctx: RowCtx) -> Vec<Pane> {
    let mut out = Vec::new();
    for m in ledger::members_in(&st.pool, period)
        .await
        .unwrap_or_default()
    {
        if let Ok(pane) = ledger::pane_with_vault(
            &st.pool,
            &m,
            period,
            ctx,
            auth.viewer,
            Some(&st.cfg.receipts_root),
        )
        .await
        {
            out.push(pane);
        }
    }
    out
}

/// One `bucket` fold: a framed container with its name, a row count and a
/// subtotal that stays visible whether the fold is open or closed.
///
/// **That subtotal is why there is no separate report screen.** `zM` closes
/// every fold and the ledger already reads as a per-category summary —
/// browsing and summarizing are one mechanism at two zoom levels.
pub struct Group {
    pub name: String,
    /// Already pluralized — the count is a number the catalog has to agree
    /// with, so it is rendered where the catalog lives rather than cast in a
    /// template.
    pub count: String,
    pub subtotal: String,
    pub negative: bool,
    pub rows: Vec<RowView>,
}

pub struct PaneView {
    /// Sent with every add, because which pane is focused is data — a
    /// server-side idea of "current member" drifts the moment the app is open
    /// in two windows.
    pub id: i64,
    pub name: String,
    pub rows: Vec<RowView>,
    /// Empty unless the member grouped the pane. Grouping never silently drops
    /// a row: anything with no bucket lands in `Uncategorized`.
    pub groups: Vec<Group>,
    pub grouped: bool,
    pub totals: PaneTotals,
    /// Whether the session may change this pane's rows — its own, or any when
    /// admin powers are in effect. A non-admin reads the other ledgers and
    /// writes only their own; the server refuses regardless, and this is what
    /// lets the page say so before a key is pressed.
    pub writable: bool,
}

/// One light/dark choice in the profile menu: the value `/theme` takes, its
/// word and icon, and whether it is the state this device is in.
struct ModeChoice {
    code: &'static str,
    label: &'static str,
    svg: &'static str,
    on: bool,
}

#[derive(Template)]
#[template(path = "pages/ledger.html")]
struct LedgerPage {
    theme_css: String,
    attr: Option<&'static str>,
    me: String,
    admin: bool,
    previewing: bool,
    t: Catalog,
    household: String,
    /// A dev build's badge and tab prefix — `None` on PROD (`crate::run`).
    run: Option<crate::run::Badge>,
    period: String,
    prev: String,
    next: String,
    panes: Vec<PaneView>,
    /// The light/dark control: the word for the state you are in, the mode its
    /// form submits, and the icon that names it. The cookie is `HttpOnly`, so
    /// this is a plain form POST to `/theme` — JS could not set it even if we
    /// wanted it to.
    mode_label: &'static str,
    mode_next: &'static str,
    mode_svg: &'static str,
    /// The same control as three direct choices, for the profile menu on a
    /// phone, where the bar has no room for the button above. Three choices
    /// rather than the button's cycle: through a menu, a cycle is up to three
    /// rounds of open-and-tap to land on the one you wanted.
    modes: Vec<ModeChoice>,
    /// Where `/theme` sends you back to. Rebuilt rather than read off the
    /// request, so it is a local path by construction; the form restamps it
    /// from `location` on submit, because `?pane=` is rewritten client-side.
    back_to: String,
    person_svg: &'static str,
    /// Which pane the cursor starts in, as an index into `panes` — the
    /// component counts panes, not member ids.
    focused: usize,
    /// `YYYY-MM`, for the forms. The label above is `fmt::period`.
    period_key: String,
    /// The picker's body, rendered by the same partial `GET /months` answers
    /// with — here without the numbers, which are fetched when it opens.
    picker: MonthPop,
    /// The dot on the trigger: any month **other than this one** with
    /// something overdue and unreceipted.
    unresolved_total: usize,
    unresolved_title: String,
    /// The member's layout, finally applied. `auto` resolves in CSS from
    /// `min_split`, rendered into one media query — never on the server and
    /// never in Alpine, either of which would be wrong on first paint and on
    /// every resize.
    layout_panes: String,
    /// `relative` | `absolute` | `none`. The shell needs it because relative
    /// numbers are measured from the cursor, and the cursor is Alpine's.
    nu: &'static str,
    cursor_on_fold: String,
    /// Shown in the statusline's two clickable cells, and the initial text
    /// before Alpine takes over — so the bar is not blank for a frame.
    nu_label: &'static str,
    sort: &'static str,
    sort_label: String,
    /// `S`'s group order, `None` while the pane is flat — the statusline cell
    /// is shown only when there are groups to order, and `S` refuses without.
    group_order: Option<&'static str>,
    group_order_label: String,
    group_order_needs_group: String,
    /// The signed-in member, for the statusline's own cell. The SESSION's
    /// member, never the focused pane's — focusing somebody else's pane does
    /// not make you them.
    me_name: String,
    /// The running build — `v0.0.3`, `v0.0.4-dev` on a dev build — at the
    /// statusline's far right. Not a PROD label: every build shows it.
    version: &'static str,
    me_admin: bool,
    /// The `:` bar's hint — **exactly** what `runCommand` handles, built here
    /// rather than written out in the template so the two cannot drift. The
    /// first version was copied from the mock and advertised `add`, `in`,
    /// `move`, `undo` and `help`, none of which exist: a hint that lists
    /// commands the app refuses is worse than no hint.
    ///
    /// Admin-only commands are absent for a non-admin, and for an admin
    /// previewing as one — the same rule that removes the Household button
    /// rather than disabling it.
    command_hint: String,
    /// The same table, for the bar's Tab completion and contextual help.
    commands: Vec<super::commands::CommandView>,
    /// The SESSION's member id — the `:` history is kept per member, so two
    /// people on one laptop do not recall each other's commands.
    me_id: i64,
    /// The household's number format, for the amount mask — `{decimals,
    /// decimal, group}` as JSON. From `household_settings.locale_currency`,
    /// the one value every money display in this app reads, so a masked field
    /// cannot disagree with the rows above it.
    money_format: String,
    min_split: i64,
    min_width: i64,
    max_width: i64,
    gap: i64,
    /// `E` on a row with no rule behind it says so in the statusline rather
    /// than doing nothing at all.
    not_a_rule: String,
    remove_file_title: String,
    rule_deleted: String,
    paid_refused_tracked: String,
    paid_refused_one_off: String,
    error_not_written: String,
    unsaved_tag: String,
    unsaved_rule_tag: String,
    drafts_parked: String,
    drafts_parked_one: String,
    /// Changing month is a page load, and a page load loses parked drafts —
    /// so with any parked, the first `[`/`]`/`T` (or click) asks. `{d}` is
    /// the drafts count, already pluralized by the pane; `{k}` the key.
    month_leave_key: String,
    month_leave_click: String,
    month_already_current: String,
    /// Today's month, from `clock::today()` — never the browser's clock, so a
    /// dev server running as of another date (`FFM_TODAY`) is honored by `T`.
    today_key: String,
    /// `default` | `comfortable` | `compact` — the session member's, written
    /// onto the shell; `row.css` reads it. Server-rendered so the first frame
    /// is already right, which is why writing it refreshes the page.
    density: String,
    /// `png` | `pdf` — which side `P` opens a PDF on (`v` flips it there).
    pdf_view: String,
    give_nobody: String,
    give_moved_key: String,
    details_none: String,
    no_files: String,
    months_word: String,
    /// The `?` modal, built from the same table `design/spec.md` documents —
    /// so a key that exists and a key that is written down stay the same set.
    keymap: Vec<(String, Vec<(String, String)>)>,
    /// What every mark means, drawn with the row's own mark partial.
    legend: Vec<LegendItem>,
    /// The three "there is no such file" messages — `g1`/`g2`/`g3` on a row
    /// that has not got one. Three fields rather than a JSON blob, so each is
    /// escaped by the same filter as every other string handed to the page.
    /// `not a command: {cmd}` — with the placeholder left in, so the pane can
    /// fill it without the catalog having to know about JavaScript.
    not_a_command: String,
    file_not_tracked: String,
    reattribute_admin_only: String,
    no_bill: String,
    no_scheduled: String,
    no_receipt: String,
    house: HouseTotals,
    empty: bool,
}

pub async fn ledger(
    State(st): State<AppState>,
    auth: Auth,
    Query(p): Query<Params>,
) -> impl IntoResponse {
    let today = crate::clock::today();
    let period = p
        .period
        .as_deref()
        .and_then(Period::parse)
        .unwrap_or_else(|| Period::of(today));

    let house = ledger::household(&st.pool).await.unwrap_or(None);
    let (name, currency, show_recurring) = match &house {
        Some(h) => (h.name.clone(), h.currency, h.show_recurring),
        None => ("FFM".to_string(), Default::default(), true),
    };
    // Language is the SESSION member's, full stop — the pre-login cookie is
    // never read on an authenticated request. Currency is the household's,
    // because every member's rows feed the same totals. Two axes, two sources.
    let lang = auth.settings.language;

    let ctx = RowCtx {
        today,
        // The member's own choice — it lives in their settings row, which is
        // the same row `:sort date` and the :profile radio write.
        sort: ledger::Sort::parse(&auth.settings.ledger_sort),
        currency,
        show_recurring,
        show_delta: auth.settings.ledger_show_delta,
        narrow: false,
        line_numbers: LineNumbers::parse(&auth.settings.line_numbers),
    };
    let t = Catalog::new(lang);

    let members = ledger::members_in(&st.pool, period)
        .await
        .unwrap_or_default();
    let mut panes = Vec::new();
    let mut ins = crate::domain::money::Cents::ZERO;
    let mut outs = crate::domain::money::Cents::ZERO;
    let mut any_estimated = false;
    // Grouping is a derived view exactly like sort: the moment rows are
    // re-clustered by category, J/K refuse for the same reason.
    let group = GroupOrder::of(&auth.settings);

    for m in &members {
        // Privacy is applied inside the query, so the rows and the totals
        // below cannot disagree about what this viewer can see.
        let Ok(pane) = ledger::pane_with_vault(
            &st.pool,
            m,
            period,
            ctx,
            auth.viewer,
            Some(&st.cfg.receipts_root),
        )
        .await
        else {
            continue;
        };
        ins += pane.ins;
        outs += pane.outs;
        any_estimated |= pane.estimated;
        let writable = auth.viewer.may_write(pane.member.id);
        panes.push(view(pane, &t, currency, group, writable));
    }

    let unresolved =
        ledger::unresolved_months(&st.pool, auth.viewer, today, Some(&st.cfg.receipts_root))
            .await
            .unwrap_or_default();

    // Which pane to land in. `?pane=` wins, so a refresh keeps you where you
    // were; with nothing asked for it is your OWN pane, because the first
    // thing a member should see is their own ledger and not somebody else's.
    // An id naming nobody falls back the same way rather than erroring — a
    // stale bookmark is not worth a failure page.
    let focused = p
        .pane
        .and_then(|id| panes.iter().position(|v| v.id == id))
        .or_else(|| panes.iter().position(|v| v.id == auth.viewer.member_id))
        .unwrap_or(0);

    let net = ins - outs;
    let back_to = match panes.get(focused) {
        Some(v) => format!("/?period={}&pane={}", period, v.id),
        None => format!("/?period={period}"),
    };

    Html(LedgerPage {
        mode_label: crate::web::auth::mode_label(&t, auth.mode),
        mode_next: auth.mode.next(),
        mode_svg: crate::ui::icons::svg(auth.mode.icon()),
        modes: [
            theme::ModePref::Light,
            theme::ModePref::Dark,
            theme::ModePref::System,
        ]
        .into_iter()
        .map(|m| ModeChoice {
            code: m.code(),
            label: crate::web::auth::mode_label(&t, m),
            svg: icons::svg(m.icon()),
            on: m == auth.mode,
        })
        .collect(),
        person_svg: crate::ui::icons::svg("person"),
        back_to,
        focused,
        // The member's palette, the device's light/dark — two facts, two homes.
        theme_css: theme::render(
            auth.settings.theme,
            auth.mode,
            &auth.settings.theme_overrides,
        )
        .to_css(),
        attr: auth.mode.attr(),
        me: fmt::name(&auth.session.member.name),
        admin: auth.viewer.is_admin(),
        previewing: auth.session.preview,
        household: name,
        run: crate::run::badge(&t),
        period: fmt::period(period),
        period_key: period.to_string(),
        picker: MonthPop {
            t: Catalog::new(lang),
            year: period.year(),
            months: month_cells(&t, &unresolved, period, today),
            summary: None,
        },
        unresolved_total: unresolved.iter().map(|u| u.count).sum(),
        unresolved_title: t
            .unresolved_dot(unresolved.iter().map(|u| u.count).sum::<usize>() as i64),
        layout_panes: auth.settings.layout_panes.clone(),
        nu: LineNumbers::parse(&auth.settings.line_numbers).code(),
        cursor_on_fold: t.cursor_on_fold().to_string(),
        nu_label: match LineNumbers::parse(&auth.settings.line_numbers) {
            LineNumbers::Relative => "rnu",
            LineNumbers::Absolute => "nu",
            LineNumbers::Off => "nonu",
        },
        sort: ledger::Sort::parse(&auth.settings.ledger_sort).code(),
        sort_label: ledger::Sort::parse(&auth.settings.ledger_sort)
            .code()
            .to_string(),
        group_order: GroupOrder::of(&auth.settings).map(GroupOrder::code),
        group_order_label: GroupOrder::of(&auth.settings)
            .map(|g| g.label(&t))
            .unwrap_or_default(),
        group_order_needs_group: t.group_order_needs_group().to_string(),
        me_name: fmt::name(&auth.session.member.name),
        version: crate::version(),
        // Whether admin powers are actually IN EFFECT, not what the row says
        // — an admin previewing as a member should read as a member here, or
        // the badge would contradict what the rest of the app is doing.
        me_admin: auth.viewer.is_admin(),
        command_hint: super::commands::hint(auth.viewer.is_admin(), auth.session.role_admin),
        commands: super::commands::views(&t, auth.viewer.is_admin(), auth.session.role_admin),
        me_id: auth.session.member.id,
        money_format: money_format(currency),
        min_split: auth.settings.layout_min_split_width,
        min_width: auth.settings.layout_min_width,
        max_width: auth.settings.layout_max_width,
        gap: auth.settings.layout_gap,
        not_a_rule: t.rule_none_on_row().to_string(),
        remove_file_title: t.carousel_remove_title("{file}"),
        rule_deleted: t.rule_deleted_on_row().to_string(),
        paid_refused_tracked: t.paid_refused_tracked().to_string(),
        paid_refused_one_off: t.paid_refused_one_off().to_string(),
        error_not_written: t.error_not_written().to_string(),
        unsaved_tag: t.row_unsaved().to_string(),
        unsaved_rule_tag: t.row_unsaved_rule().to_string(),
        // The count is filled in by the pane, so the catalog keeps the plural
        // and the template keeps the number. Both forms: built from 2 alone,
        // one draft read "1 unsaved drafts".
        drafts_parked: t.drafts_parked(2).replace('2', "{n}"),
        drafts_parked_one: t.drafts_parked(1),
        month_leave_key: t.month_leave_key("{d}", "{k}"),
        month_leave_click: t.month_leave_click("{d}"),
        month_already_current: t.month_already_current().to_string(),
        today_key: Period::of(today).to_string(),
        density: auth.settings.density.clone(),
        pdf_view: auth.settings.pdf_view.clone(),
        give_nobody: t.give_nobody().to_string(),
        give_moved_key: t.give_moved_key().to_string(),
        details_none: t.details_none().to_string(),
        no_files: t.file_no_files().to_string(),
        months_word: t.details_months().to_string(),
        keymap: keymap(&t, auth.viewer.is_admin(), auth.session.role_admin),
        legend: legend(&t, currency, show_recurring),
        not_a_command: t.not_a_command("{cmd}"),
        file_not_tracked: t.file_not_tracked().to_string(),
        reattribute_admin_only: t.reattribute_admin_only().to_string(),
        no_bill: t.file_none_bill().to_string(),
        no_scheduled: t.file_none_scheduled().to_string(),
        no_receipt: t.file_none_receipt().to_string(),
        prev: period.pred().to_string(),
        next: period.succ().to_string(),
        empty: panes.is_empty(),
        panes,
        house: HouseTotals {
            ins: money(ins, currency, any_estimated),
            outs: fmt::money(outs, currency),
            net: money(net.abs(), currency, any_estimated),
            negative: net.is_negative(),
        },
        t,
    })
}

/// The number format the amount mask types into, as JSON.
///
/// Only the three facts a mask needs — how many minor digits, and the two
/// separators. Not the symbol: the mask writes a bare number, and the symbol
/// is the row's job. Built here rather than written into the template so it
/// comes from `Currency::format` like every other money surface.
fn money_format(currency: crate::domain::money::Currency) -> String {
    let f = currency.format();
    format!(
        r#"{{"decimals":{},"decimal":"{}","group":"{}"}}"#,
        f.decimals, f.decimal_sep, f.group_sep
    )
}

/// The keymap, as `?` shows it.
///
/// Lowercase acts on the **occurrence**, uppercase on the **rule or the
/// window** — which is the rule the whole table follows, and the reason it can
/// be read rather than memorized.
fn keymap(t: &Catalog, admin: bool, holds_role: bool) -> Vec<(String, Vec<(String, String)>)> {
    let k = |key: &str, what: &str| (key.to_string(), what.to_string());
    vec![
        (
            t.keymap_move().to_string(),
            vec![
                k("j / k", "row down / up"),
                k("5j 7k", "counted motion"),
                k("gg / G", "first / last row"),
                k("H / L", "focus pane, left / right"),
                k("gt / gT", "next / previous member tab"),
                k("2gt", "jump straight to tab 2"),
                k("[ / ]", "previous / next month"),
                k("3]", "three months on — 3[ three back"),
                k("T", "this month — today's"),
            ],
        ),
        (
            t.keymap_act().to_string(),
            vec![
                k("a / i", "add a one-off spend / income"),
                k("A / I", "create a recurring bill / income"),
                k("e / E", "edit this row / its rule"),
                k("J / K", "move the row down / up"),
                k("s", "row order: manual · date · amount"),
                k(
                    "ga",
                    "give the row to another member — picked by name, admin only",
                ),
                k("x", "select, for a batch operation"),
                k("p", "toggle private"),
                k("m", "mark paid by hand — untracked recurring bills"),
                k("gd", "due, cadence and receipt details"),
                k("gl", "this row's history"),
            ],
        ),
        (
            t.keymap_find().to_string(),
            vec![
                k("/", "search this month — incremental, regex"),
                k("n / N", "next / previous match"),
                k(":grep", "search every month"),
            ],
        ),
        (
            t.keymap_fold().to_string(),
            vec![
                k("za", "toggle the fold under the cursor"),
                k("zo / zc", "open / close it"),
                k("zR / zM", "open / close every fold"),
                k("S", "order the groups: first seen · A–Z · by cost"),
            ],
        ),
        (
            t.keymap_files().to_string(),
            vec![
                k("g1 / g2 / g3", "open the bill / scheduled / receipt"),
                k("gu1 / gu2 / gu3", "attach a bill / scheduled / receipt"),
                k("P", "preview this row's files"),
                k("1 2 3 · h l", "inside P: jump to a file · step"),
                k("+ - 0", "inside P: zoom in, out, reset"),
                k("j k · f", "inside P: scroll · fit height / width"),
                k("H M L", "inside P: top, middle, bottom of the page"),
                k("J K", "inside P: next / previous page"),
                k("gg G", "inside P: first page, the end of the last"),
                k("t", "inside P: trim or shrink the PDF on show"),
                k("b", "inside P: add a page with a barcode drawn from a code"),
                k("a", "inside P: append another PDF's pages"),
                k(
                    "v",
                    "inside P: the PDF in the browser's own viewer, and back",
                ),
                k(
                    "h l · J K · gg G · j k · x · s",
                    "trim view: pages · scroll · keep / drop · shrink",
                ),
            ],
        ),
        (
            t.keymap_undo().to_string(),
            vec![
                k("dd", "delete — opens a confirmation"),
                k("y", "confirm in a modal"),
                k("u / U", "undo · redo — your changes, this pane, this month"),
                k(
                    "Ctrl-u · :trash",
                    "the trash — bring back any deleted row or file",
                ),
                k("Esc", "clear count, buffer, selection and search"),
            ],
        ),
        (
            t.keymap_type().to_string(),
            // From the command table, so `?` cannot advertise a command the
            // bar refuses — it listed `:add 50 mercado` long after `:add`
            // was abandoned.
            super::commands::visible(admin, holds_role)
                .map(|c| {
                    let mut key = format!(":{}", c.name);
                    for a in c.aliases {
                        key.push_str(&format!(" · :{a}"));
                    }
                    (key, (c.about)(t))
                })
                .chain([k("?", "this")])
                .collect(),
        ),
    ]
}

/// One line of the `?` legend: one or more samples, and what they mean.
pub struct LegendItem {
    pub samples: Vec<LegendSample>,
    pub text: String,
}

pub enum LegendSample {
    /// A status mark, rendered by `partials/mark.html` — the row's own.
    Mark(MarkView),
    /// The recurring or private icon, on or dim.
    Flag(FlagView),
    /// A file-rail slot: its number and its icon.
    File { number: u8, svg: &'static str },
    /// An amount as the row writes it, e.g. a projection.
    Amount(String),
}

/// The `?` legend. Every mark is built from the same `Mark` the ledger draws
/// and rendered by the same partial, so the legend cannot drift from the
/// rows. The words come from the catalog; the sample amount from `fmt`.
fn legend(
    t: &Catalog,
    currency: crate::domain::money::Currency,
    show_recurring: bool,
) -> Vec<LegendItem> {
    use crate::ui::row::{Mark, Tone, ring_tone_at};
    let ring = |progress: f32, glyph: Option<&'static str>| {
        LegendSample::Mark(
            Mark::Ring {
                progress,
                tone: if glyph.is_some() {
                    Tone::Info
                } else {
                    ring_tone_at(progress)
                },
                glyph,
            }
            .view(),
        )
    };
    let flag = |icon: &'static str, on| {
        LegendSample::Flag(FlagView {
            icon,
            svg: icons::svg(icon),
            on,
        })
    };
    let mut out = vec![
        LegendItem {
            samples: vec![LegendSample::Mark(
                Mark::Disc {
                    tone: Tone::Pos,
                    glyph: "check",
                }
                .view(),
            )],
            text: t.legend_paid().to_string(),
        },
        LegendItem {
            samples: vec![LegendSample::Mark(
                Mark::Outline {
                    tone: Tone::Pos,
                    glyph: "check",
                }
                .view(),
            )],
            text: t.legend_manual_paid().to_string(),
        },
        LegendItem {
            samples: vec![ring(0.3, None), ring(0.8, None), ring(1.0, None)],
            text: t.legend_pending().to_string(),
        },
        LegendItem {
            samples: vec![ring(0.5, Some("clock"))],
            text: t.legend_scheduled().to_string(),
        },
        LegendItem {
            samples: vec![LegendSample::Mark(
                Mark::Disc {
                    tone: Tone::Neg,
                    glyph: "alert",
                }
                .view(),
            )],
            text: t.legend_scheduled_overdue().to_string(),
        },
        LegendItem {
            samples: vec![LegendSample::Mark(
                Mark::Dot {
                    filled: false,
                    tone: Tone::Muted,
                }
                .view(),
            )],
            text: t.legend_plain().to_string(),
        },
        LegendItem {
            samples: vec![LegendSample::Amount(fmt::money_estimated(
                crate::domain::money::Cents(413_333),
                currency,
            ))],
            text: t.legend_estimated().to_string(),
        },
    ];
    // "We don't use this concept here" removes the icon from every row, so
    // it has nothing to explain either.
    if show_recurring {
        out.push(LegendItem {
            samples: vec![flag("recurring", true), flag("recurring", false)],
            text: t.legend_recurring().to_string(),
        });
    }
    out.push(LegendItem {
        samples: vec![flag("private", true), flag("private", false)],
        text: t.legend_private().to_string(),
    });
    out.push(LegendItem {
        samples: vec![
            LegendSample::File {
                number: 1,
                svg: icons::svg("bill"),
            },
            LegendSample::File {
                number: 2,
                svg: icons::svg("scheduled"),
            },
            LegendSample::File {
                number: 3,
                svg: icons::svg("receipt"),
            },
        ],
        text: t.legend_files().to_string(),
    });
    out
}

/// The twelve cells of the year on screen, each carrying whatever is
/// unresolved in it. Clicking one jumps to that month — the picker's existing
/// behavior, so no new command is needed on top of it.
///
/// Labels, dots and marks only: everything here is known without summing a
/// single pane, so the page render stays as cheap as it was.
pub(crate) fn month_cells(
    t: &Catalog,
    unresolved: &[ledger::Unresolved],
    showing: Period,
    today: NaiveDate,
) -> Vec<MonthCell> {
    let now = Period::of(today);
    let names: Vec<&str> = t.months_short().split_whitespace().collect();
    (1..=12)
        .filter_map(|m| Period::new(showing.year(), m))
        .map(|p| {
            let count = unresolved
                .iter()
                .find(|u| u.period == p)
                .map(|u| u.count)
                .unwrap_or(0);
            let future = p > now;
            MonthCell {
                key: p.to_string(),
                label: month_name(&names, p),
                current: p == showing,
                now: p == now,
                future,
                net: None,
                negative: false,
                unresolved: count,
                title: if count > 0 {
                    t.unresolved_dot(count as i64)
                } else if future {
                    t.month_future().to_string()
                } else {
                    String::new()
                },
            }
        })
        .collect()
}

fn month_name(names: &[&str], p: Period) -> String {
    names
        .get(p.month() as usize - 1)
        .map(|s| (*s).to_string())
        .unwrap_or_else(|| p.month().to_string())
}

#[derive(Deserialize)]
pub struct MonthParams {
    period: Option<String>,
}

#[derive(Template)]
#[template(path = "partials/month_trigger.html")]
struct MonthTrigger {
    t: Catalog,
    period: String,
    unresolved_total: usize,
    unresolved_title: String,
}

/// `GET /months/dot` — the picker's trigger and its dot, alone. The page draws
/// it once; this is how it stays current without a reload: after a mutation
/// that says `ffm:dot`, after any live event (a file filed by hand in August
/// resolves August's dot while September is on screen), and when the tab
/// becomes visible again — the one moment the clock can have made a bill
/// overdue with nothing else happening. Per viewer, like the page: a private
/// row's overdue state lights only its owner's and the admin's dot.
pub async fn month_dot(
    State(st): State<AppState>,
    auth: Auth,
    Query(p): Query<MonthParams>,
) -> impl IntoResponse {
    let today = crate::clock::today();
    let showing = p
        .period
        .as_deref()
        .and_then(Period::parse)
        .unwrap_or_else(|| Period::of(today));
    let t = auth.catalog();
    let unresolved =
        ledger::unresolved_months(&st.pool, auth.viewer, today, Some(&st.cfg.receipts_root))
            .await
            .unwrap_or_default();
    let total: usize = unresolved.iter().map(|u| u.count).sum();
    Html(MonthTrigger {
        period: fmt::period(showing),
        unresolved_total: total,
        unresolved_title: t.unresolved_dot(total as i64),
        t,
    })
}

/// `GET /months` — the picker's body with its numbers, fetched each time the
/// picker opens.
///
/// A month's NET is the real pane query for every member, vault included: a
/// tracked income is only confirmed — and so only stops being an estimate —
/// when its payslip is on disk. That is a dozen pane reads, which is fine for
/// a popup somebody asked to see and waste on every ledger load for one that
/// is usually closed. Fetched on every open rather than once, so a NET is
/// never stale after an edit.
pub async fn month_pop(
    State(st): State<AppState>,
    auth: Auth,
    Query(p): Query<MonthParams>,
) -> impl IntoResponse {
    use crate::domain::money::Cents;

    let today = crate::clock::today();
    let showing = p
        .period
        .as_deref()
        .and_then(Period::parse)
        .unwrap_or_else(|| Period::of(today));
    let ctx = session_ctx(&st, &auth).await;
    let currency = ctx.currency;
    let t = auth.catalog();
    let unresolved =
        ledger::unresolved_months(&st.pool, auth.viewer, today, Some(&st.cfg.receipts_root))
            .await
            .unwrap_or_default();
    let mut months = month_cells(&t, &unresolved, showing, today);

    let (mut ins, mut outs, mut estimated) = (Cents::ZERO, Cents::ZERO, false);
    let mut counted: Vec<Period> = Vec::new();
    for cell in months.iter_mut().filter(|c| !c.future) {
        let Some(period) = Period::parse(&cell.key) else {
            continue;
        };
        let (mut m_in, mut m_out, mut m_est) = (Cents::ZERO, Cents::ZERO, false);
        for pane in member_panes(&st, &auth, period, ctx).await {
            m_in += pane.ins;
            m_out += pane.outs;
            m_est |= pane.estimated;
        }
        let net = m_in - m_out;
        let compact = fmt::money_compact(net, currency);
        cell.net = Some(if m_est {
            format!("~{compact}")
        } else {
            compact
        });
        cell.negative = net.is_negative();
        ins += m_in;
        outs += m_out;
        estimated |= m_est;
        counted.push(period);
    }

    let summary = year_summary(&t, &months, &counted, ins, outs, estimated, currency);
    Html(MonthPop {
        t,
        year: showing.year(),
        months,
        summary,
    })
}

/// The foot: IN, OUT and NET over the months that have happened, their
/// average, and — only when something is flagged — how many months carry a
/// dot. Full money format: this is the one place in the picker the exact
/// figure is read.
pub(crate) fn year_summary(
    t: &Catalog,
    months: &[MonthCell],
    counted: &[Period],
    ins: crate::domain::money::Cents,
    outs: crate::domain::money::Cents,
    estimated: bool,
    currency: crate::domain::money::Currency,
) -> Option<YearSummary> {
    use crate::domain::money::Cents;

    let (&first, &last) = (counted.first()?, counted.last()?);
    let names: Vec<&str> = t.months_short().split_whitespace().collect();
    let year = first.year().to_string();
    let heading = if counted.len() == 12 {
        t.year_full(&year)
    } else if first == last {
        t.year_so_far(&year, &month_name(&names, first))
    } else {
        let range = format!("{}–{}", month_name(&names, first), month_name(&names, last));
        t.year_so_far(&year, &range)
    };
    // The same minus the ledger bar's NET uses, and a plus beside it: a
    // year's NET is a signed fact, not a direction carried by a row's kind.
    // The tilde leads, as it does in the cells: it qualifies the whole figure,
    // sign included.
    let signed = |c: Cents| {
        let sign = if c.is_negative() { "−" } else { "+" };
        let tilde = if estimated { "~" } else { "" };
        format!("{tilde}{sign}{}", fmt::money(c.abs(), currency))
    };
    let net = ins - outs;
    let avg = Cents(net.get() / counted.len() as i64);
    let flagged = months.iter().filter(|c| c.unresolved > 0).count();
    Some(YearSummary {
        heading,
        ins: money(ins, currency, estimated),
        outs: fmt::money(outs, currency),
        net: signed(net),
        negative: net.is_negative(),
        avg: t.year_avg(&signed(avg)),
        unresolved: if flagged > 0 {
            t.year_unresolved(flagged as i64)
        } else {
            String::new()
        },
    })
}

/// How `:group bucket` orders its groups — `member_settings.group_order`,
/// cycled with `S`. `Uncategorized` is last in every one of them: it is a
/// catch-all, not a category, so the order only decides among buckets
/// somebody actually chose. Before this there was only first-appearance, and
/// one unfiled income at the top of a pane pulled `Uncategorized` above
/// everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupOrder {
    /// First seen in the pane's own sort — the default, and what grouping
    /// always did, so nothing moves for a member who never asks.
    Appearance,
    /// A–Z by bucket, ignoring case and accents: "Água" files among the A's.
    Name,
    /// By what the group costs, the biggest spend first.
    Subtotal,
}

impl GroupOrder {
    pub fn parse(s: &str) -> GroupOrder {
        match s {
            "name" => GroupOrder::Name,
            "subtotal" => GroupOrder::Subtotal,
            _ => GroupOrder::Appearance,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            GroupOrder::Appearance => "appearance",
            GroupOrder::Name => "name",
            GroupOrder::Subtotal => "subtotal",
        }
    }

    /// The member's order when their pane is grouped at all, `None` when it
    /// is flat — one answer for the page and every fragment that re-renders
    /// a pane, so the two cannot group differently.
    pub fn of(settings: &crate::db::settings::MemberSettings) -> Option<GroupOrder> {
        (settings.ledger_group == "bucket").then(|| GroupOrder::parse(&settings.group_order))
    }

    pub fn label(self, t: &Catalog) -> String {
        match self {
            GroupOrder::Appearance => t.profile_group_order_appearance().to_string(),
            GroupOrder::Name => t.profile_group_order_name().to_string(),
            GroupOrder::Subtotal => t.profile_group_order_subtotal().to_string(),
        }
    }
}

/// Puts groups in `order`, `Uncategorized` last. Stable throughout, so a tie
/// — two buckets with the same subtotal, or names equal once accents and case
/// are set aside — keeps first-seen order. Generic over what a group carries
/// so the ordering can be tested without building ledger rows.
fn order_groups<T>(
    mut groups: Vec<(String, crate::domain::money::Cents, T)>,
    order: GroupOrder,
    uncategorized: &str,
) -> Vec<(String, crate::domain::money::Cents, T)> {
    match order {
        GroupOrder::Appearance => {}
        GroupOrder::Name => groups.sort_by_cached_key(|g| name_key(&g.0)),
        // Signed and ascending: a subtotal is spend minus income, negative
        // when the group costs money, so the most negative is the biggest
        // cost. A group that nets to income ranks after every cost.
        GroupOrder::Subtotal => groups.sort_by_key(|g| g.1),
    }
    // Stable, so moving the catch-all leaves the others exactly as ordered.
    groups.sort_by_key(|g| g.0 == uncategorized);
    groups
}

/// A–Z the way a Brazilian household reads it: "Água" among the A's rather
/// than after "Z", which is where a byte comparison puts an accented letter.
fn name_key(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

/// Clusters a pane's rows by `bucket`, then orders the groups by the member's
/// `group_order` — first seen by default, so a grouped pane still reads in
/// something like the order the household put it in.
///
/// Nesting stops at `bucket`: one level is enough for a household-sized
/// ledger, and a `bill_name` sub-fold would be a second gesture to learn for
/// a distinction most rows do not make.
pub fn group_rows(
    rows: &[crate::ui::row::LedgerRow],
    views: Vec<RowView>,
    t: &Catalog,
    currency: crate::domain::money::Currency,
    group_order: GroupOrder,
) -> Vec<Group> {
    let mut order: Vec<String> = Vec::new();
    let mut buckets: std::collections::HashMap<
        String,
        (crate::domain::money::Cents, Vec<RowView>),
    > = std::collections::HashMap::new();

    for (row, view) in rows.iter().zip(views) {
        let name = match row
            .bucket
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
        {
            Some(b) => b.to_string(),
            // Grouping never silently drops a row.
            None => t.group_uncategorized().to_string(),
        };
        if !buckets.contains_key(&name) {
            order.push(name.clone());
        }
        let entry = buckets
            .entry(name)
            .or_insert((crate::domain::money::Cents::ZERO, Vec::new()));
        // A subtotal is what this bucket costs, so income counts against it —
        // a "Renda" group summing to its own income is the useful reading.
        if let Some(c) = row.effective_cents() {
            if row.kind.is_income() {
                entry.0 += c;
            } else {
                entry.0 -= c;
            }
        }
        entry.1.push(view);
    }

    let seen = order
        .into_iter()
        .filter_map(|name| {
            let (total, rows) = buckets.remove(&name)?;
            Some((name, total, rows))
        })
        .collect();
    order_groups(seen, group_order, t.group_uncategorized())
        .into_iter()
        .map(|(name, total, rows)| Group {
            count: t.group_rows(rows.len() as i64),
            subtotal: fmt::money(total.abs(), currency),
            negative: total.is_negative(),
            name,
            rows,
        })
        .collect()
}

pub(crate) fn view(
    pane: Pane,
    t: &Catalog,
    currency: crate::domain::money::Currency,
    group: Option<GroupOrder>,
    writable: bool,
) -> PaneView {
    let grouped = group.is_some();
    let balance = pane.balance();
    PaneView {
        writable,
        id: pane.member.id,
        name: fmt::name(&pane.member.name),
        totals: PaneTotals {
            id: pane.member.id,
            ins: money(pane.ins, currency, pane.estimated),
            outs: fmt::money(pane.outs, currency),
            balance: money(balance.abs(), currency, pane.estimated),
            negative: balance.is_negative(),
        },
        rows: if grouped {
            Vec::new()
        } else {
            pane.rows.iter().map(|r| r.view(t)).collect()
        },
        groups: if let Some(order) = group {
            let views = pane.rows.iter().map(|r| r.view(t)).collect();
            group_rows(&pane.rows, views, t, currency, order)
        } else {
            Vec::new()
        },
        grouped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::money::{Cents, Currency};
    use crate::ui::i18n::Lang;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// The pane from the report that started this: an unfiled income first,
    /// which put `Uncategorized` above every real bucket.
    fn udo() -> Vec<(String, Cents, ())> {
        [
            ("Uncategorized", 1_912),
            ("Mobilidade", -13_145),
            ("Padaria", -35_780),
            ("Consumo", 0),
        ]
        .into_iter()
        .map(|(n, c)| (n.to_string(), Cents(c), ()))
        .collect()
    }

    fn names(groups: Vec<(String, Cents, ())>) -> Vec<String> {
        groups.into_iter().map(|g| g.0).collect()
    }

    #[test]
    fn uncategorized_is_last_in_every_order() {
        for order in [
            GroupOrder::Appearance,
            GroupOrder::Name,
            GroupOrder::Subtotal,
        ] {
            let got = names(order_groups(udo(), order, "Uncategorized"));
            assert_eq!(
                got.last().map(String::as_str),
                Some("Uncategorized"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn first_seen_keeps_the_panes_order_for_real_buckets() {
        assert_eq!(
            names(order_groups(udo(), GroupOrder::Appearance, "Uncategorized")),
            ["Mobilidade", "Padaria", "Consumo", "Uncategorized"]
        );
    }

    #[test]
    fn a_to_z_ignores_accents_and_case() {
        let groups = ["Zelador", "Água", "mercado", "Aluguel"]
            .into_iter()
            .map(|n| (n.to_string(), Cents(0), ()))
            .collect();
        assert_eq!(
            names(order_groups(groups, GroupOrder::Name, "Uncategorized")),
            ["Água", "Aluguel", "mercado", "Zelador"]
        );
    }

    #[test]
    fn by_cost_puts_the_biggest_spend_first_and_income_after_every_cost() {
        let mut groups = udo();
        groups.push(("Renda".into(), Cents(90_000), ()));
        assert_eq!(
            names(order_groups(groups, GroupOrder::Subtotal, "Uncategorized")),
            ["Padaria", "Mobilidade", "Consumo", "Renda", "Uncategorized"]
        );
    }

    #[test]
    fn a_tie_keeps_first_seen_order() {
        let groups = ["B", "A", "C"]
            .into_iter()
            .map(|n| (n.to_string(), Cents(-500), ()))
            .collect();
        assert_eq!(
            names(order_groups(groups, GroupOrder::Subtotal, "Uncategorized")),
            ["B", "A", "C"]
        );
    }

    #[test]
    fn cells_mark_the_month_on_screen_today_and_the_future_apart() {
        let t = Catalog::new(Lang::En);
        let showing = Period::new(2026, 3).unwrap();
        let cells = month_cells(&t, &[], showing, d(2026, 9, 25));
        assert_eq!(cells.len(), 12);
        assert!(cells[2].current && !cells[2].now);
        assert!(cells[8].now && !cells[8].current && !cells[8].future);
        assert!(cells[9..].iter().all(|c| c.future && c.net.is_none()));
        assert!(cells[..9].iter().all(|c| !c.future));
    }

    #[test]
    fn the_summary_covers_only_the_months_counted_and_signs_the_net() {
        let t = Catalog::new(Lang::En);
        let months = month_cells(&t, &[], Period::new(2026, 9).unwrap(), d(2026, 9, 25));
        let counted: Vec<Period> = (1..=9).filter_map(|m| Period::new(2026, m)).collect();
        let y = year_summary(
            &t,
            &months,
            &counted,
            Cents(900_000),
            Cents(1_080_000),
            false,
            Currency::Brl,
        )
        .unwrap();
        assert_eq!(y.heading, "2026 so far · Jan–Sep");
        assert_eq!(y.net, "−R$ 1.800,00");
        assert!(y.negative);
        assert_eq!(y.avg, "avg −R$ 200,00 / month");
        assert!(y.unresolved.is_empty());

        assert!(
            year_summary(
                &t,
                &months,
                &[],
                Cents::ZERO,
                Cents::ZERO,
                false,
                Currency::Brl
            )
            .is_none()
        );
    }
}
