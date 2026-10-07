//! Reading a month.
//!
//! One query per pane, plus the household and member rows that frame it. The
//! interesting part is not the SELECT — it is what the query is obliged to
//! carry, and why.
//!
//! `LedgerRow`'s contract is that **every visual channel is a pure function of
//! fields on the struct**, which is what lets the sandbox render twenty states
//! with no database at all. Three facts cannot be derived from a single row and
//! so are computed here and hung on it: the previous period's amount (the MoM
//! delta's only input), how many occurrences of a rule have happened (the
//! `7/12` badge), and — later — whether each file exists on disk.

use crate::domain::estimate;
use crate::domain::money::{Cents, Currency};
use crate::domain::period::Period;
use crate::domain::status::{RowFlags, RowState, TxKind, Visibility};
use crate::domain::viewer::Viewer;
use crate::ui::i18n::Lang;
use crate::ui::row::{FilePresence, LedgerRow, RowCtx};
use chrono::NaiveDate;
use sqlx::SqlitePool;

/// The one household row. Every member's numbers feed the same totals, which
/// is exactly why currency lives here and not beside theme.
#[derive(Clone, Debug)]
pub struct Household {
    pub name: String,
    pub currency: Currency,
    pub default_language: Lang,
    /// The household's stated policy for new rows. A member may override it,
    /// and `:profile` labels that override with what this resolves to.
    pub default_visibility: String,
    pub show_recurring: bool,
}

#[derive(Clone, Debug)]
pub struct Member {
    pub id: i64,
    pub name: String,
    pub uuid: String,
    pub slug: String,
    pub is_admin: bool,
}

/// One member's month: the rows, and the totals they add up to.
#[derive(Debug)]
pub struct Pane {
    pub member: Member,
    pub rows: Vec<LedgerRow>,
    pub ins: Cents,
    pub outs: Cents,
    /// Marked when any component of the totals is a projection rather than a
    /// fact — leaving projections out would make a future month's balance
    /// useless, and including them unmarked would present a guess as a fact.
    pub estimated: bool,
}

impl Pane {
    pub fn balance(&self) -> Cents {
        self.ins - self.outs
    }
}

pub async fn household(pool: &SqlitePool) -> sqlx::Result<Option<Household>> {
    let row = sqlx::query!(
        r#"SELECT name, locale_currency, default_language, default_visibility,
                  ledger_show_recurring
           FROM household_settings WHERE id = 1"#
    )
    .fetch_optional(pool)
    .await?;

    // The CHECK constraints make every one of these a closed set, so an
    // unparseable value would mean the schema was bypassed. Fall back rather
    // than panic: a ledger that renders in the wrong currency is recoverable,
    // a process that will not start is not.
    Ok(row.map(|r| Household {
        name: r.name,
        currency: Currency::parse(&r.locale_currency).unwrap_or_default(),
        default_language: Lang::parse(&r.default_language).unwrap_or(Lang::En),
        default_visibility: r.default_visibility,
        show_recurring: r.ledger_show_recurring != 0,
    }))
}

/// Everyone with a pane, in creation order — which is the order the tab bar and
/// the split panes read left to right.
pub async fn members(pool: &SqlitePool) -> sqlx::Result<Vec<Member>> {
    let rows = sqlx::query!(
        r#"SELECT id, name, uuid, slug, role FROM members
           WHERE deleted_at IS NULL ORDER BY id"#
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Member {
            id: r.id,
            name: r.name,
            uuid: r.uuid,
            slug: r.slug,
            is_admin: r.role == 0,
        })
        .collect())
}

/// The members with a pane in `period`: everyone not in the trash and not
/// **inactive** in it. An inactive member's pane is hidden from the month
/// they were turned off until the month they were turned back on — the
/// months before still show them — and a hidden pane takes its money with
/// it: the totals and the month picker's NETs sum these panes, so a row
/// nobody can see counts in no total either. (`member_inactive`; the same
/// test is the `NOT EXISTS` in `grep` and `unresolved_months` below.)
pub async fn members_in(pool: &SqlitePool, period: Period) -> sqlx::Result<Vec<Member>> {
    let p = period.to_string();
    let rows = sqlx::query!(
        r#"SELECT id, name, uuid, slug, role FROM members m
           WHERE deleted_at IS NULL
             AND NOT EXISTS (SELECT 1 FROM member_inactive i
                             WHERE i.member_id = m.id AND i.from_period <= ?1
                               AND (i.until_period IS NULL OR ?1 < i.until_period))
           ORDER BY id"#,
        p
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Member {
            id: r.id,
            name: r.name,
            uuid: r.uuid,
            slug: r.slug,
            is_admin: r.role == 0,
        })
        .collect())
}

/// One member's rows for one period, in stored manual order, as **this viewer**
/// is allowed to see them.
///
/// Soft-deleted rows are left out here. They are not gone — nothing is — but
/// the pane is what the household reads every day, and a deleted row comes back
/// into it through `u`, which is the mutation phase's job rather than this
/// query's.
///
/// Privacy is applied **once**, in the SELECT, and the totals are then summed
/// from the rows it returned. That is deliberate and load-bearing: filtering
/// the list in one place and the sum in another is how a hidden row announces
/// itself by making the numbers fail to add up. There is no second predicate
/// to keep in step because there is no second predicate.
pub async fn pane(
    pool: &SqlitePool,
    member: &Member,
    period: Period,
    ctx: RowCtx,
    viewer: Viewer,
) -> sqlx::Result<Pane> {
    pane_with_vault(pool, member, period, ctx, viewer, None).await
}

/// How the rows are ordered.
///
/// `Manual` is the default because this app replaces a spreadsheet where rows
/// *were* arranged by hand. The other two are **derived views**: the moment
/// rows are read in an order the household did not put them in, `J`/`K` stop
/// making sense and say so rather than pretending.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Sort {
    #[default]
    Manual,
    Date,
    Amount,
}

impl Sort {
    pub fn code(self) -> &'static str {
        match self {
            Sort::Manual => "manual",
            Sort::Date => "date",
            Sort::Amount => "amount",
        }
    }

    pub fn parse(s: &str) -> Sort {
        match s {
            "date" => Sort::Date,
            "amount" => Sort::Amount,
            _ => Sort::Manual,
        }
    }

    /// A derived view cannot be reordered by hand.
    pub fn is_derived(self) -> bool {
        self != Sort::Manual
    }
}

/// The same query, with the receipt vault available so each tracked row can be
/// asked what is actually on disk.
///
/// `root` is optional because the sandbox and most tests have no vault — and a
/// row with no files is a perfectly honest row: it reads `pending`, which is
/// what "nothing has happened yet" means.
pub async fn pane_with_vault(
    pool: &SqlitePool,
    member: &Member,
    period: Period,
    ctx: RowCtx,
    viewer: Viewer,
    root: Option<&std::path::Path>,
) -> sqlx::Result<Pane> {
    let period_s = period.to_string();
    let prev_s = period.pred().to_string();
    let sees_private = viewer.sees_private_of(member.id);
    let sort_code = ctx.sort.code();

    let rows = sqlx::query!(
        r#"
        SELECT
            t.id                 AS "id!: i64",
            t.rule_id            AS "rule_id: i64",
            t.period             AS "period!: String",
            t.kind               AS "kind!: String",
            t.title              AS "title!: String",
            t.amount_cents       AS "amount_cents: i64",
            t.date               AS "date: String",
            t.due_date           AS "due_date: String",
            t.visibility         AS "visibility!: String",
            t.bucket             AS "bucket: String",
            t.bill_name          AS "bill_name: String",
            t.manual_paid        AS "manual_paid!: i64",

            r.tracked            AS "tracked?: i64",
            r.bucket             AS "rule_bucket?: String",
            r.bill_name          AS "rule_bill_name?: String",
            r.instance           AS "instance?: String",
            r.total_installments AS "total_installments?: i64",
            r.spans_period       AS "spans_period?: i64",
            r.amount_in_filename AS "amount_in_filename?: i64",
            r.varies             AS "varies?: i64",
            r.cadence_months     AS "cadence_months?: i64",
            r.estimate_basis     AS "estimate_basis?: String",
            r.estimate_window    AS "estimate_window?: i64",
            r.amount_cents       AS "rule_amount_cents?: i64",
            -- A deleted rule still joins: its occurrences keep their files
            -- and their tracked status, which live at the rule's taxonomy.
            -- They only stop claiming to recur.
            r.deleted_at         AS "rule_deleted_at?: String",

            -- "The same line" in the previous period: exact title, same member.
            -- Unambiguous for a recurring row (it is the same rule) and the only
            -- workable answer for a one-off, which has no rule to match on. It
            -- degrades the right way: rename the line and the delta disappears,
            -- because it is not that line any more.
            (SELECT p.amount_cents
               FROM transactions p
              WHERE p.member_id = t.member_id
                AND p.title     = t.title
                AND p.period    = ?2
                AND p.deleted_at IS NULL
              ORDER BY p.sort_key
              LIMIT 1)           AS "prev_amount_cents: i64",

            -- The `7/12` badge. Counts occurrences of this rule up to and
            -- including this period, so it reads as position in the run.
            (SELECT COUNT(*)
               FROM transactions o
              WHERE o.rule_id IS NOT NULL
                AND o.rule_id = t.rule_id
                AND o.period <= t.period
                AND o.deleted_at IS NULL) AS "installments_paid!: i64"
        FROM transactions t
        LEFT JOIN recurring_rules r ON r.id = t.rule_id
        WHERE t.member_id  = ?1
          AND t.period     = ?3
          AND t.deleted_at IS NULL
          AND (t.visibility = 'household' OR ?4)
        -- Ordered three ways from one query rather than three, because the
        -- SELECT above is the expensive part and duplicating it is how two of
        -- them quietly stop agreeing about what a row is.
        ORDER BY
          CASE ?5
            -- An unpaid row has no paid-on date yet, so it sorts by the day
            -- it is DUE. Falling back to nothing would bunch every unpaid row
            -- at one end, which is the opposite of what sorting by date is
            -- for.
            WHEN 'date'   THEN coalesce(t.date, t.due_date, t.period)
            WHEN 'amount' THEN NULL
            ELSE t.sort_key
          END ASC,
          -- Biggest first: sorting a ledger by amount is asking "what is
          -- costing us the most".
          CASE ?5 WHEN 'amount' THEN t.amount_cents END DESC,
          t.sort_key ASC
        "#,
        member.id,
        prev_s,
        period_s,
        sees_private,
        sort_code,
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    let mut ins = Cents::ZERO;
    let mut outs = Cents::ZERO;
    // The rule facts a projection needs, kept beside the rows rather than on
    // them: they decide the estimate, they are not rendered.
    let mut basis_of: Vec<Option<String>> = Vec::with_capacity(rows.len());
    let mut window_of: Vec<Option<i64>> = Vec::with_capacity(rows.len());
    let mut rule_amount_of: Vec<Option<i64>> = Vec::with_capacity(rows.len());

    for (i, r) in rows.into_iter().enumerate() {
        let kind = TxKind::parse(&r.kind).unwrap_or(TxKind::Out);
        let amount = r.amount_cents.map(Cents);
        if let Some(a) = amount {
            match kind {
                TxKind::In => ins += a,
                TxKind::Out => outs += a,
            }
        }

        let rule_deleted_on = r.rule_deleted_at.as_deref().and_then(local_day);
        let recurring = r.rule_id.is_some() && r.rule_deleted_at.is_none();
        let private = r.visibility == "private";
        basis_of.push(r.estimate_basis.clone());
        window_of.push(r.estimate_window);
        rule_amount_of.push(r.rule_amount_cents);

        out.push(LedgerRow {
            id: r.id,
            rule_id: r.rule_id,
            period: Period::parse(&r.period).unwrap_or(period),
            kind,
            title: r.title,
            amount_cents: amount,
            date: r.date.as_deref().and_then(parse_date),
            due_date: r.due_date.as_deref().and_then(parse_date),
            visibility: Visibility::parse(&r.visibility).unwrap_or_default(),
            bucket: r.bucket,
            bill_name: r.bill_name,
            manual_paid: r.manual_paid != 0,

            file_bucket: r.rule_bucket,
            file_bill_name: r.rule_bill_name,
            tracked: r.tracked.unwrap_or(0) != 0,
            instance: r.instance,
            total_installments: r.total_installments.map(|n| n as u16),
            spans_period: r.spans_period.unwrap_or(0) != 0,
            // Filled with the files below, from the month's code.
            bill_page: None,
            bill_path: None,
            amount_in_filename: r.amount_in_filename.unwrap_or(0) != 0,
            varies: r.varies.unwrap_or(0) != 0,
            cadence_months: r.cadence_months,
            rule_deleted_on,

            member_slug: member.slug.clone(),

            prev_amount_cents: r.prev_amount_cents.map(Cents),
            // Computed below, once the row exists to be asked about. Derived
            // on read and never stored, so it follows new history instead of
            // freezing an old guess — and never writes a spurious "amount
            // changed" entry into the log.
            estimate_cents: None,
            installments_paid: recurring.then_some(r.installments_paid as u16),
            // Filled in below, once the row exists to be asked about.
            files: FilePresence::default(),

            state: RowState::Ok,
            flags: RowFlags {
                private,
                recurring,
                ..RowFlags::default()
            },
            // Relative is measured from the cursor, which the server does not
            // have — but the cursor always starts on the first row, so
            // relative-to-0 is right for the first frame and `paint` keeps it
            // right from there.
            gutter: ctx.line_numbers.gutter(i, 0),
            ctx,
        });
    }

    // A varying income shows a projection until the money lands. Done here
    // rather than in `LedgerRow` because it needs the last N confirmed
    // periods, which is a query — and the row's contract is that every visual
    // channel is a function of fields ON the row.
    for (i, row) in out.iter_mut().enumerate() {
        if !row.kind.is_income() || !row.varies {
            continue;
        }
        let basis =
            estimate::Basis::parse(basis_of.get(i).and_then(|b| b.as_deref()).unwrap_or(""))
                .unwrap_or_default();
        let window = window_of
            .get(i)
            .copied()
            .flatten()
            .unwrap_or(3)
            .clamp(1, 12) as usize;
        let fallback = rule_amount_of.get(i).copied().flatten().map(Cents);
        let Some(rule_id) = row.rule_id else { continue };

        let history = confirmed_history(pool, root, rule_id, row.period, window).await?;
        if let Some(p) = estimate::project(&history, basis, window, fallback) {
            row.estimate_cents = Some(p.amount);
        }
    }

    // The disk decides `paid`, so it is consulted per tracked row — one exact
    // path per kind, never a traversal. An untracked row has no taxonomy to
    // file under and is skipped entirely.
    if let Some(root) = root {
        for row in out.iter_mut().filter(|r| r.tracked) {
            // The path is `slot_of`'s, the one place it is decided — never
            // rebuilt here from the row's own bucket, which is how the ledger
            // came to look in one folder while uploads wrote to another.
            let Ok(Some((slot, _))) = crate::services::receipts::slot_of(pool, row.id).await else {
                continue;
            };
            if let Ok(found) = crate::services::receipts::presence(pool, root, &slot).await {
                row.files = found;
            }
            // A bill covering several months is one file on one row: every
            // month it has a code for shows it, on that code's page.
            if row.spans_period
                && let Some(shared) =
                    crate::services::receipts::shared_bill(pool, root, row.id).await
            {
                row.bill_page = Some(shared.page);
                if row.files.bill.is_none() {
                    row.files.bill = Some(shared.ext);
                    row.bill_path = Some(shared.path);
                }
            }
        }
    }

    // Any projection in the pane marks its INs and Balance. Leaving
    // projections out of the totals would make a future month's balance
    // useless; putting them in unmarked would present a guess as a fact.
    let estimated = out.iter().any(|r| r.is_estimated());

    // The totals were summed from the STORED amounts as the rows were read;
    // a projected row contributes its projection instead, or the balance
    // would disagree with the row right above it.
    for row in out.iter().filter(|r| r.is_estimated()) {
        if let (Some(est), stored) = (row.estimate_cents, row.amount_cents) {
            ins += est;
            ins -= stored.unwrap_or(Cents::ZERO);
        }
    }

    Ok(Pane {
        member: member.clone(),
        rows: out,
        ins,
        outs,
        estimated,
    })
}

/// The last `window` **confirmed** periods of a rule, newest first.
///
/// Confirmed means the same thing it means everywhere else: a payslip on disk
/// for a tracked rule, the manual tick for an untracked one. An unconfirmed
/// month is skipped rather than folded in — two of them back to back would
/// otherwise project a guess from another guess.
async fn confirmed_history(
    pool: &SqlitePool,
    root: Option<&std::path::Path>,
    rule_id: i64,
    before: Period,
    window: usize,
) -> sqlx::Result<Vec<estimate::Confirmed>> {
    let before_s = before.to_string();
    // A generous candidate list: unconfirmed months are skipped, so the window
    // has to be able to reach past them to find real ones.
    let limit = (window as i64 * 6).clamp(6, 48);

    let rows = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.period AS "period!: String",
                  t.amount_cents AS "amount_cents: i64",
                  t.manual_paid AS "manual_paid!: i64",
                  r.tracked AS "tracked!: i64"
           FROM transactions t
           JOIN recurring_rules r ON r.id = t.rule_id
           WHERE t.rule_id = ?1 AND t.period < ?2 AND t.deleted_at IS NULL
             AND t.amount_cents IS NOT NULL
           ORDER BY t.period DESC
           LIMIT ?3"#,
        rule_id,
        before_s,
        limit,
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::new();
    for r in rows {
        if out.len() >= window {
            break;
        }
        let amount = Cents(r.amount_cents.unwrap_or(0));
        let confirmed = if r.tracked != 0 {
            // No manual tick for a tracked rule, ever — the disk answers, at
            // the path `slot_of` decides (it used to be rebuilt here from the
            // row's bucket, and with a hardcoded currency in the filename).
            match root {
                Some(root) => match crate::services::receipts::slot_of(pool, r.id).await {
                    Ok(Some((slot, _))) => crate::services::receipts::presence(pool, root, &slot)
                        .await
                        .map(|p| p.receipt.is_some())
                        .unwrap_or(false),
                    _ => false,
                },
                // No vault to consult: nothing is confirmed, which is the
                // honest answer rather than a convenient one.
                _ => false,
            }
        } else {
            r.manual_paid != 0
        };

        if confirmed {
            out.push(estimate::Confirmed {
                period: r.period,
                amount,
            });
        }
    }
    Ok(out)
}

/// Dates are ISO-8601 TEXT, fixed width, so this is exact rather than lenient.
/// A value that does not parse came from outside the app.
fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// The household's own day for a stored UTC timestamp — a rule deleted at
/// 22:00 in São Paulo was deleted that day, not the next.
pub fn local_day(s: &str) -> Option<NaiveDate> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&chrono::Local).date_naive())
}

// ---------------------------------------------------------------------------
// `:grep` — the different verb
// ---------------------------------------------------------------------------

/// One hit, with enough context to act on it.
#[derive(Clone, Debug)]
pub struct Hit {
    pub id: i64,
    pub period: Period,
    /// Whose pane the row sits in, so picking the hit can land there.
    pub member_id: i64,
    pub member: String,
    pub title: String,
    pub amount: Option<Cents>,
    pub is_income: bool,
}

/// Searches **every** month, which is why it is a different verb from `/`.
///
/// Privacy is applied here exactly as it is on a pane read: a row you cannot
/// see is a row you cannot find, or the search would leak the existence of
/// something the ledger carefully hides. The FTS index is external-content, so
/// there is no second copy of a private row's title to leak from either.
pub async fn grep(pool: &SqlitePool, viewer: Viewer, term: &str) -> sqlx::Result<Vec<Hit>> {
    let term = term.trim();
    if term.is_empty() {
        return Ok(Vec::new());
    }
    // FTS5 treats a bare term as a query language — a stray `"` or `*` is a
    // syntax error, not a search. Quoting makes the whole thing one phrase,
    // which is what somebody typing into a search box means.
    let query = format!("\"{}\"*", term.replace('"', "\"\""));
    let viewer_id = viewer.member_id;
    let sees_all = viewer.is_admin();

    let rows = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.period AS "period!: String",
                  t.title AS "title!: String", t.kind AS "kind!: String",
                  t.amount_cents AS "amount_cents: i64",
                  t.member_id AS "member_id!: i64",
                  m.name AS "member!: String"
           FROM transactions_fts f
           JOIN transactions t ON t.id = f.rowid
           JOIN members m ON m.id = t.member_id
           WHERE transactions_fts MATCH ?1
             AND t.deleted_at IS NULL
             AND (t.visibility = 'household' OR t.member_id = ?2 OR ?3)
             -- A month a member was inactive in has no pane for them, so a
             -- hit there would land on nothing (`members_in`).
             AND NOT EXISTS (SELECT 1 FROM member_inactive i
                             WHERE i.member_id = t.member_id AND i.from_period <= t.period
                               AND (i.until_period IS NULL OR t.period < i.until_period))
           ORDER BY t.period DESC, t.sort_key
           LIMIT 100"#,
        query,
        viewer_id,
        sees_all,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(Hit {
                id: r.id,
                period: Period::parse(&r.period)?,
                member_id: r.member_id,
                member: crate::ui::fmt::name(&r.member),
                title: r.title,
                amount: r.amount_cents.map(Cents),
                is_income: r.kind == "in",
            })
        })
        .collect())
}

// ---------------------------------------------------------------------------
// unresolved issues across months
// ---------------------------------------------------------------------------

/// A month with something still owed in it.
///
/// "Unresolved" means a **tracked** occurrence that is overdue with no
/// receipt: `pending` past its due date, or a scheduled payment whose promise
/// was never confirmed. An untracked bill sitting without a manual tick is
/// **not** an issue — there is no filesystem evidence to act on, so flagging
/// it would be nagging based on nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unresolved {
    pub period: Period,
    pub count: usize,
}

/// Every month of the current calendar year with an unresolved occurrence.
///
/// **Bounded to the year on purpose.** On a date in September 2026 that means
/// January–August 2026 get checked, not December 2025: a badge that reaches
/// back forever is a badge nobody can ever clear.
///
/// Per viewer, by the same rule every other read follows — a private row's
/// unresolved status counts for its owner and the admin, never for a member
/// who cannot see the row itself. Otherwise the dot would announce the
/// existence of something the ledger carefully hides.
pub async fn unresolved_months(
    pool: &SqlitePool,
    viewer: Viewer,
    today: NaiveDate,
    root: Option<&std::path::Path>,
) -> sqlx::Result<Vec<Unresolved>> {
    let current = Period::of(today);
    let year_start = format!("{}-01", current.year());
    let today_s = today.format("%Y-%m-%d").to_string();
    let viewer_id = viewer.member_id;
    let sees_all = viewer.is_admin();

    // Tracked, overdue, and not in the month already on screen — the badge is
    // about somewhere ELSE needing attention.
    let rows = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.period AS "period!: String"
           FROM transactions t
           JOIN recurring_rules r ON r.id = t.rule_id
           WHERE r.tracked = 1
             AND t.deleted_at IS NULL
             AND t.period >= ?1
             AND t.due_date IS NOT NULL
             AND t.due_date < ?2
             AND (t.visibility = 'household' OR t.member_id = ?3 OR ?4)
             -- Nor a month the member was inactive in: nothing there is shown.
             AND NOT EXISTS (SELECT 1 FROM member_inactive i
                             WHERE i.member_id = t.member_id AND i.from_period <= t.period
                               AND (i.until_period IS NULL OR t.period < i.until_period))
           ORDER BY t.period"#,
        year_start,
        today_s,
        viewer_id,
        sees_all,
    )
    .fetch_all(pool)
    .await?;

    let mut months: Vec<Unresolved> = Vec::new();
    for r in rows {
        let Some(period) = Period::parse(&r.period) else {
            continue;
        };
        // The current month is where the person already is; the badge points
        // at the ones they are not looking at.
        if period == current {
            continue;
        }

        // The disk decides, here as everywhere: a receipt on disk resolves it,
        // whatever the database thinks.
        // The path is `slot_of`'s, as everywhere.
        let resolved = match root {
            Some(root) => match crate::services::receipts::slot_of(pool, r.id).await {
                Ok(Some((slot, _))) => crate::services::receipts::presence(pool, root, &slot)
                    .await
                    .map(|p| p.receipt.is_some())
                    .unwrap_or(false),
                _ => false,
            },
            // No vault to consult: nothing is resolved, which is the honest
            // answer rather than a convenient one.
            None => false,
        };
        if resolved {
            continue;
        }

        match months.last_mut() {
            Some(m) if m.period == period => m.count += 1,
            _ => months.push(Unresolved { period, count: 1 }),
        }
    }
    Ok(months)
}
