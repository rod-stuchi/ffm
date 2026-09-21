//! Dummy rows for the sandbox.
//!
//! The Rust analogue of the mockup's `T({...})` factory: one [`base`] row plus
//! struct-update syntax per specimen, so adding a field to `LedgerRow` does not
//! mean editing twenty fixtures.
//!
//! **These are pure functions of a [`SandboxCtx`]. No pool, no filesystem, no
//! clock.** That is the whole value of the sandbox — the moment a migration is
//! needed to look at a row, iterating on CSS stops being cheap. If a template
//! cannot be rendered from a fixture struct, that is a signal the template is
//! doing a query's job.
//!
//! The household content — `Água · Sabesp`, `Mesada`, `Energia · Enel SP` —
//! stays Brazilian-household Portuguese on purpose. It is content a household
//! typed, not app copy, so it must never reach a `.ftl` file. A test asserts it
//! does not.

use crate::domain::money::Cents;
use crate::domain::period::Period;
use crate::domain::status::{EditKind, RowFlags, RowState, TxKind, Visibility};
use crate::ui::row::{FilePresence, Gutter, LedgerRow, RowCtx};
use chrono::NaiveDate;

/// The mockup's own "today" — 07/09/2026.
///
/// Pinned rather than read from the clock, because half the row states are
/// functions of the current date: `scheduled overdue`, a `pending` ring at 75%,
/// income past its expected day, a barcode page outside its window. Reading the
/// real clock would make the gallery render differently every day and leave
/// some states unreachable in some months.
pub const SANDBOX_TODAY: (i32, u32, u32) = (2026, 9, 7);

pub fn default_today() -> NaiveDate {
    let (y, m, d) = SANDBOX_TODAY;
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

/// A tracked spend in the current period, due on the 15th, nothing filed yet.
pub fn base(ctx: RowCtx) -> LedgerRow {
    LedgerRow {
        id: 1,
        rule_id: Some(10),
        period: Period::new(2026, 9).unwrap(),
        kind: TxKind::Out,
        title: "Energia · Enel SP".into(),
        amount_cents: Some(Cents(34362)),
        date: None,
        due_date: Some(d(2026, 9, 15)),
        visibility: Visibility::Household,
        bucket: Some("Consumption".into()),
        bill_name: Some("Power".into()),
        manual_paid: false,
        tracked: true,
        instance: None,
        total_installments: None,
        spans_period: false,
        window_size: None,
        window_start: None,
        amount_in_filename: false,
        varies: false,
        member_uuid: "a1b2c3".into(),
        member_slug: "rodrigo".into(),
        prev_amount_cents: None,
        estimate_cents: None,
        installments_paid: None,
        files: FilePresence::default(),
        state: RowState::Ok,
        flags: RowFlags::default(),
        gutter: Gutter::Relative(2),
        ctx,
    }
}

/// One specimen: the row, and the prose explaining why it looks that way.
pub struct Spec {
    pub id: &'static str,
    pub heading: &'static str,
    /// Lifted from `design/ffm-prototype.html`'s SPECS gallery where one
    /// exists, so the annotated justification survives the port instead of
    /// being stranded in a throwaway HTML file.
    pub why: &'static str,
    pub build: fn(RowCtx) -> LedgerRow,
}

pub fn specs() -> Vec<Spec> {
    vec![
        Spec {
            id: "pending",
            heading: "Pending, time still running",
            why: "Neither a scheduled file nor a receipt exists yet. The ring is elapsed time \
                  between the period start and the due date — three-quarters amber reads as \
                  \"still room, but not much\" with no legend. It fills <code>green → amber → \
                  red</code> as the date closes in.",
            build: |ctx| base(ctx),
        },
        Spec {
            id: "bill-only",
            heading: "The bill arrived, nothing paid",
            why: "A barcode file on disk and nothing else. The bill is <em>informational</em> — \
                  it is proof of what is owed, not of anything happening — so it reaches the \
                  file rail and never the status column. This row is still <code>pending</code>. \
                  It is the single easiest rule to break while porting.",
            build: |ctx| LedgerRow {
                files: FilePresence {
                    bill: Some("pdf".into()),
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "scheduled",
            heading: "Scheduled, not yet due",
            why: "A <code>_scheduled</code> file exists — proof a payment was set up ahead of \
                  the due date. It is optional: a same-day payment out of the balance skips \
                  straight to the receipt, with no scheduling step to record.",
            build: |ctx| LedgerRow {
                files: FilePresence {
                    bill: Some("pdf".into()),
                    scheduled: Some("pdf".into()),
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "scheduled-overdue",
            heading: "Scheduled, now overdue",
            why: "The due date passed and no receipt landed — a promise that was never \
                  confirmed kept. This is a distinct warning state, not the same grey \
                  <code>pending</code> ring: \"nothing has happened yet\" and \"something was \
                  promised and did not happen\" are different news.",
            build: |ctx| LedgerRow {
                due_date: Some(d(2026, 8, 18)),
                period: Period::new(2026, 8).unwrap(),
                title: "Água · Sabesp".into(),
                amount_cents: Some(Cents(10840)),
                files: FilePresence {
                    bill: Some("pdf".into()),
                    scheduled: Some("pdf".into()),
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "paid",
            heading: "Paid, receipt confirmed",
            why: "The receipt exists at the computed path. Solid green mark. This is the only \
                  way a tracked row becomes paid — there is no manual tick, because the whole \
                  point of opting into tracking is that a checkbox stops being sufficient \
                  evidence.",
            build: |ctx| LedgerRow {
                date: Some(d(2026, 9, 11)),
                prev_amount_cents: Some(Cents(28140)),
                files: FilePresence {
                    bill: Some("pdf".into()),
                    receipt: Some("pdf".into()),
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "plain",
            heading: "Untracked recurring",
            why: "Tracking opted out. There is no filesystem proof either way, so the row \
                  carries no paid/unpaid state at all — just the recurrence glyph and a hollow \
                  mark.",
            build: |ctx| LedgerRow {
                title: "Netflix".into(),
                amount_cents: Some(Cents(5990)),
                date: Some(d(2026, 9, 21)),
                due_date: None,
                tracked: false,
                bucket: Some("Subscriptions".into()),
                bill_name: Some("Netflix".into()),
                ..base(ctx)
            },
        },
        Spec {
            id: "manual-paid",
            heading: "Manually marked paid",
            why: "The one manual tick in the whole model, and only because there is no \
                  filesystem proof to contradict it. Outlined green disc — deliberately not the \
                  solid fill of a file-verified receipt, so asserted and proven never share a \
                  mark.",
            build: |ctx| LedgerRow {
                title: "Spotify Família".into(),
                amount_cents: Some(Cents(3490)),
                date: Some(d(2026, 9, 22)),
                due_date: None,
                tracked: false,
                manual_paid: true,
                bucket: Some("Subscriptions".into()),
                bill_name: Some("Spotify".into()),
                ..base(ctx)
            },
        },
        Spec {
            id: "no-amount",
            heading: "Tracked, amount not known yet",
            why: "A rule that does not carry its amount forward starts each period with no \
                  amount at all, rather than repeating a number that is wrong every month — a \
                  credit-card statement, say. The placeholder is itself a money format, so it \
                  follows the household's currency.",
            build: |ctx| LedgerRow {
                title: "Cartão · Nubank".into(),
                amount_cents: None,
                ..base(ctx)
            },
        },
        Spec {
            id: "installments",
            heading: "Fixed-length installment",
            why: "A car loan is not a subscription — it has a real end. \
                  <code>total_installments</code> counts it down as a bare <code>7/12</code> \
                  badge in the meta slot, not baked into the title. Distinct from a seasonal \
                  bill, which recurs every year rather than ending for good.",
            build: |ctx| LedgerRow {
                title: "Financiamento · Honda Biz".into(),
                amount_cents: Some(Cents(68700)),
                total_installments: Some(12),
                installments_paid: Some(7),
                tracked: false,
                bucket: Some("Transporte".into()),
                bill_name: Some("Financiamento".into()),
                ..base(ctx)
            },
        },
        Spec {
            id: "barcode-window",
            heading: "One barcode file, several months",
            why: "Some bills arrive as a single document with twelve or thirty-six barcodes, \
                  one page per month. The page is <em>derived</em> from the window's start and \
                  this occurrence's period — never stored per row — and shows on the bill \
                  control as <code>p.9</code>.",
            build: |ctx| LedgerRow {
                spans_period: true,
                window_start: Some(Period::new(2026, 1).unwrap()),
                window_size: Some(12),
                files: FilePresence {
                    bill: Some("pdf".into()),
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "barcode-outside",
            heading: "Barcode page outside the window",
            why: "The uploaded file ran out: page 9 of a six-page document. Flagged on the file \
                  control rather than silently wrong, through the same warning channel as \
                  scheduled-overdue — not a new colour meaning. A fresh upload is needed.",
            build: |ctx| LedgerRow {
                spans_period: true,
                window_start: Some(Period::new(2026, 1).unwrap()),
                window_size: Some(6),
                files: FilePresence {
                    bill: Some("pdf".into()),
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "expected",
            heading: "Income, expected",
            why: "Income runs on its own axis: arrival, not payment. It is \
                  <code>expected</code> until it lands, and never shows <code>paid</code>, \
                  <code>scheduled</code> or <code>scheduled overdue</code>. The ring is the \
                  same elapsed-time arc, counting toward the rule's expected day.",
            build: |ctx| LedgerRow {
                kind: TxKind::In,
                title: "Salário · Contabilizei".into(),
                amount_cents: Some(Cents(1_180_000)),
                due_date: Some(d(2026, 9, 20)),
                bucket: Some("Salário".into()),
                bill_name: Some("Contabilizei".into()),
                ..base(ctx)
            },
        },
        Spec {
            id: "received",
            heading: "Income, received",
            why: "Confirmed by its payslip on disk — the identical <code>exists()</code> check \
                  a bill's receipt uses, so confirmation needed no new verb. The delta inverts \
                  for income: the arrow still means direction, but more money arriving is good \
                  news.",
            build: |ctx| LedgerRow {
                kind: TxKind::In,
                title: "Salário · Contabilizei".into(),
                amount_cents: Some(Cents(1_180_000)),
                prev_amount_cents: Some(Cents(1_100_000)),
                date: Some(d(2026, 9, 5)),
                due_date: Some(d(2026, 9, 5)),
                bucket: Some("Salário".into()),
                bill_name: Some("Contabilizei".into()),
                files: FilePresence {
                    receipt: Some("pdf".into()),
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "estimated",
            heading: "Income, projected from history",
            why: "A commission cannot carry its own number forward, so until the period is \
                  confirmed the row shows an estimate from the last N confirmed periods — \
                  lowest by default, because over-estimating income produces a shortfall. \
                  Computed on read, never stored, so it never writes a spurious \
                  \"amount changed\" entry into the change log.",
            build: |ctx| LedgerRow {
                kind: TxKind::In,
                title: "Freelance · Auditoria".into(),
                amount_cents: None,
                estimate_cents: Some(Cents(413_333)),
                varies: true,
                due_date: Some(d(2026, 9, 25)),
                bucket: Some("Freelance".into()),
                bill_name: Some("Auditoria".into()),
                ..base(ctx)
            },
        },
        Spec {
            id: "delta-up",
            heading: "Month-over-month delta",
            why: "Derived against the same line in the previous month, never stored — a stored \
                  delta goes stale the instant either month is edited. Red up, green down for a \
                  bill; income inverts the colour while the arrow keeps meaning direction.",
            build: |ctx| LedgerRow {
                date: Some(d(2026, 9, 11)),
                prev_amount_cents: Some(Cents(28140)),
                files: FilePresence {
                    receipt: Some("pdf".into()),
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "focused",
            heading: "Focused row",
            why: "The cursor: accent left rail, gutter <code>0</code>, tinted ground. Focus and \
                  selection use different channels on purpose — one is \"where I am\", the other \
                  is \"what I picked\". The file rail also reveals its labels here.",
            build: |ctx| LedgerRow {
                gutter: Gutter::Relative(0),
                flags: RowFlags::focused(),
                files: FilePresence {
                    bill: Some("pdf".into()),
                    receipt: Some("pdf".into()),
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "selected",
            heading: "Selected",
            why: "<code>x</code> marks the row for a batch operation. It is a marker, never a \
                  delete — which is exactly why it gets its own <code>--info</code> inset bar \
                  rather than borrowing the accent or a warning colour.",
            build: |ctx| LedgerRow {
                flags: RowFlags {
                    selected: true,
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "private",
            heading: "Private",
            why: "Hidden from other members, never from the admin. Recurring and private are \
                  always both rendered, dimmed when off, so the row's width does not jump as \
                  rows scroll past.",
            build: |ctx| LedgerRow {
                title: "Consulta · dermatologista".into(),
                amount_cents: Some(Cents(45000)),
                visibility: Visibility::Private,
                tracked: false,
                date: Some(d(2026, 9, 23)),
                due_date: None,
                ..base(ctx)
            },
        },
        Spec {
            id: "deleted",
            heading: "Soft-deleted",
            why: "Dimmed and struck through, with the row retained. <code>u</code> clears the \
                  mark on this same row — restore happens in place and does not insert a \
                  second, superseding row, because that left two ledger rows for one \
                  transaction. The history lives in the change log instead.",
            build: |ctx| LedgerRow {
                flags: RowFlags {
                    deleted: true,
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "draft",
            heading: "Parked draft",
            why: "Leaving a row mid-edit collapses it back to read-only; an unchanged draft \
                  vanishes, a changed one stays marked until saved or discarded, and several \
                  can be parked at once. A draft is unsubmitted form values — visual state — so \
                  it lives in Alpine and never on the server.",
            build: |ctx| LedgerRow {
                flags: RowFlags {
                    draft: Some(EditKind::Occurrence),
                    ..Default::default()
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "placeholder",
            heading: "New-rule placeholder",
            why: "<code>A</code> and <code>I</code> park a dashed row at the cursor for the rule \
                  pane's arrow to point at. Create turns it into the real row in that spot; \
                  Cancel removes it.",
            build: |ctx| LedgerRow {
                title: "New recurring bill".into(),
                amount_cents: None,
                due_date: None,
                state: RowState::Placeholder,
                files: FilePresence::default(),
                ..base(ctx)
            },
        },
        Spec {
            id: "skeleton",
            heading: "Optimistic skeleton",
            why: "Submitted and in flight. The skeleton's geometry matches the real row exactly, \
                  so nothing shifts when the server's fragment lands.",
            build: |ctx| LedgerRow {
                state: RowState::Skeleton,
                ..base(ctx)
            },
        },
        Spec {
            id: "error",
            heading: "Failed, retryable",
            why: "The server said no. The copy says what happened and that nothing was written — \
                  no apologies — and <b>Try Again</b> resubmits the original action.",
            build: |ctx| LedgerRow {
                state: RowState::Error {
                    message: "Could not save “Energia · Enel SP”".into(),
                },
                ..base(ctx)
            },
        },
        Spec {
            id: "nonu",
            heading: "Gutter off",
            why: "<code>:set nonu</code> narrows the gutter track to 10px and empties the cell. \
                  It never removes it — <code>display:none</code> on a grid child reflows its \
                  siblings, which pulled the row's content into the first track and crushed it.",
            build: |ctx| LedgerRow {
                gutter: Gutter::Hidden,
                ..base(ctx)
            },
        },
    ]
}

/// Enough rows to read as a real pane rather than a list of specimens.
pub fn ledger(ctx: RowCtx) -> Vec<LedgerRow> {
    let all = specs();
    [
        "received",
        "paid",
        "scheduled-overdue",
        "focused",
        "installments",
        "plain",
        "estimated",
    ]
    .into_iter()
    .filter_map(|id| all.iter().find(|s| s.id == id))
    .enumerate()
    .map(|(i, s)| {
        let mut row = (s.build)(ctx);
        row.gutter = if row.flags.focused {
            Gutter::Relative(0)
        } else {
            Gutter::Relative(i as i32)
        };
        row
    })
    .collect()
}
