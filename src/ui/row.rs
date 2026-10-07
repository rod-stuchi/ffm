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
use crate::domain::receipt_path;
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
    /// The elapsed-time ring, optionally with a glyph at its center.
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

/// Which semantic color a mark uses. Deliberately not the accent: the accent
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

    /// How the store names this kind. The rail calls slot 1 a *Bill* (or an
    /// *Invoice*); on disk it is the `_barcode` file. One is what a person
    /// reads, the other is what the filesystem holds, and they are allowed to
    /// differ — but only through this one mapping.
    pub fn storage(self) -> crate::domain::receipt_path::Kind {
        use crate::domain::receipt_path::Kind;
        match self {
            FileKind::Bill => Kind::Barcode,
            FileKind::Scheduled => Kind::Scheduled,
            FileKind::Receipt => Kind::Receipt,
        }
    }

    pub fn of_storage(kind: crate::domain::receipt_path::Kind) -> FileKind {
        use crate::domain::receipt_path::Kind;
        match kind {
            Kind::Barcode => FileKind::Bill,
            Kind::Scheduled => FileKind::Scheduled,
            Kind::Receipt => FileKind::Receipt,
        }
    }
}

/// One control in the file rail.
#[derive(Clone, PartialEq, Debug)]
pub struct FileControl {
    pub kind: FileKind,
    pub number: u8,
    /// No label here. `Bill`/`Invoice`, `Scheduled`, `Receipt`/`Payslip`
    /// changes by direction AND by language, and this struct has no Catalog
    /// — which is exactly how those words ended up hardcoded in English on a
    /// pt-BR ledger. The word is chosen in `view`, where `t` is in scope; the
    /// number, which never changes, stays here.
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

/// The month-over-month delta. The arrow is direction; the color is whether
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

/// What the member chose for the gutter — `member_settings.line_numbers`.
///
/// **Relative is the only one the server cannot fully decide**, because it is
/// measured from the cursor and the cursor is Alpine's. The server still
/// renders it correctly for the first frame: the cursor always starts on the
/// first row, so relative-to-row-0 is the right answer before any key is
/// pressed, and `paint` keeps it right afterwards. That is the same posture
/// `layout = auto` takes — correct before the first paint, no round trip.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum LineNumbers {
    #[default]
    Relative,
    Absolute,
    Off,
}

impl LineNumbers {
    pub fn parse(s: &str) -> LineNumbers {
        match s {
            "absolute" => LineNumbers::Absolute,
            "none" | "off" => LineNumbers::Off,
            _ => LineNumbers::Relative,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            LineNumbers::Relative => "relative",
            LineNumbers::Absolute => "absolute",
            LineNumbers::Off => "none",
        }
    }

    /// The gutter for the row at index `i`, with the cursor at `cursor`.
    pub fn gutter(self, i: usize, cursor: usize) -> Gutter {
        match self {
            LineNumbers::Relative => Gutter::Relative(i as i32 - cursor as i32),
            LineNumbers::Absolute => Gutter::Absolute(i as u32 + 1),
            LineNumbers::Off => Gutter::Hidden,
        }
    }
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
    /// How the pane is ordered. The row itself does not care, but the query
    /// that builds it does, and passing it here keeps "what the member chose"
    /// in one place rather than threading it through every call site.
    pub sort: crate::db::ledger::Sort,
    pub currency: Currency,
    /// `household_settings.ledger_show_recurring` — "we don't use this concept
    /// here" removes the glyph household-wide.
    pub show_recurring: bool,
    pub show_delta: bool,
    /// Narrow screens keep the file rail icon-only regardless of focus.
    pub narrow: bool,
    /// `member_settings.line_numbers`. Was missing, which is why `relative`
    /// and `none` both rendered as `absolute`: the pane query hardcoded the
    /// gutter and never asked what the member had chosen.
    pub line_numbers: LineNumbers,
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
    /// The RULE's bucket and bill name: where a tracked row's files live.
    /// `bucket` / `bill_name` above are the row's own, for grouping — a path
    /// built from those looked in a folder uploads never wrote to, once a
    /// rule's bucket had been renamed.
    pub file_bucket: Option<String>,
    pub file_bill_name: Option<String>,
    pub tracked: bool,
    pub instance: Option<String>,
    pub total_installments: Option<u16>,
    pub spans_period: bool,
    pub window_size: Option<u16>,
    pub window_start: Option<Period>,
    pub amount_in_filename: bool,
    /// Income whose amount is projected rather than carried forward.
    pub varies: bool,
    /// Months between occurrences, for the details popup.
    pub cadence_months: Option<i64>,
    /// The day the rule behind this row was deleted. Its occurrences keep
    /// their files and their tracked status — the files live at the rule's
    /// taxonomy — and only stop claiming to recur.
    pub rule_deleted_on: Option<NaiveDate>,

    // --- member facts needed to compute a path ----------------------------
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
        ring_tone_at(self.ring_progress().unwrap_or(0.0))
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
    /// good news, so the arrow keeps meaning direction while the color flips.
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
        if !self.is_recurring() {
            return None;
        }
        Some((self.installments_paid?, self.total_installments?))
    }

    // -- files -----------------------------------------------------------

    /// `{slug}/{bucket}/{bill_name}[/{instance}]/{year}/{month}...`
    ///
    /// Fully computed, never authored and never stored — which is exactly why
    /// renaming a rule's taxonomy has to *move* the files rather than just
    /// relabel them.
    pub fn file_path(&self, kind: FileKind) -> Option<String> {
        let ext = self.files.get(kind)?;
        // `{slug}`: files live under their owner's folder, so two
        // members using the same bucket name never collide.
        let vault = crate::domain::receipt_path::vault(&self.member_slug);
        let tax = receipt_path::Taxonomy {
            vault: &vault,
            bucket: self.file_bucket.as_deref()?,
            bill_name: self.file_bill_name.as_deref()?,
            instance: self.instance.as_deref(),
        };
        Some(receipt_path::file(
            &tax,
            self.period,
            kind.storage(),
            ext,
            // Only the receipt ever carries it, and only when the rule opted
            // in — the builder enforces both, so the row just hands over what
            // it knows.
            self.amount_in_filename
                .then_some(self.amount_cents)
                .flatten()
                .map(|c| (c, self.ctx.currency)),
        ))
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

    /// The full sentence behind the mark, for `gd`.
    pub fn status_sentence(&self, t: &Catalog) -> String {
        match self.status() {
            RowStatus::Paid => t.status_paid(),
            RowStatus::Scheduled => t.status_scheduled(),
            RowStatus::ScheduledOverdue => t.status_scheduled_overdue(),
            RowStatus::Pending => t.status_pending(),
            RowStatus::Plain => t.status_plain(),
            RowStatus::ManualPaid => t.status_manual_paid(),
            RowStatus::Expected => t.status_expected(),
            RowStatus::Received => t.status_received(),
        }
        .to_string()
    }

    /// Only a recurring row has one, which is why this is an `Option` rather
    /// than a default of "monthly".
    fn cadence_label(&self, _t: &Catalog) -> Option<String> {
        if !self.is_recurring() {
            return None;
        }
        Some(match self.cadence_months {
            Some(1) | None => "1".to_string(),
            Some(n) => n.to_string(),
        })
    }

    /// Where this period's receipt would live, file or no file — "where would
    /// it go" is exactly the question somebody asks when it is missing.
    fn receipt_path_preview(&self) -> Option<String> {
        let vault = crate::domain::receipt_path::vault(&self.member_slug);
        let tax = receipt_path::Taxonomy {
            vault: &vault,
            bucket: self.file_bucket.as_deref()?,
            bill_name: self.file_bill_name.as_deref()?,
            instance: self.instance.as_deref(),
        };
        self.tracked.then(|| {
            receipt_path::file(
                &tax,
                self.period,
                receipt_path::Kind::Receipt,
                "pdf",
                self.amount_in_filename
                    .then_some(self.amount_cents)
                    .flatten()
                    .map(|c| (c, self.ctx.currency)),
            )
        })
    }

    /// The rail: only the kinds that exist, in fixed order, with fixed numbers.
    ///
    /// Income has no slot 2 — money arriving is never "scheduled" — so `g2` on
    /// an income row is a no-op with a message rather than a renumbering.
    /// Whether this row can take a file at all: a tracked row with the
    /// taxonomy its files are filed under, and not a stand-in for one.
    pub fn attachable(&self) -> bool {
        self.tracked
            && self.bucket.is_some()
            && self.bill_name.is_some()
            && !matches!(self.state, RowState::Skeleton | RowState::Placeholder)
    }

    /// The slots an attachable row has no file in, in their fixed order —
    /// what `density = default` draws as `+1`, `+2`, `+3`.
    fn missing_slots(&self, t: &Catalog) -> Vec<SlotView> {
        if !self.attachable() {
            return Vec::new();
        }
        let income = self.kind.is_income();
        let have: Vec<FileKind> = self.file_rail().into_iter().map(|c| c.kind).collect();
        FileKind::ALL
            .into_iter()
            .filter(|k| !have.contains(k) && !(income && *k == FileKind::Scheduled))
            .map(|k| SlotView {
                svg: slot_svg(k),
                number: k.number(),
                label: slot_label(k, income, t),
                kind: k.storage().code(),
            })
            .collect()
    }

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
                    page: (kind == FileKind::Bill)
                        .then(|| self.barcode_page())
                        .flatten(),
                    path,
                })
            })
            .collect()
    }

    // -- chrome ----------------------------------------------------------

    /// A live rule. A deleted one leaves its rows as orphaned bills: they
    /// keep `rule_id` (the log and the files still need it) but nothing about
    /// them recurs any more.
    pub fn is_recurring(&self) -> bool {
        self.rule_id.is_some() && self.rule_deleted_on.is_none()
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
/// What `gd` and the hover tooltip show — the long version of a status the
/// meta line only has room to hint at.
///
/// The row's own text stays short (`overdue since 18/07`) because it is
/// scanned; the sentence belongs next to the Due/Cadence/Receipt fields it
/// explains, rather than growing the meta line into a paragraph every row
/// pays for.
pub struct Details {
    pub due: Option<String>,
    pub cadence: Option<String>,
    pub receipt: Option<String>,
    pub status: String,
    pub recurring: bool,
    /// When the rule behind an orphaned bill was deleted — said in place of
    /// the cadence, which no longer applies.
    pub rule_deleted: Option<String>,
}

pub struct RowView {
    /// The transaction's id, so a fragment can be addressed for an
    /// out-of-band swap without the template knowing where the row sits.
    pub id: i64,
    /// The rule behind this row, if any — what `E` needs, and what makes
    /// "not generated from a recurring rule" a fact the row already knows
    /// rather than a round trip to find out.
    pub rule_id: Option<i64>,
    /// The manual paid tick, `Some(current)` only where it applies: an
    /// untracked recurring row. What `m` and the row menu read before asking
    /// the server, which decides again (`tx::manual_paid_applies`).
    pub paid_toggle: Option<bool>,
    pub classes: String,
    pub gutter: String,
    pub title: String,

    pub mark: MarkView,

    // meta
    pub meta_text: Option<String>,
    pub meta_tone: &'static str,
    pub installments: Option<String>,
    pub flags: Vec<FlagView>,

    // files
    pub files: Vec<FileView>,
    /// The slots this row could hold a file in and does not — empty unless
    /// `attachable`. Slot 2 is never here on an income: you do not schedule
    /// money arriving.
    pub missing: Vec<SlotView>,
    /// Whether this row can hold files at all. A one-off has no taxonomy to
    /// file under, so it gets no attach control — the feature is scoped to
    /// recurring-rule occurrences, and the row says which it is.
    pub attachable: bool,

    // amount
    pub amount: String,
    pub amount_income: bool,
    pub amount_estimated: bool,
    pub amount_placeholder: bool,
    pub delta: Option<DeltaView>,

    pub error: Option<String>,
    pub is_skeleton: bool,
    pub details: Details,
}

/// The status mark, flattened for `partials/mark.html`. Shared by the row
/// and by the `?` legend, so the legend cannot show a mark the ledger does
/// not draw.
#[derive(Clone, Copy, Debug)]
pub struct MarkView {
    pub kind: &'static str, // disc | outline | ring | dot | skeleton | error
    pub tone: &'static str,
    pub glyph: &'static str,
    pub inner: Option<&'static str>,
    /// Pre-computed `stroke-dashoffset` for a 10.5px-radius ring.
    pub ring_offset: f32,
}

/// The ring's color at a given elapsed fraction: green, then amber from two
/// thirds of the way, then red once the due date has passed.
pub fn ring_tone_at(progress: f32) -> Tone {
    if progress >= 1.0 {
        Tone::Neg
    } else if progress >= 0.66 {
        Tone::Accent
    } else {
        Tone::Pos
    }
}

impl Mark {
    pub fn view(&self) -> MarkView {
        let (kind, tone, glyph, inner) = match self {
            Mark::Skeleton => ("skeleton", "muted", "", None),
            Mark::Error => ("error", "neg", "alert", None),
            Mark::Disc { tone, glyph } => ("disc", tone.css(), *glyph, None),
            Mark::Outline { tone, glyph } => ("outline", tone.css(), *glyph, None),
            Mark::Ring { tone, glyph, .. } => ("ring", tone.css(), "", *glyph),
            Mark::Dot { tone, filled } => {
                ("dot", if *filled { tone.css() } else { "muted" }, "", None)
            }
        };
        let progress = match self {
            Mark::Ring { progress, .. } => *progress,
            _ => 0.0,
        };
        MarkView {
            kind,
            tone,
            // The mark's glyph is chosen here rather than in the template so
            // an unmapped name can never reach the page as literal text.
            glyph: match glyph {
                "check" => "✓",
                "alert" => "!",
                other => {
                    debug_assert!(other.is_empty(), "unmapped mark glyph: {other}");
                    ""
                }
            },
            inner: inner.map(|_| "◷"),
            ring_offset: RING_CIRCUMFERENCE * (1.0 - progress),
        }
    }
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
    /// The computed path, shown in the control's tooltip — the vault is meant
    /// to be browsable by hand, so knowing where a file actually is matters.
    pub path: String,
    /// The route that serves it. Never the path: a URL built from user-typed
    /// taxonomy is a path traversal waiting to be written, so the server
    /// recomputes it from the row's own id and kind instead.
    pub href: String,
    /// `barcode` | `scheduled` | `receipt`, for the delete and upload actions.
    pub kind: &'static str,
    pub page: Option<String>,
    pub warn: bool,
}

/// An empty slot on an attachable row, drawn as its own `+` control under
/// `density = default` — `+1 Bill`, `+3 Receipt` — instead of the single
/// `+` menu. Rendered at every density and shown by CSS, so a density change
/// needs nothing from the server but the attribute.
pub struct SlotView {
    pub svg: &'static str,
    pub number: u8,
    pub label: &'static str,
    /// `barcode` | `scheduled` | `receipt` — the picker's kind.
    pub kind: &'static str,
}

/// The word for a slot: chosen by the kind AND which way the money goes — a
/// spend's Bill is an income's Invoice. One match, for the files a row has
/// and the slots it is missing alike.
fn slot_label(kind: FileKind, income: bool, t: &Catalog) -> &'static str {
    match (kind, income) {
        (FileKind::Bill, false) => t.file_bill(),
        (FileKind::Bill, true) => t.file_invoice(),
        (FileKind::Scheduled, _) => t.file_scheduled(),
        (FileKind::Receipt, false) => t.file_receipt(),
        (FileKind::Receipt, true) => t.file_payslip(),
    }
}

fn slot_svg(kind: FileKind) -> &'static str {
    crate::ui::icons::svg(match kind {
        FileKind::Bill => "bill",
        FileKind::Scheduled => "scheduled",
        FileKind::Receipt => "receipt",
    })
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
        let mark = self.mark().view();

        RowView {
            id: self.id,
            rule_id: self.rule_id,
            paid_toggle: (self.rule_id.is_some()
                && !self.tracked
                && matches!(self.state, RowState::Ok)
                && !self.flags.deleted)
                .then_some(self.manual_paid),
            classes: self.classes().join(" "),
            gutter: self.gutter.text(),
            // A placeholder's label is the app talking, not the household:
            // "New recurring bill" is a prompt for a row that does not exist
            // yet, so it comes from the catalog rather than from a title
            // nobody typed. Everything else is content, and is left alone.
            title: match (&self.state, self.kind.is_income()) {
                (RowState::Placeholder, false) => t.rule_new_spend().to_string(),
                (RowState::Placeholder, true) => t.rule_new_income().to_string(),
                _ => self.title.clone(),
            },

            mark,

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
                    svg: slot_svg(c.kind),
                    number: c.number,
                    // From the catalog, not a literal. The rail is one of the
                    // few places a word is chosen by BOTH the file's kind and
                    // which way the money goes — a spend's Bill is an
                    // income's Invoice — and doing that match where there is
                    // no Catalog in scope is how it ended up in English on a
                    // pt-BR ledger.
                    label: slot_label(c.kind, self.kind.is_income(), t),
                    page: c.page.map(|p| format!("p.{}", p.page)),
                    warn: c.page.is_some_and(|p| !p.in_window),
                    href: format!("/tx/{}/file/{}", self.id, c.kind.storage().code()),
                    kind: c.kind.storage().code(),
                    path: c.path,
                })
                .collect(),

            attachable: self.attachable(),
            missing: self.missing_slots(t),

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

            details: Details {
                due: self.due_date.map(fmt::date),
                cadence: self.cadence_label(t),
                // The computed path, whether or not the file is there yet:
                // "where would it go" is exactly what somebody asks when it
                // is missing.
                receipt: self.receipt_path_preview(),
                status: self.status_sentence(t),
                recurring: self.is_recurring(),
                rule_deleted: self.rule_deleted_on.map(fmt::date),
            },

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
    use crate::ui::i18n::Lang;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn ctx() -> RowCtx {
        RowCtx {
            sort: Default::default(),
            today: d(2026, 9, 7),
            currency: Currency::Brl,
            show_recurring: true,
            show_delta: true,
            narrow: false,
            line_numbers: LineNumbers::Relative,
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
            file_bucket: Some("Consumption".into()),
            file_bill_name: Some("Power".into()),
            manual_paid: false,
            tracked: true,
            instance: None,
            cadence_months: Some(1),
            rule_deleted_on: None,
            total_installments: None,
            spans_period: false,
            window_size: None,
            window_start: None,
            amount_in_filename: false,
            varies: false,
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
    fn a_deleted_rule_leaves_an_orphaned_bill_not_a_recurring_one() {
        let mut r = base();
        r.total_installments = Some(25);
        r.installments_paid = Some(1);
        r.files.receipt = Some("pdf".into());
        r.rule_deleted_on = Some(d(2026, 9, 24));

        // Stops claiming to recur…
        assert!(!r.is_recurring());
        assert_eq!(r.flag_pair()[0], ("recurring", false));
        assert_eq!(r.installments(), None);
        let v = r.view(&Catalog::new(Lang::En));
        assert_eq!(v.details.cadence, None);
        assert_eq!(v.details.rule_deleted.as_deref(), Some("24/09/2026"));

        // …but keeps its files and the status they prove.
        assert_eq!(r.status(), RowStatus::Paid);
        assert_eq!(v.files.len(), 1);
        assert!(v.details.receipt.is_some());
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
        // The 1st is a day begun: a sliver, never an empty circle.
        r.ctx.today = d(2026, 9, 1);
        assert!(r.ring_progress().unwrap() > 0.0);
        assert_eq!(r.ring_tone(), Tone::Pos);

        r.ctx.today = d(2026, 9, 8); // the 8th of 15 days
        assert!((r.ring_progress().unwrap() - 8.0 / 15.0).abs() < 0.01);

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
    fn delta_color_inverts_for_income_but_the_arrow_does_not() {
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
            "rodrigo/consumption/power/2026/09.pdf"
        );

        r.files.bill = Some("jpg".into());
        assert_eq!(
            r.file_path(FileKind::Bill).unwrap(),
            "rodrigo/consumption/power/2026/09_barcode.jpg"
        );

        r.files.scheduled = Some("pdf".into());
        assert_eq!(
            r.file_path(FileKind::Scheduled).unwrap(),
            "rodrigo/consumption/power/2026/09_scheduled.pdf"
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
            "rodrigo/consumption/power/house_8898/2026/09.pdf"
        );
    }

    #[test]
    fn the_amount_suffix_is_opt_in_and_needs_a_real_value() {
        let mut r = base();
        r.files.receipt = Some("pdf".into());
        r.amount_in_filename = true;
        assert_eq!(
            r.file_path(FileKind::Receipt).unwrap(),
            "rodrigo/consumption/power/2026/09_brl343.62.pdf"
        );

        // Nothing is ever named after a zero nobody owes.
        r.amount_cents = None;
        assert_eq!(
            r.file_path(FileKind::Receipt).unwrap(),
            "rodrigo/consumption/power/2026/09.pdf"
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
        assert!(!rail.iter().any(|c| c.kind == FileKind::Scheduled));
        // The numbers are fixed by kind and never renumbered by what exists:
        // an income row's rail is slots 1 and 3, with nothing where 2 would
        // be — which is what makes `g1` and `g3` mean one thing everywhere.
        assert_eq!(rail[0].number, 1);
        assert_eq!(rail[1].number, 3);

        // And the words come from the catalog, in whichever language is
        // asking — the reason they are no longer on FileControl at all.
        let en = r.clone().view(&Catalog::new(Lang::En));
        assert_eq!(en.files[0].label, "Invoice");
        assert_eq!(en.files[1].label, "Payslip");
        let pt = r.view(&Catalog::new(Lang::PtBr));
        assert_eq!(pt.files[0].label, "Fatura");
        assert_eq!(pt.files[1].label, "Holerite");
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
    }

    #[test]
    fn the_gutter_follows_what_the_member_chose() {
        // The pane query used to hardcode Absolute, so `relative` and `none`
        // were settings you could pick and nothing happened.
        use LineNumbers::*;
        assert_eq!(Absolute.gutter(0, 0), Gutter::Absolute(1));
        assert_eq!(Absolute.gutter(4, 2), Gutter::Absolute(5));
        assert_eq!(Off.gutter(4, 2), Gutter::Hidden);

        // Relative is measured from the cursor, and the row under it reads 0.
        assert_eq!(Relative.gutter(2, 2), Gutter::Relative(0));
        assert_eq!(Relative.gutter(0, 3).text(), "3");
        assert_eq!(Relative.gutter(5, 3).text(), "2");
    }

    #[test]
    fn relative_is_rendered_from_the_cursor_the_server_can_assume() {
        // The server does not have the cursor — it is Alpine's — but the
        // cursor always starts on the first row, so relative-to-0 is right
        // for the first frame and the client keeps it right after that.
        use LineNumbers::*;
        let first_frame: Vec<String> = (0..4).map(|i| Relative.gutter(i, 0).text()).collect();
        assert_eq!(first_frame, ["0", "1", "2", "3"]);

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
