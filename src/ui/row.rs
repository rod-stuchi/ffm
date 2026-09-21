//! `LedgerRow` — the densest component in the app, and the one everything else
//! renders inside.
//!
//! **Every visual channel is a pure function of fields on this struct.** That
//! is what lets the sandbox render all twenty-odd states with no database, no
//! filesystem and no clock — and it is why the query layer hangs
//! `prev_amount_cents`, `estimate_cents`, `installments_paid` and file presence
//! onto the row rather than leaving methods to go and fetch them. The mockup
//! cheats here twice (`estimateFor` walks other months, `barcodePageFor` reads
//! global config); this does not.
//!
//! Derived, never stored: the MoM delta, the three file paths, the barcode page
//! number, the income estimate, paid/scheduled (the disk decides), the
//! installment counter, and ring progress.

use crate::domain::money::{Cents, Currency};
use crate::domain::period::Period;
use crate::domain::slug::slugify;
use crate::domain::status::{RowFlags, RowState, RowStatus, TxKind, Visibility};
use crate::ui::fmt;
use crate::ui::i18n::Catalog;
use chrono::NaiveDate;

// ---------------------------------------------------------------------------
// supporting types
// ---------------------------------------------------------------------------

/// The status mark: one 26px glyph, the biggest thing on the row.
#[derive(Clone, PartialEq, Debug)]
pub enum Mark {
    /// Filled disc — proven. `paid`, `received`.
    Disc { tone: Tone, glyph: &'static str },
    /// Outlined disc — asserted, not proven. `manual paid`.
    Outline { tone: Tone, glyph: &'static str },
    /// The elapsed-time ring, optionally with a glyph at its centre.
    Ring {
        progress: f32,
        tone: Tone,
        glyph: Option<&'static str>,
    },
    /// Hollow dot — untracked, no claim either way.
    Dot { filled: bool, tone: Tone },
    /// In flight.
    Skeleton,
    /// The server said no.
    Error,
}

/// Which semantic colour a mark uses. Deliberately not the accent: the accent
/// means "you are here", and a money fact must never borrow it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tone {
    Pos,
    Neg,
    Info,
    Accent,
    Muted,
}

impl Tone {
    pub fn css(self) -> &'static str {
        match self {
            Tone::Pos => "pos",
            Tone::Neg => "neg",
            Tone::Info => "info",
            Tone::Accent => "accent",
            Tone::Muted => "muted",
        }
    }
}

/// The three file kinds, with numbering fixed by kind and never by position.
/// `g1` means "the bill" on every row in the app, which is what makes it
/// learnable; renumbering by what happens to exist would break that.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FileKind {
    Bill,
    Scheduled,
    Receipt,
}

impl FileKind {
    pub const ALL: [FileKind; 3] = [FileKind::Bill, FileKind::Scheduled, FileKind::Receipt];

    /// The keyboard number, and the `<kbd>` shown on the control.
    pub fn number(self) -> u8 {
        match self {
            FileKind::Bill => 1,
            FileKind::Scheduled => 2,
            FileKind::Receipt => 3,
        }
    }

    /// The filename suffix before the extension. The receipt has none — its
    /// bare presence is what `paid` means.
    pub fn suffix(self) -> &'static str {
        match self {
            FileKind::Bill => "_barcode",
            FileKind::Scheduled => "_scheduled",
            FileKind::Receipt => "",
        }
    }
}

/// One control in the file rail.
#[derive(Clone, PartialEq, Debug)]
pub struct FileControl {
    pub kind: FileKind,
    pub number: u8,
    /// `Bill`/`Invoice`, `Scheduled`, `Receipt`/`Payslip` — the label changes
    /// by direction, the number never does.
    pub label: &'static str,
    pub path: String,
    /// `Bill · p.4` when the rule's barcode spans several periods.
    pub page: Option<BarcodePage>,
}

/// Which page of a multi-period barcode file this occurrence is.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct BarcodePage {
    pub page: i32,
    pub total: u16,
    /// False once the window has been outgrown — flagged, never silently wrong.
    pub in_window: bool,
}

/// The month-over-month delta. The arrow is direction; the colour is whether
/// that direction is good news, which inverts for income.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Delta {
    pub pct: i32,
    pub up: bool,
    pub good: bool,
}

/// An amount ready to render.
#[derive(Clone, PartialEq, Debug)]
pub struct Amount {
    pub text: String,
    pub is_placeholder: bool,
    pub is_estimated: bool,
    pub is_income: bool,
}

/// The line-number gutter. Its cell always stays in the grid — collapsing it
/// with `display:none` pulls the row's content into the first track and crushes
/// it, which is a bug this codebase has already paid for once.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Gutter {
    Relative(i32),
    Absolute(u32),
    /// `nonu` — the member turned line numbers off household-wide for
    /// themselves. The cell stays in the grid and only its track narrows.
    Hidden,
    /// No number to show, but the row must keep the ledger's geometry — a
    /// deleted row re-rendered on its own, which no longer has a position in
    /// the pane query. Narrowing its track instead would make the row jump
    /// sideways the instant it is deleted, which is the same family of bug as
    /// `display: none` on a grid child.
    Blank,
}

impl Gutter {
    pub fn text(self) -> String {
        match self {
            Gutter::Relative(0) => "0".into(),
            Gutter::Relative(n) => n.abs().to_string(),
            Gutter::Absolute(n) => n.to_string(),
            Gutter::Hidden | Gutter::Blank => String::new(),
        }
    }
}

/// Settings that apply to every row in a render, passed once rather than
/// copied onto each row.
#[derive(Copy, Clone, Debug)]
pub struct RowCtx {
    pub today: NaiveDate,
    pub currency: Currency,
    /// `household_settings.ledger_show_recurring` — "we don't use this concept
    /// here" removes the glyph household-wide.
    pub show_recurring: bool,
    pub show_delta: bool,
    /// Narrow screens keep the file rail icon-only regardless of focus.
    pub narrow: bool,
}

// ---------------------------------------------------------------------------
// the row
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct LedgerRow {
    pub id: i64,
    pub rule_id: Option<i64>,
    pub period: Period,

    pub kind: TxKind,
    pub title: String,
    pub amount_cents: Option<Cents>,
    pub date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    pub visibility: Visibility,
    pub bucket: Option<String>,
    pub bill_name: Option<String>,
    pub manual_paid: bool,

    // --- rule facts, joined on and read-only here -------------------------
    pub tracked: bool,
    pub instance: Option<String>,
    pub total_installments: Option<u16>,
    pub spans_period: bool,
    pub window_size: Option<u16>,
    pub window_start: Option<Period>,
    pub amount_in_filename: bool,
    /// Income whose amount is projected rather than carried forward.
    pub varies: bool,

    // --- member facts needed to compute a path ----------------------------
    pub member_uuid: String,
    pub member_slug: String,

    // --- computed by the QUERY, not by this struct ------------------------
    /// The same line in the previous period. The delta's only input.
    pub prev_amount_cents: Option<Cents>,
    /// A projection from the last N confirmed periods. Computed on read so it
    /// follows new history instead of freezing an old guess — and so it never
    /// writes a spurious "amount changed" entry into the change log.
    pub estimate_cents: Option<Cents>,
    /// Periods elapsed for this rule, for the `7/12` badge.
    pub installments_paid: Option<u16>,
    /// Whether each file exists on disk, and with which extension.
    pub files: FilePresence,

    // --- view -------------------------------------------------------------
    pub state: RowState,
    pub flags: RowFlags,
    pub gutter: Gutter,
    pub ctx: RowCtx,
}

/// What the disk says. `None` means no such file; the extension matters because
/// the computed path ends in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePresence {
    pub bill: Option<String>,
    pub scheduled: Option<String>,
    pub receipt: Option<String>,
}

impl FilePresence {
    pub fn get(&self, kind: FileKind) -> Option<&str> {
        match kind {
            FileKind::Bill => self.bill.as_deref(),
            FileKind::Scheduled => self.scheduled.as_deref(),
            FileKind::Receipt => self.receipt.as_deref(),
        }
    }
}

impl LedgerRow {
    // -- status ----------------------------------------------------------

    /// The one mutually-exclusive fact.
    ///
    /// The ordering is load-bearing — it is the mockup's `markHTML` fall-through
    /// chain. Note what is **absent**: the bill/barcode file appears nowhere.
    /// It never touches the status column; it only reaches the file rail. That
    /// is the single easiest rule to break while porting.
    pub fn status(&self) -> RowStatus {
        if self.kind.is_income() {
            return if self.is_confirmed() {
                RowStatus::Received
            } else {
                RowStatus::Expected
            };
        }
        if !self.tracked {
            return if self.manual_paid {
                RowStatus::ManualPaid
            } else {
                RowStatus::Plain
            };
        }
        if self.files.receipt.is_some() {
            return RowStatus::Paid;
        }
        if self.files.scheduled.is_some() {
            return if self.is_past_due() {
                RowStatus::ScheduledOverdue
            } else {
                RowStatus::Scheduled
            };
        }
        RowStatus::Pending
    }

    /// Confirmed by evidence: a file on disk for a tracked row, the manual tick
    /// for an untracked one. Never both — a tracked rule has no checkbox.
    pub fn is_confirmed(&self) -> bool {
        if self.tracked {
            self.files.receipt.is_some()
        } else {
            self.manual_paid
        }
    }

    pub fn is_past_due(&self) -> bool {
        self.due_date.is_some_and(|d| self.ctx.today > d)
    }

    /// Feeds the month-picker's warning dot.
    pub fn is_unresolved(&self) -> bool {
        if !self.tracked || self.flags.deleted {
            return false;
        }
        self.status().is_unresolved(self.is_past_due())
    }

    // -- mark ------------------------------------------------------------

    pub fn mark(&self) -> Mark {
        match &self.state {
            RowState::Skeleton => return Mark::Skeleton,
            RowState::Error { .. } => return Mark::Error,
            RowState::Placeholder => {
                return Mark::Dot {
                    filled: false,
                    tone: Tone::Muted,
                };
            }
            RowState::Ok => {}
        }

        match self.status() {
            RowStatus::Paid => Mark::Disc {
                tone: Tone::Pos,
                glyph: "check",
            },
            RowStatus::Received => Mark::Disc {
                tone: Tone::Pos,
                glyph: "check",
            },
            // Outlined, not filled: asserted rather than proven. The two must
            // never share a mark.
            RowStatus::ManualPaid => Mark::Outline {
                tone: Tone::Pos,
                glyph: "check",
            },
            RowStatus::Scheduled => Mark::Ring {
                progress: self.ring_progress().unwrap_or(0.0),
                tone: Tone::Info,
                glyph: Some("clock"),
            },
            RowStatus::ScheduledOverdue => Mark::Disc {
                tone: Tone::Neg,
                glyph: "alert",
            },
            RowStatus::Pending | RowStatus::Expected => Mark::Ring {
                progress: self.ring_progress().unwrap_or(0.0),
                tone: self.ring_tone(),
                glyph: None,
            },
            RowStatus::Plain => Mark::Dot {
                filled: false,
                tone: Tone::Muted,
            },
        }
    }

    /// Elapsed time from the period's first day to the due date — not a
    /// countdown. Three-quarters amber reads as "still room, but not much"
    /// without a legend to explain it.
    pub fn ring_progress(&self) -> Option<f32> {
        let due = self.due_date?;
        Some(self.period.elapsed_toward(due, self.ctx.today))
    }

    fn ring_tone(&self) -> Tone {
        match self.ring_progress() {
            Some(p) if p >= 1.0 => Tone::Neg,
            Some(p) if p >= 0.66 => Tone::Accent,
            _ => Tone::Pos,
        }
    }

    // -- amount ----------------------------------------------------------

    /// Income that varies and has not been confirmed shows a projection.
    pub fn is_estimated(&self) -> bool {
        self.kind.is_income()
            && self.varies
            && !self.is_confirmed()
            && self.estimate_cents.is_some()
    }

    /// What this row is actually worth right now: the stored amount, or the
    /// projection when there isn't one yet.
    pub fn effective_cents(&self) -> Option<Cents> {
        if self.is_estimated() {
            self.estimate_cents
        } else {
            self.amount_cents
        }
    }

    pub fn amount(&self) -> Amount {
        let currency = self.ctx.currency;
        let income = self.kind.is_income();

        match self.effective_cents() {
            None => Amount {
                text: fmt::money_placeholder(currency),
                is_placeholder: true,
                is_estimated: false,
                is_income: income,
            },
            Some(c) if self.is_estimated() => Amount {
                text: fmt::money_estimated(c, currency),
                is_placeholder: false,
                is_estimated: true,
                is_income: income,
            },
            Some(c) => Amount {
                text: fmt::money(c, currency),
                is_placeholder: false,
                is_estimated: false,
                is_income: income,
            },
        }
    }

    // -- delta -----------------------------------------------------------

    /// Red up, green down — for a bill. Income inverts: more money arriving is
    /// good news, so the arrow keeps meaning direction while the colour flips.
    ///
    /// Never compares a projection to a fact.
    pub fn delta(&self) -> Option<Delta> {
        if !self.ctx.show_delta || self.is_estimated() {
            return None;
        }
        let pct = self.effective_cents()?.delta_pct(self.prev_amount_cents?)?;
        let up = pct > 0;
        Some(Delta {
            pct,
            up,
            good: if self.kind.is_income() { up } else { !up },
        })
    }

    // -- installments ----------------------------------------------------

    /// The bare `7/12` badge. Not baked into the title — it lives in the meta
    /// slot beside the due date and the recurrence glyph.
    pub fn installments(&self) -> Option<(u16, u16)> {
        Some((self.installments_paid?, self.total_installments?))
    }

    // -- files -----------------------------------------------------------

    /// `{uuid}-{slug}/{bucket}/{bill_name}[/{instance}]/{year}/{month}...`
    ///
    /// Fully computed, never authored and never stored — which is exactly why
    /// renaming a rule's taxonomy has to *move* the files rather than just
    /// relabel them.
    pub fn file_path(&self, kind: FileKind) -> Option<String> {
        let ext = self.files.get(kind)?;
        let bucket = slugify(self.bucket.as_deref()?);
        let bill = slugify(self.bill_name.as_deref()?);

        let mut path = format!(
            "{}-{}/{}/{}",
            self.member_uuid, self.member_slug, bucket, bill
        );
        if let Some(instance) = self.instance.as_deref() {
            path.push('/');
            path.push_str(&slugify(instance));
        }
        path.push('/');
        path.push_str(&self.period.path_year());
        path.push('/');
        path.push_str(&self.period.path_month());

        // `09_brl122.94.pdf`. Only the definitive receipt carries the amount —
        // and only when a real value is known, so nothing is ever named after a
        // zero nobody owes.
        if kind == FileKind::Receipt
            && self.amount_in_filename
            && let Some(c) = self.amount_cents
        {
            path.push('_');
            path.push_str(&fmt::money_filename(c, self.ctx.currency));
        }
        path.push_str(kind.suffix());
        path.push('.');
        path.push_str(ext);
        Some(path)
    }

    /// Which page of a multi-period barcode document this occurrence is.
    ///
    /// Derived from the window's start and this row's own period — never a
    /// per-occurrence stored number.
    pub fn barcode_page(&self) -> Option<BarcodePage> {
        if !self.spans_period {
            return None;
        }
        let start = self.window_start?;
        let total = self.window_size?;
        let page = start.months_until(self.period) + 1;
        Some(BarcodePage {
            page,
            total,
            in_window: page >= 1 && page <= total as i32,
        })
    }

    /// The rail: only the kinds that exist, in fixed order, with fixed numbers.
    ///
    /// Income has no slot 2 — money arriving is never "scheduled" — so `g2` on
    /// an income row is a no-op with a message rather than a renumbering.
    pub fn file_rail(&self) -> Vec<FileControl> {
        let income = self.kind.is_income();
        FileKind::ALL
            .into_iter()
            .filter(|k| !(income && *k == FileKind::Scheduled))
            .filter_map(|kind| {
                let path = self.file_path(kind)?;
                Some(FileControl {
                    kind,
                    number: kind.number(),
                    label: match (kind, income) {
                        (FileKind::Bill, false) => "Bill",
                        (FileKind::Bill, true) => "Invoice",
                        (FileKind::Scheduled, _) => "Scheduled",
                        (FileKind::Receipt, false) => "Receipt",
                        (FileKind::Receipt, true) => "Payslip",
                    },
                    page: (kind == FileKind::Bill)
                        .then(|| self.barcode_page())
                        .flatten(),
                    path,
                })
            })
            .collect()
    }

    // -- chrome ----------------------------------------------------------

    pub fn is_recurring(&self) -> bool {
        self.rule_id.is_some()
    }

    pub fn is_private(&self) -> bool {
        self.visibility == Visibility::Private
    }

    /// Recurring and private are **always both rendered**, dimmed when off, so
    /// the row's width does not jump as rows scroll past. The household-wide
    /// `show_recurring = false` is the one thing that removes the first one
    /// outright.
    pub fn flag_pair(&self) -> Vec<(&'static str, bool)> {
        let mut out = Vec::with_capacity(2);
        if self.ctx.show_recurring {
            out.push(("recurring", self.is_recurring()));
        }
        out.push(("private", self.is_private()));
        out
    }

    /// The `is-`/`has-` prefixed state classes. Prefixed on purpose: a bare
    /// `.cfg` once collided with a leftover rule and broke the shell width.
    pub fn classes(&self) -> Vec<&'static str> {
        let mut c = vec![];
        if self.flags.focused {
            c.push("is-focus");
        }
        if self.flags.selected {
            c.push("is-selected");
        }
        if self.flags.matched {
            c.push("is-match");
        }
        if self.flags.deleted {
            c.push("is-deleted");
        }
        if self.flags.editing.is_some() {
            c.push("is-editing");
        }
        if self.flags.draft.is_some() {
            c.push("has-draft");
        }
        if self.flags.log_open {
            c.push("is-logopen");
        }
        if matches!(self.state, RowState::Error { .. }) {
            c.push("is-error");
        }
        if matches!(self.state, RowState::Placeholder) {
            c.push("is-placeholder");
        }
        if self.gutter == Gutter::Hidden {
            c.push("is-nogut");
        }
        c
    }
}

// ---------------------------------------------------------------------------
// the render-ready view
// ---------------------------------------------------------------------------

/// Everything a template needs, flattened to primitives.
///
/// Askama can match on an enum, but doing so drags type paths into the markup
/// and puts branching where the spec says it must not be: *"the status mark,
/// delta arrow and meta glyphs are pure functions of fields — put them as
/// methods on the struct, not as logic in the template."* Computing the view
/// here keeps that literally true, and leaves `row.html` with almost no
/// decisions left to make.
pub struct RowView {
    /// The transaction's id, so a fragment can be addressed for an
    /// out-of-band swap without the template knowing where the row sits.
    pub id: i64,
    pub classes: String,
    pub gutter: String,
    pub title: String,

    // mark
    pub mark_kind: &'static str, // disc | outline | ring | dot | skeleton | error
    pub mark_tone: &'static str,
    pub mark_glyph: &'static str,
    pub mark_inner: Option<&'static str>,
    /// Pre-computed `stroke-dashoffset` for a 10.5px-radius ring.
    pub ring_offset: f32,

    // meta
    pub meta_text: Option<String>,
    pub meta_tone: &'static str,
    pub installments: Option<String>,
    pub flags: Vec<FlagView>,

    // files
    pub files: Vec<FileView>,

    // amount
    pub amount: String,
    pub amount_income: bool,
    pub amount_estimated: bool,
    pub amount_placeholder: bool,
    pub delta: Option<DeltaView>,

    pub error: Option<String>,
    pub is_skeleton: bool,
}

pub struct FlagView {
    pub icon: &'static str,
    pub svg: &'static str,
    pub on: bool,
}

pub struct FileView {
    pub svg: &'static str,
    pub number: u8,
    pub label: &'static str,
    pub path: String,
    pub page: Option<String>,
    pub warn: bool,
}

pub struct DeltaView {
    pub arrow: &'static str,
    pub text: String,
    pub tone: &'static str,
}

/// Circumference of the ring: 2 * PI * 10.5.
const RING_CIRCUMFERENCE: f32 = 65.973;

impl LedgerRow {
    /// Takes the catalog because the meta line has WORDS in it ("due",
    /// "overdue since"). Money and dates inside it do not — they come from
    /// `fmt`, which takes a currency and never a language. The three axes stay
    /// separate even where they share a line.
    pub fn view(&self, t: &Catalog) -> RowView {
        let status = self.status();
        let amount = self.amount();
        let mark = self.mark();

        let (mark_kind, mark_tone, mark_glyph, mark_inner) = match &mark {
            Mark::Skeleton => ("skeleton", "muted", "", None),
            Mark::Error => ("error", "neg", "alert", None),
            Mark::Disc { tone, glyph } => ("disc", tone.css(), *glyph, None),
            Mark::Outline { tone, glyph } => ("outline", tone.css(), *glyph, None),
            Mark::Ring { tone, glyph, .. } => ("ring", tone.css(), "", *glyph),
            Mark::Dot { tone, filled } => {
                ("dot", if *filled { tone.css() } else { "muted" }, "", None)
            }
        };

        let progress = match &mark {
            Mark::Ring { progress, .. } => *progress,
            _ => 0.0,
        };

        RowView {
            id: self.id,
            classes: self.classes().join(" "),
            gutter: self.gutter.text(),
            title: self.title.clone(),

            mark_kind,
            mark_tone,
            // The mark's glyph is chosen here rather than in the template so
            // an unmapped name can never reach the page as literal text.
            mark_glyph: match mark_glyph {
                "check" => "✓",
                "alert" => "!",
                other => {
                    debug_assert!(other.is_empty(), "unmapped mark glyph: {other}");
                    ""
                }
            },
            mark_inner: mark_inner.map(|_| "◷"),
            ring_offset: RING_CIRCUMFERENCE * (1.0 - progress),

            meta_text: self.meta_text(t),
            meta_tone: match status {
                RowStatus::ScheduledOverdue => "warn",
                RowStatus::Scheduled => "info",
                RowStatus::Paid | RowStatus::Received | RowStatus::ManualPaid => "ok",
                _ => "",
            },
            installments: self.installments().map(|(a, b)| format!("{a}/{b}")),
            flags: self
                .flag_pair()
                .into_iter()
                .map(|(icon, on)| FlagView {
                    icon,
                    svg: crate::ui::icons::svg(icon),
                    on,
                })
                .collect(),

            files: self
                .file_rail()
                .into_iter()
                .map(|c| FileView {
                    svg: crate::ui::icons::svg(match c.kind {
                        FileKind::Bill => "bill",
                        FileKind::Scheduled => "scheduled",
                        FileKind::Receipt => "receipt",
                    }),
                    number: c.number,
                    label: c.label,
                    page: c.page.map(|p| format!("p.{}", p.page)),
                    warn: c.page.is_some_and(|p| !p.in_window),
                    path: c.path,
                })
                .collect(),

            amount: amount.text,
            amount_income: amount.is_income,
            amount_estimated: amount.is_estimated,
            amount_placeholder: amount.is_placeholder,
            delta: self.delta().map(|d| DeltaView {
                arrow: if d.pct == 0 {
                    "—"
                } else if d.up {
                    "▲"
                } else {
                    "▼"
                },
                text: if d.pct == 0 {
                    "0%".to_string()
                } else {
                    format!("{}{}%", if d.up { "+" } else { "" }, d.pct)
                },
                tone: if d.pct == 0 {
                    "is-flat"
                } else if d.good {
                    "is-good"
                } else {
                    "is-bad"
                },
            }),

            error: match &self.state {
                RowState::Error { message } => Some(message.clone()),
                _ => None,
            },
            is_skeleton: matches!(self.state, RowState::Skeleton),
        }
    }

    /// The meta line's short form. The full sentence lives in the `gd` popup —
    /// a row's own status text stays short so the line does not wrap.
    fn meta_text(&self, t: &Catalog) -> Option<String> {
        match self.status() {
            RowStatus::Paid | RowStatus::Received | RowStatus::Plain => self.date.map(fmt::date),
            RowStatus::ScheduledOverdue => {
                self.due_date.map(|d| t.row_overdue_since(&fmt::date(d)))
            }
            RowStatus::Scheduled => self.due_date.map(|d| t.row_scheduled_for(&fmt::date(d))),
            RowStatus::Pending | RowStatus::Expected => {
                self.due_date.map(|d| t.row_due(&fmt::date(d)))
            }
            RowStatus::ManualPaid => Some(t.row_manual_paid().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::status::EditKind;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn ctx() -> RowCtx {
        RowCtx {
            today: d(2026, 9, 7),
            currency: Currency::Brl,
            show_recurring: true,
            show_delta: true,
            narrow: false,
        }
    }

    /// A tracked spend in September 2026, due on the 15th, nothing filed.
    fn base() -> LedgerRow {
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
            gutter: Gutter::Relative(0),
            ctx: ctx(),
        }
    }

    // -- status ----------------------------------------------------------

    #[test]
    fn the_bill_file_never_touches_the_status_column() {
        // The single easiest rule to break while porting: a barcode is
        // informational, so a row with only a bill on disk is still Pending.
        let mut r = base();
        r.files.bill = Some("pdf".into());
        assert_eq!(r.status(), RowStatus::Pending);

        r.files.scheduled = Some("pdf".into());
        assert_eq!(r.status(), RowStatus::Scheduled);

        r.files.receipt = Some("pdf".into());
        assert_eq!(r.status(), RowStatus::Paid);
    }

    #[test]
    fn scheduled_goes_overdue_only_after_the_due_date() {
        let mut r = base();
        r.files.scheduled = Some("pdf".into());

        r.ctx.today = d(2026, 9, 14);
        assert_eq!(r.status(), RowStatus::Scheduled);
        r.ctx.today = d(2026, 9, 15); // due day itself is not yet overdue
        assert_eq!(r.status(), RowStatus::Scheduled);
        r.ctx.today = d(2026, 9, 16);
        assert_eq!(r.status(), RowStatus::ScheduledOverdue);
    }

    #[test]
    fn untracked_rows_use_the_manual_tick_and_nothing_else() {
        let mut r = base();
        r.tracked = false;
        assert_eq!(r.status(), RowStatus::Plain);

        r.manual_paid = true;
        assert_eq!(r.status(), RowStatus::ManualPaid);

        // A tracked rule has no checkbox, so the tick cannot override evidence.
        r.tracked = true;
        assert_eq!(r.status(), RowStatus::Pending);
    }

    #[test]
    fn income_never_reports_a_spend_status() {
        let mut r = base();
        r.kind = TxKind::In;
        assert_eq!(r.status(), RowStatus::Expected);

        // Even with a scheduled file present, which cannot happen for income
        // but must not produce `Scheduled` if it somehow did.
        r.files.scheduled = Some("pdf".into());
        assert_eq!(r.status(), RowStatus::Expected);

        r.files.receipt = Some("pdf".into());
        assert_eq!(r.status(), RowStatus::Received);
        assert!(r.status().is_income());
    }

    #[test]
    fn manual_and_verified_paid_never_share_a_mark() {
        let mut verified = base();
        verified.files.receipt = Some("pdf".into());

        let mut manual = base();
        manual.tracked = false;
        manual.manual_paid = true;

        assert!(matches!(verified.mark(), Mark::Disc { .. }));
        assert!(matches!(manual.mark(), Mark::Outline { .. }));
        assert_ne!(verified.mark(), manual.mark());
    }

    // -- ring ------------------------------------------------------------

    #[test]
    fn the_ring_is_elapsed_time_and_reddens_past_due() {
        let mut r = base();
        r.ctx.today = d(2026, 9, 1);
        assert_eq!(r.ring_progress(), Some(0.0));
        assert_eq!(r.ring_tone(), Tone::Pos);

        r.ctx.today = d(2026, 9, 8); // exactly half of 1st -> 15th
        assert!((r.ring_progress().unwrap() - 0.5).abs() < 0.01);

        r.ctx.today = d(2026, 9, 14);
        assert_eq!(r.ring_tone(), Tone::Accent); // amber: room, but not much

        r.ctx.today = d(2026, 9, 20);
        assert_eq!(r.ring_progress(), Some(1.0));
        assert_eq!(r.ring_tone(), Tone::Neg);
    }

    // -- amount ----------------------------------------------------------

    #[test]
    fn no_amount_yet_renders_the_currency_aware_placeholder() {
        let mut r = base();
        r.amount_cents = None;
        let a = r.amount();
        assert_eq!(a.text, "R$ __,__");
        assert!(a.is_placeholder);

        r.ctx.currency = Currency::Usd;
        assert_eq!(r.amount().text, "$__.__");
    }

    #[test]
    fn varying_income_is_projected_and_marked() {
        let mut r = base();
        r.kind = TxKind::In;
        r.varies = true;
        r.amount_cents = None;
        r.estimate_cents = Some(Cents(413_333));

        let a = r.amount();
        assert!(a.is_estimated);
        assert_eq!(a.text, "~R$ 4.133,33");

        // Once it actually lands, it is a fact and loses the tilde.
        r.files.receipt = Some("pdf".into());
        r.amount_cents = Some(Cents(450_000));
        assert!(!r.amount().is_estimated);
        assert_eq!(r.amount().text, "R$ 4.500,00");
    }

    // -- delta -----------------------------------------------------------

    #[test]
    fn delta_colour_inverts_for_income_but_the_arrow_does_not() {
        let mut bill = base();
        bill.amount_cents = Some(Cents(34362));
        bill.prev_amount_cents = Some(Cents(28140));
        let d1 = bill.delta().unwrap();
        assert!(d1.up && !d1.good, "a bill going up is bad news");

        let mut income = bill.clone();
        income.kind = TxKind::In;
        let d2 = income.delta().unwrap();
        assert!(d2.up && d2.good, "income going up is good news");
        assert_eq!(d1.pct, d2.pct, "the arrow means direction, not sentiment");
    }

    #[test]
    fn a_projection_is_never_compared_to_a_fact() {
        let mut r = base();
        r.kind = TxKind::In;
        r.varies = true;
        r.amount_cents = None;
        r.estimate_cents = Some(Cents(400_000));
        r.prev_amount_cents = Some(Cents(300_000));
        assert!(r.is_estimated());
        assert_eq!(r.delta(), None);
    }

    #[test]
    fn show_delta_off_silences_it() {
        let mut r = base();
        r.prev_amount_cents = Some(Cents(28140));
        assert!(r.delta().is_some());
        r.ctx.show_delta = false;
        assert_eq!(r.delta(), None);
    }

    // -- files -----------------------------------------------------------

    #[test]
    fn the_computed_path_matches_the_spec() {
        let mut r = base();
        r.files.receipt = Some("pdf".into());
        assert_eq!(
            r.file_path(FileKind::Receipt).unwrap(),
            "a1b2c3-rodrigo/consumption/power/2026/09.pdf"
        );

        r.files.bill = Some("jpg".into());
        assert_eq!(
            r.file_path(FileKind::Bill).unwrap(),
            "a1b2c3-rodrigo/consumption/power/2026/09_barcode.jpg"
        );

        r.files.scheduled = Some("pdf".into());
        assert_eq!(
            r.file_path(FileKind::Scheduled).unwrap(),
            "a1b2c3-rodrigo/consumption/power/2026/09_scheduled.pdf"
        );
    }

    #[test]
    fn instance_appears_in_the_path_only_when_the_rule_has_one() {
        let mut r = base();
        r.files.receipt = Some("pdf".into());
        assert!(!r.file_path(FileKind::Receipt).unwrap().contains("house"));

        r.instance = Some("House 8898".into());
        assert_eq!(
            r.file_path(FileKind::Receipt).unwrap(),
            "a1b2c3-rodrigo/consumption/power/house_8898/2026/09.pdf"
        );
    }

    #[test]
    fn the_amount_suffix_is_opt_in_and_needs_a_real_value() {
        let mut r = base();
        r.files.receipt = Some("pdf".into());
        r.amount_in_filename = true;
        assert_eq!(
            r.file_path(FileKind::Receipt).unwrap(),
            "a1b2c3-rodrigo/consumption/power/2026/09_brl343.62.pdf"
        );

        // Nothing is ever named after a zero nobody owes.
        r.amount_cents = None;
        assert_eq!(
            r.file_path(FileKind::Receipt).unwrap(),
            "a1b2c3-rodrigo/consumption/power/2026/09.pdf"
        );

        // And the app's own suffixes keep their plain form.
        r.amount_cents = Some(Cents(34362));
        r.files.bill = Some("pdf".into());
        assert!(
            r.file_path(FileKind::Bill)
                .unwrap()
                .ends_with("09_barcode.pdf")
        );
    }

    #[test]
    fn file_numbers_are_fixed_by_kind_not_by_position() {
        let mut r = base();
        // Bill and receipt, no scheduled: the receipt is still 3.
        r.files.bill = Some("pdf".into());
        r.files.receipt = Some("pdf".into());

        let rail = r.file_rail();
        assert_eq!(rail.len(), 2);
        assert_eq!(rail[0].number, 1);
        assert_eq!(rail[1].number, 3);
        assert_eq!(rail[1].kind, FileKind::Receipt);
    }

    #[test]
    fn income_has_no_scheduled_slot_and_relabels_the_others() {
        let mut r = base();
        r.kind = TxKind::In;
        r.files.bill = Some("pdf".into());
        r.files.scheduled = Some("pdf".into()); // must be ignored
        r.files.receipt = Some("pdf".into());

        let rail = r.file_rail();
        assert_eq!(rail.len(), 2);
        assert_eq!(rail[0].label, "Invoice");
        assert_eq!(rail[1].label, "Payslip");
        assert!(!rail.iter().any(|c| c.kind == FileKind::Scheduled));
    }

    #[test]
    fn a_row_with_no_files_has_no_rail_at_all() {
        // Absent, not a reserved empty box.
        assert!(base().file_rail().is_empty());
    }

    // -- barcode window --------------------------------------------------

    #[test]
    fn barcode_page_derives_from_the_window_start() {
        let mut r = base();
        r.spans_period = true;
        r.window_start = Some(Period::new(2026, 1).unwrap());
        r.window_size = Some(12);

        let p = r.barcode_page().unwrap();
        assert_eq!(p.page, 9); // January is page 1, so September is page 9
        assert!(p.in_window);
    }

    #[test]
    fn an_occurrence_past_the_window_is_flagged_not_silently_wrong() {
        let mut r = base();
        r.spans_period = true;
        r.window_start = Some(Period::new(2026, 1).unwrap());
        r.window_size = Some(6); // only January..June in the file

        let p = r.barcode_page().unwrap();
        assert_eq!(p.page, 9);
        assert!(!p.in_window, "page 9 of a 6-page file must be flagged");
    }

    // -- chrome ----------------------------------------------------------

    #[test]
    fn flags_compose_into_classes() {
        let mut r = base();
        r.flags = RowFlags {
            focused: true,
            selected: true,
            draft: Some(EditKind::Occurrence),
            ..Default::default()
        };
        let c = r.classes();
        assert!(c.contains(&"is-focus") && c.contains(&"is-selected") && c.contains(&"has-draft"));
        assert!(!c.contains(&"is-deleted"));
    }

    #[test]
    fn both_meta_flags_always_render_so_the_row_does_not_jump() {
        let r = base();
        let pair = r.flag_pair();
        assert_eq!(pair.len(), 2);
        assert_eq!(pair[0], ("recurring", true));
        assert_eq!(pair[1], ("private", false)); // present, just dim

        // The household-wide switch is the one thing that removes it outright.
        let mut off = base();
        off.ctx.show_recurring = false;
        assert_eq!(off.flag_pair().len(), 1);
    }

    #[test]
    fn the_gutter_cell_survives_nonu() {
        // Hidden means an empty cell, never a removed track.
        assert_eq!(Gutter::Hidden.text(), "");
        assert_eq!(Gutter::Blank.text(), "");
        assert_eq!(Gutter::Relative(0).text(), "0");
        assert_eq!(Gutter::Relative(-3).text(), "3");
        assert_eq!(Gutter::Absolute(12).text(), "12");

        let mut r = base();
        r.gutter = Gutter::Hidden;
        assert!(r.classes().contains(&"is-nogut"));

        // Blank keeps the track: a deleted row must not jump sideways.
        r.gutter = Gutter::Blank;
        assert!(!r.classes().contains(&"is-nogut"));
    }

    // -- unresolved ------------------------------------------------------

    #[test]
    fn only_tracked_overdue_rows_feed_the_warning_badge() {
        let mut r = base();
        r.ctx.today = d(2026, 9, 20); // past the 15th

        assert!(r.is_unresolved(), "tracked, past due, nothing filed");

        r.files.receipt = Some("pdf".into());
        assert!(!r.is_unresolved(), "paid is not an issue");

        r.files.receipt = None;
        r.tracked = false;
        assert!(!r.is_unresolved(), "untracked has no evidence to act on");

        r.tracked = true;
        r.flags.deleted = true;
        assert!(!r.is_unresolved(), "a deleted row is not outstanding");
    }

    #[test]
    fn state_short_circuits_before_status() {
        let mut r = base();
        r.state = RowState::Skeleton;
        assert_eq!(r.mark(), Mark::Skeleton);
        r.state = RowState::Error {
            message: "server unreachable".into(),
        };
        assert_eq!(r.mark(), Mark::Error);
        assert!(r.classes().contains(&"is-error"));
    }
}
