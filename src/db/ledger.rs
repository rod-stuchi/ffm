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
use crate::domain::schedule::Plan;
use crate::domain::status::{RowFlags, RowState, TxKind, Visibility};
use crate::domain::viewer::Viewer;
use crate::ui::i18n::Lang;
use crate::ui::row::{FilePresence, LedgerRow, Progress, RowCtx};
use chrono::NaiveDate;
use sqlx::SqlitePool;
use std::collections::HashMap;

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
/// *were* arranged by hand. The other three are **derived views**: the moment
/// rows are read in an order the household did not put them in, `J`/`K` stop
/// making sense and say so rather than pretending.
///
/// Declared in the order `s` cycles them — [`Sort::ALL`] is that order, and
/// `settings::LEDGER_SORTS` spells it.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Sort {
    #[default]
    Manual,
    /// A–Z by title, ignoring case and accents; a tie keeps manual order.
    Name,
    /// Biggest first.
    Amount,
    /// Paid on, or due on while unpaid.
    Date,
}

impl Sort {
    /// Every order, in the order `s` steps through them.
    pub const ALL: [Sort; 4] = [Sort::Manual, Sort::Name, Sort::Amount, Sort::Date];

    pub fn code(self) -> &'static str {
        match self {
            Sort::Manual => "manual",
            Sort::Name => "name",
            Sort::Amount => "amount",
            Sort::Date => "date",
        }
    }

    pub fn parse(s: &str) -> Sort {
        match s {
            "name" => Sort::Name,
            "amount" => Sort::Amount,
            "date" => Sort::Date,
            _ => Sort::Manual,
        }
    }

    /// The order `s` goes to from this one, round again after the last.
    pub fn next(self) -> Sort {
        let at = Sort::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Sort::ALL[(at + 1) % Sort::ALL.len()]
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
            t.sort_key           AS "sort_key!: String",

            r.tracked            AS "tracked?: i64",
            r.bucket             AS "rule_bucket?: String",
            r.bill_name          AS "rule_bill_name?: String",
            r.instance           AS "instance?: String",
            r.total_installments AS "total_installments?: i64",
            r.start_date         AS "rule_start?: String",
            r.end_date           AS "rule_end?: String",
            r.months             AS "rule_months?: String",
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

            -- "The same line" in the previous period, same member: the same
            -- rule first, then the exact title — the order the cursor follows
            -- a row to another month. The rule is what survives a rename: a
            -- recurring row renamed this month is still that bill. A one-off
            -- has no rule, and for it the title degrades the right way: rename
            -- the line and the delta disappears, because it is not that line
            -- any more.
            --
            -- The row is picked by id, so a rule's row with no amount yet is
            -- still the one compared against, never a same-titled other.
            (SELECT p.amount_cents
               FROM transactions p
              WHERE p.id = COALESCE(
                    (SELECT q.id FROM transactions q
                      WHERE q.member_id = t.member_id
                        AND q.rule_id   = t.rule_id
                        AND q.period    = ?2
                        AND q.deleted_at IS NULL
                      ORDER BY q.sort_key LIMIT 1),
                    (SELECT q.id FROM transactions q
                      WHERE q.member_id = t.member_id
                        AND q.title     = t.title
                        AND q.period    = ?2
                        AND q.deleted_at IS NULL
                      ORDER BY q.sort_key LIMIT 1))
            )                    AS "prev_amount_cents: i64"
        FROM transactions t
        LEFT JOIN recurring_rules r ON r.id = t.rule_id
        WHERE t.member_id  = ?1
          AND t.period     = ?3
          AND t.deleted_at IS NULL
          AND (t.visibility = 'household' OR ?4)
        -- Ordered from one query rather than one per order, because the
        -- SELECT above is the expensive part and duplicating it is how two of
        -- them quietly stop agreeing about what a row is. `name` reads
        -- `sort_key` here and is sorted below, in Rust: SQLite's `lower()`
        -- folds only ASCII, and "Água" belongs among the A's.
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

    // By title, A–Z the way the household reads it. Stable, so rows whose
    // titles are equal once case and accents are set aside keep the manual
    // order the query read them in. Before the rows are numbered, so the
    // gutter counts what is on screen.
    // Where each row sits in the MANUAL order, read before anything re-sorts
    // them. Under "first seen" a fold is placed by its earliest row here, so
    // `s` re-sorts the rows inside the folds and never moves a fold — which
    // first seen in the CURRENT sort did, every time a sort put another
    // bucket's row first.
    let manual_of: HashMap<i64, usize> = {
        let mut keyed: Vec<(&str, i64)> =
            rows.iter().map(|r| (r.sort_key.as_str(), r.id)).collect();
        keyed.sort_unstable();
        keyed
            .into_iter()
            .enumerate()
            .map(|(at, (_, id))| (id, at))
            .collect()
    };

    let mut rows = rows;
    if ctx.sort == Sort::Name {
        rows.sort_by_cached_key(|r| crate::domain::collate::name_key(&r.title));
    }

    let mut out = Vec::with_capacity(rows.len());
    let mut ins = Cents::ZERO;
    let mut outs = Cents::ZERO;
    // The rule facts a projection needs, kept beside the rows rather than on
    // them: they decide the estimate, they are not rendered.
    let mut basis_of: Vec<Option<String>> = Vec::with_capacity(rows.len());
    let mut window_of: Vec<Option<i64>> = Vec::with_capacity(rows.len());
    let mut rule_amount_of: Vec<Option<i64>> = Vec::with_capacity(rows.len());
    // Which months the rule runs in, for the `3/10` badge — the same
    // `domain::schedule` answer the materializer acts on.
    let mut plan_of: Vec<Option<Plan>> = Vec::with_capacity(rows.len());

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
        plan_of.push(
            r.rule_start
                .as_deref()
                .filter(|_| recurring)
                .and_then(|start| {
                    Plan::from_columns(
                        start,
                        r.rule_end.as_deref(),
                        r.cadence_months.unwrap_or(1),
                        r.rule_months.as_deref(),
                        r.total_installments,
                    )
                }),
        );

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

            manual: manual_of.get(&r.id).copied().unwrap_or(0),
            prev_amount_cents: r.prev_amount_cents.map(Cents),
            // Computed below, once the row exists to be asked about. Derived
            // on read and never stored, so it follows new history instead of
            // freezing an old guess — and never writes a spurious "amount
            // changed" entry into the log.
            estimate_cents: None,
            // Filled in below: it needs the rule's other months.
            progress: None,
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

    // Three facts below need OTHER rows than the ones on screen: a varying
    // income's projection (its last confirmed months), the `3/10` badge (every
    // month of the rule's run) and the files (the disk). Each used to ask per
    // row, and per month of each run — hundreds of queries and stats for one
    // pane. So the rows they need are gathered first, the vault is asked once
    // for all of them, and the three are computed from that one answer.

    // A varying income shows a projection until the money lands. Done here
    // rather than in `LedgerRow` because it needs the last N confirmed
    // periods, which is a query — and the row's contract is that every visual
    // channel is a function of fields ON the row.
    let varying: Vec<(usize, (i64, Period, usize))> = out
        .iter()
        .enumerate()
        .filter(|(_, row)| row.kind.is_income() && row.varies)
        .filter_map(|(i, row)| {
            let window = estimate_window(window_of.get(i).copied().flatten());
            Some((i, (row.rule_id?, row.period, window)))
        })
        .collect();
    let wants: Vec<(i64, Period, usize)> = varying.iter().map(|(_, w)| *w).collect();
    let mut history: Vec<Option<Vec<Occurrence>>> = (0..out.len()).map(|_| None).collect();
    for ((i, _), candidates) in varying.iter().zip(history_for(pool, &wants).await?) {
        history[*i] = Some(candidates);
    }

    // How far each rule's run has come as of this month: the `3/10` badge and
    // the popup's Progress line. A rule with nothing to count to — open-ended,
    // every month — has no run and is skipped.
    let runs: Vec<Option<Vec<Period>>> = out
        .iter()
        .enumerate()
        .map(|(i, row)| {
            row.rule_id?;
            plan_of.get(i)?.as_ref()?.run_for(row.period)
        })
        .collect();
    let spans: Vec<(i64, Period, Period)> = out
        .iter()
        .zip(&runs)
        .filter_map(|(row, run)| {
            let run = run.as_ref()?;
            Some((row.rule_id?, *run.first()?, *run.last()?))
        })
        .collect();
    let run_rows = run_occurrences(pool, &spans).await?;
    let planned: Vec<i64> = spans.iter().map(|(rule, _, _)| *rule).collect();
    let codes = crate::db::bill_codes::amounts_for(pool, &planned).await?;

    // The disk decides `paid`, so it is consulted for every tracked row — one
    // exact path per kind, never a traversal — and for every tracked month a
    // projection or a run counts. An untracked row has no taxonomy to file
    // under and is skipped entirely. The path is `slot_of`'s (`slots_for`),
    // the one place it is decided — never rebuilt here from the row's own
    // bucket, which is how the ledger came to look in one folder while
    // uploads wrote to another.
    let files: HashMap<i64, FilePresence> = match root {
        Some(root) => {
            let mut ids: Vec<i64> = out.iter().filter(|r| r.tracked).map(|r| r.id).collect();
            ids.extend(
                history
                    .iter()
                    .flatten()
                    .flatten()
                    .filter(|o| o.tracked)
                    .map(|o| o.id),
            );
            for (row, run) in out.iter().zip(&runs) {
                let (Some(rule_id), Some(run)) = (row.rule_id, run) else {
                    continue;
                };
                ids.extend(
                    in_run(&run_rows, rule_id, run)
                        .filter(|o| o.tracked && o.period <= row.period)
                        .map(|o| o.id),
                );
            }
            ids.sort_unstable();
            ids.dedup();
            // A vault that cannot be read leaves the rows reading `pending`,
            // as a row whose path could not be built always has.
            crate::services::receipts::files_of(pool, root, &ids)
                .await
                .unwrap_or_default()
        }
        None => HashMap::new(),
    };

    for row in out.iter_mut().filter(|r| r.tracked) {
        if let Some(found) = files.get(&row.id) {
            row.files = found.clone();
        }
    }

    // A bill covering several months is one file on one row: every month it
    // has a code for shows it, on that code's page. Only for a row whose
    // files could be looked for at all — one with a path.
    if let Some(root) = root {
        let ids: Vec<i64> = out
            .iter()
            .filter(|r| r.tracked && r.spans_period && files.contains_key(&r.id))
            .map(|r| r.id)
            .collect();
        let shared = crate::services::receipts::shared_bills(pool, root, &ids)
            .await
            .unwrap_or_default();
        for row in out.iter_mut() {
            if let Some(shared) = shared.get(&row.id) {
                row.bill_page = Some(shared.page);
                if row.files.bill.is_none() {
                    row.files.bill = Some(shared.ext.clone());
                    row.bill_path = Some(shared.path.clone());
                }
            }
        }
    }

    for (i, row) in out.iter_mut().enumerate() {
        let Some(candidates) = &history[i] else {
            continue;
        };
        let window = estimate_window(window_of.get(i).copied().flatten());
        row.estimate_cents = project(
            &confirmed_of(candidates, window, &files),
            basis_of.get(i).and_then(|b| b.as_deref()),
            window,
            rule_amount_of.get(i).copied().flatten(),
        );
    }

    for (i, row) in out.iter_mut().enumerate() {
        let (Some(rule_id), Some(run)) = (row.rule_id, &runs[i]) else {
            continue;
        };
        let fallback = rule_amount_of.get(i).copied().flatten().map(Cents);
        let rows: Vec<&Occurrence> = in_run(&run_rows, rule_id, run).collect();
        row.progress = Some(progress_from(
            run,
            row.period,
            &rows,
            codes.get(&rule_id),
            fallback,
            &files,
        ));
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

/// A varying income's row, as much as projecting it needs.
pub(crate) struct Varying<'a> {
    pub id: i64,
    pub rule_id: i64,
    pub period: Period,
    pub tracked: bool,
    pub manual_paid: bool,
    pub basis: Option<&'a str>,
    pub window: Option<i64>,
    pub rule_amount: Option<i64>,
}

/// What each varying income **not yet confirmed** is projected at, keyed by
/// row id — `None`-free: a row with nothing to project from, or one whose
/// money has landed, is absent.
///
/// The pane and `:charts` must put the same `~` figure in a month's IN, so
/// both reach it through the same steps — [`history_for`],
/// [`confirmed_of`], [`project`] — with the vault asked once for the whole
/// batch rather than once per row. `:charts` reads months by the year, and
/// asked twice per varying income in the range.
pub(crate) async fn unconfirmed_projections(
    pool: &SqlitePool,
    root: Option<&std::path::Path>,
    rows: &[Varying<'_>],
) -> sqlx::Result<HashMap<i64, Cents>> {
    let tracked: Vec<i64> = rows.iter().filter(|r| r.tracked).map(|r| r.id).collect();
    let own = vault_files(pool, root, &tracked).await;
    let open: Vec<&Varying> = rows
        .iter()
        .filter(|r| !confirmed(r.tracked, r.manual_paid, r.id, &own))
        .collect();

    let wants: Vec<(i64, Period, usize)> = open
        .iter()
        .map(|r| (r.rule_id, r.period, estimate_window(r.window)))
        .collect();
    let candidates = history_for(pool, &wants).await?;
    let ids: Vec<i64> = candidates
        .iter()
        .flatten()
        .filter(|o| o.tracked)
        .map(|o| o.id)
        .collect();
    let files = vault_files(pool, root, &ids).await;

    Ok(open
        .iter()
        .zip(&candidates)
        .filter_map(|(r, c)| {
            let window = estimate_window(r.window);
            let p = project(
                &confirmed_of(c, window, &files),
                r.basis,
                window,
                r.rule_amount,
            )?;
            Some((r.id, p))
        })
        .collect())
}

/// The projection itself, from confirmed history already in hand.
fn project(
    history: &[estimate::Confirmed],
    basis: Option<&str>,
    window: usize,
    rule_amount: Option<i64>,
) -> Option<Cents> {
    let basis = estimate::Basis::parse(basis.unwrap_or("")).unwrap_or_default();
    estimate::project(history, basis, window, rule_amount.map(Cents)).map(|p| p.amount)
}

/// The rule's `estimate_window`, as the projection reads it.
fn estimate_window(window: Option<i64>) -> usize {
    window.unwrap_or(3).clamp(1, 12) as usize
}

/// One occurrence of a rule, as much as confirming it and counting it needs.
#[derive(Clone, Debug)]
struct Occurrence {
    id: i64,
    period: Period,
    amount_cents: Option<i64>,
    manual_paid: bool,
    tracked: bool,
}

impl Occurrence {
    fn confirmed(&self, files: &HashMap<i64, FilePresence>) -> bool {
        confirmed(self.tracked, self.manual_paid, self.id, files)
    }
}

/// Whether the money moved — the same answer the mark gives: the receipt (or
/// payslip) on disk for a tracked rule, the manual tick for an untracked one.
/// No manual tick for a tracked rule, ever. `files` is the vault's answer for
/// the batch; a tracked occurrence missing from it has no file, or no path to
/// have one at.
fn confirmed(
    tracked: bool,
    manual_paid: bool,
    id: i64,
    files: &HashMap<i64, FilePresence>,
) -> bool {
    if tracked {
        files.get(&id).is_some_and(|f| f.receipt.is_some())
    } else {
        manual_paid
    }
}

/// The months each varying income's projection may be drawn from, newest
/// first — one list per `(rule, before, window)` asked, in the order asked.
///
/// A generous candidate list: unconfirmed months are skipped, so the window
/// has to be able to reach past them to find real ones. Every rule's history
/// is read in ONE query and sliced here, since a pane projects a row or two
/// and `:charts` a row per unconfirmed month of a decade.
async fn history_for(
    pool: &SqlitePool,
    wants: &[(i64, Period, usize)],
) -> sqlx::Result<Vec<Vec<Occurrence>>> {
    let Some(latest) = wants.iter().map(|(_, before, _)| *before).max() else {
        return Ok(Vec::new());
    };
    let rules: Vec<i64> = wants.iter().map(|(rule, _, _)| *rule).collect();
    let rules = serde_json::to_string(&rules).unwrap_or_else(|_| "[]".into());
    let latest_s = latest.to_string();

    let rows = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.rule_id AS "rule_id!: i64",
                  t.period AS "period!: String",
                  t.amount_cents AS "amount_cents: i64",
                  t.manual_paid AS "manual_paid!: i64",
                  r.tracked AS "tracked!: i64"
           FROM transactions t
           JOIN recurring_rules r ON r.id = t.rule_id
           WHERE t.rule_id IN (SELECT value FROM json_each(?1))
             AND t.period < ?2 AND t.deleted_at IS NULL
             AND t.amount_cents IS NOT NULL
           ORDER BY t.period DESC"#,
        rules,
        latest_s,
    )
    .fetch_all(pool)
    .await?;

    let mut by_rule: HashMap<i64, Vec<Occurrence>> = HashMap::new();
    for r in rows {
        let Some(period) = Period::parse(&r.period) else {
            continue;
        };
        by_rule.entry(r.rule_id).or_default().push(Occurrence {
            id: r.id,
            period,
            amount_cents: r.amount_cents,
            manual_paid: r.manual_paid != 0,
            tracked: r.tracked != 0,
        });
    }
    Ok(wants
        .iter()
        .map(|(rule, before, window)| {
            let limit = (*window * 6).clamp(6, 48);
            by_rule
                .get(rule)
                .into_iter()
                .flatten()
                .filter(|o| o.period < *before)
                .take(limit)
                .cloned()
                .collect()
        })
        .collect())
}

/// The last `window` **confirmed** periods among `candidates`, newest first.
///
/// Confirmed means the same thing it means everywhere else: a payslip on disk
/// for a tracked rule, the manual tick for an untracked one. An unconfirmed
/// month is skipped rather than folded in — two of them back to back would
/// otherwise project a guess from another guess.
fn confirmed_of(
    candidates: &[Occurrence],
    window: usize,
    files: &HashMap<i64, FilePresence>,
) -> Vec<estimate::Confirmed> {
    candidates
        .iter()
        .filter(|o| o.confirmed(files))
        .take(window)
        .map(|o| estimate::Confirmed {
            period: o.period.to_string(),
            amount: Cents(o.amount_cents.unwrap_or(0)),
        })
        .collect()
}

/// The vault's answer for a batch of tracked occurrences; empty with no
/// vault, so nothing reads as confirmed — the honest answer rather than a
/// convenient one.
async fn vault_files(
    pool: &SqlitePool,
    root: Option<&std::path::Path>,
    ids: &[i64],
) -> HashMap<i64, FilePresence> {
    match root {
        Some(root) => crate::services::receipts::files_of(pool, root, ids)
            .await
            .unwrap_or_default(),
        None => HashMap::new(),
    }
}

/// Every occurrence of the rules in `spans`, each `(rule, first, last)` of a
/// run, in ONE query — a pane counts several runs, and used to read each with
/// its own. Keyed by rule; [`in_run`] narrows a rule's rows to one run.
///
/// The rows are the rule's in any member's ledger, as a run is: a row handed
/// to somebody else is still that installment.
async fn run_occurrences(
    pool: &SqlitePool,
    spans: &[(i64, Period, Period)],
) -> sqlx::Result<HashMap<i64, Vec<Occurrence>>> {
    let mut out: HashMap<i64, Vec<Occurrence>> = HashMap::new();
    let (Some(first), Some(last)) = (
        spans.iter().map(|(_, f, _)| *f).min(),
        spans.iter().map(|(_, _, l)| *l).max(),
    ) else {
        return Ok(out);
    };
    let rules: Vec<i64> = spans.iter().map(|(r, _, _)| *r).collect();
    let rules = serde_json::to_string(&rules).unwrap_or_else(|_| "[]".into());
    let (first_s, last_s) = (first.to_string(), last.to_string());
    let rows = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.rule_id AS "rule_id!: i64",
                  t.period AS "period!: String",
                  t.amount_cents AS "amount_cents: i64",
                  t.manual_paid AS "manual_paid!: i64",
                  r.tracked AS "tracked!: i64"
           FROM transactions t
           JOIN recurring_rules r ON r.id = t.rule_id
           WHERE t.rule_id IN (SELECT value FROM json_each(?1))
             AND t.period BETWEEN ?2 AND ?3
             AND t.deleted_at IS NULL"#,
        rules,
        first_s,
        last_s,
    )
    .fetch_all(pool)
    .await?;
    for r in rows {
        let Some(period) = Period::parse(&r.period) else {
            continue;
        };
        out.entry(r.rule_id).or_default().push(Occurrence {
            id: r.id,
            period,
            amount_cents: r.amount_cents,
            manual_paid: r.manual_paid != 0,
            tracked: r.tracked != 0,
        });
    }
    Ok(out)
}

/// A rule's occurrences inside one run's first and last month.
fn in_run<'a>(
    rows: &'a HashMap<i64, Vec<Occurrence>>,
    rule_id: i64,
    run: &[Period],
) -> impl Iterator<Item = &'a Occurrence> {
    let bounds = run.first().copied().zip(run.last().copied());
    rows.get(&rule_id)
        .into_iter()
        .flatten()
        .filter(move |o| bounds.is_some_and(|(first, last)| first <= o.period && o.period <= last))
}

/// How far a rule's run has come as of `at` (`ui::row::Progress`).
///
/// **Paid** counts the run's months up to and including `at` whose
/// occurrence is confirmed — so a month missed earlier shows as a gap on every
/// row after it. **To go** is everything else in the run, each month at its
/// own row's amount, else its bill code's (a bill covering several months
/// knows every month's figure), else the rule's, else the newest amount the
/// run has seen — what carrying the amount forward would seed it with. The
/// last two are a projection, and say so. With none of them, what is left is
/// unknown (`None`), never `R$ 0,00`: those months are not free.
///
/// `rows` are the rule's occurrences inside the run, `codes` its bill codes'
/// amounts, and `files` the vault's answer for the pane.
fn progress_from(
    run: &[Period],
    at: Period,
    rows: &[&Occurrence],
    codes: Option<&HashMap<Period, Cents>>,
    fallback: Option<Cents>,
    files: &HashMap<i64, FilePresence>,
) -> Progress {
    if run.is_empty() {
        return Progress::default();
    }
    let fallback = fallback.or_else(|| {
        rows.iter()
            .filter(|r| r.amount_cents.is_some())
            .max_by_key(|r| r.period)
            .and_then(|r| r.amount_cents)
            .map(Cents)
    });

    let mut p = Progress {
        total: run.len() as u16,
        to_go_cents: Some(Cents::ZERO),
        ..Progress::default()
    };
    for period in run {
        let row = rows.iter().find(|r| r.period == *period);
        let amount = row
            .and_then(|r| r.amount_cents)
            .map(Cents)
            .or_else(|| codes.and_then(|c| c.get(period)).copied());
        let confirmed = match row {
            Some(r) if *period <= at => r.confirmed(files),
            _ => false,
        };
        if confirmed {
            p.paid += 1;
            p.paid_cents += amount.unwrap_or(Cents::ZERO);
        } else {
            let a = amount.or_else(|| {
                p.estimated = true;
                fallback
            });
            p.to_go_cents = match (p.to_go_cents, a) {
                (Some(t), Some(a)) => Some(t + a),
                _ => None,
            };
        }
    }
    p
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

    // The current month is where the person already is; the badge points at
    // the ones they are not looking at.
    let rows: Vec<(i64, Period)> = rows
        .into_iter()
        .filter_map(|r| Some((r.id, Period::parse(&r.period)?)))
        .filter(|(_, period)| *period != current)
        .collect();

    // The disk decides, here as everywhere: a receipt on disk resolves it,
    // whatever the database thinks. The path is `slot_of`'s, as everywhere,
    // and the vault is asked once for the whole year. With no vault to
    // consult nothing is resolved, which is the honest answer rather than a
    // convenient one.
    let ids: Vec<i64> = rows.iter().map(|(id, _)| *id).collect();
    let files = vault_files(pool, root, &ids).await;

    let mut months: Vec<Unresolved> = Vec::new();
    for (id, period) in rows {
        // Every row here is tracked (the query says so), so only the disk can
        // resolve it — there is no manual tick to consult.
        if confirmed(true, false, id, &files) {
            continue;
        }
        match months.last_mut() {
            Some(m) if m.period == period => m.count += 1,
            _ => months.push(Unresolved { period, count: 1 }),
        }
    }
    Ok(months)
}
