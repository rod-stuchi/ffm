//! Row state, modeled as three independent things rather than one enum.
//!
//! `design/spec.md`'s row-state table reads like twenty mutually exclusive
//! states. It is not, and transcribing it as one flat enum is the trap: it
//! works until a row is focused *and* selected *and* private *and* holds a
//! parked draft — which is a screenshot in the mockup, not a hypothetical. You
//! would need a `FocusedSelectedPrivateDraft` variant, and then another.
//!
//! So:
//!
//! * [`RowStatus`] — mutually exclusive. Exactly one is true of any real row.
//!   This is the table's *status mark* column and nothing else.
//! * [`RowState`] — the lifecycle of the rendered *fragment*, not of the
//!   transaction: in flight, failed, a placeholder awaiting Create.
//! * [`RowFlags`] — independent visual channels that compose freely.
//!
//! "Estimated" is deliberately none of the three: it is a property of the
//! amount, so it lives on the amount.

/// The one mutually-exclusive fact about a row's payment state.
///
/// Spend and income run on different axes and never borrow each other's words:
/// a bill is `paid`, income is `received`. Income never reports `Paid`,
/// `Scheduled` or `ScheduledOverdue`.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum RowStatus {
    /// Tracked, and the receipt file exists on disk. The disk decides this —
    /// never a database flag that could quietly disagree with the folder.
    Paid,
    /// Untracked, and a human ticked the box. The only manual assertion in the
    /// whole model, allowed precisely because there is no file to contradict it.
    ManualPaid,
    /// Tracked, a scheduled-payment file exists, the due date has not passed.
    Scheduled,
    /// Tracked, a scheduled-payment file exists, the due date HAS passed and no
    /// receipt landed: a promise that was never confirmed kept. Deliberately
    /// distinct from `Pending` — "nothing has happened yet" and "something was
    /// promised and did not happen" are different news.
    ScheduledOverdue,
    /// Tracked, neither a receipt nor a scheduled file.
    Pending,
    /// Untracked and unticked. No filesystem proof either way, so no claim.
    Plain,
    /// Income that has not arrived yet.
    Expected,
    /// Income that landed — by payslip on disk, or by the manual tick.
    Received,
}

impl RowStatus {
    /// Every variant, so a fixture gallery can assert it covers all of them.
    /// A new status with no fixture fails the sandbox's coverage test.
    pub const ALL: [RowStatus; 8] = [
        RowStatus::Paid,
        RowStatus::ManualPaid,
        RowStatus::Scheduled,
        RowStatus::ScheduledOverdue,
        RowStatus::Pending,
        RowStatus::Plain,
        RowStatus::Expected,
        RowStatus::Received,
    ];

    /// The stable slug used in sandbox URLs and CSS state classes.
    pub fn slug(self) -> &'static str {
        match self {
            RowStatus::Paid => "paid",
            RowStatus::ManualPaid => "manual-paid",
            RowStatus::Scheduled => "scheduled",
            RowStatus::ScheduledOverdue => "scheduled-overdue",
            RowStatus::Pending => "pending",
            RowStatus::Plain => "plain",
            RowStatus::Expected => "expected",
            RowStatus::Received => "received",
        }
    }

    /// Whether this status counts toward the month-picker's warning dot.
    ///
    /// Only a *tracked* occurrence can be unresolved: an untracked bill with no
    /// tick has no filesystem evidence either way, so flagging it would be
    /// nagging based on nothing.
    pub fn is_unresolved(self, today_is_past_due: bool) -> bool {
        match self {
            RowStatus::ScheduledOverdue => true,
            RowStatus::Pending | RowStatus::Expected => today_is_past_due,
            _ => false,
        }
    }

    pub fn is_income(self) -> bool {
        matches!(self, RowStatus::Expected | RowStatus::Received)
    }
}

/// The lifecycle of the rendered fragment. Orthogonal to [`RowStatus`]: a
/// skeleton has no payment state yet, and an error row never had one.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum RowState {
    #[default]
    Ok,
    /// In flight. Its geometry must match the real row exactly, so nothing
    /// shifts when the server's fragment lands.
    Skeleton,
    /// The server said no and wrote nothing. The copy says so; "Try Again"
    /// resubmits the original action.
    Error { message: String },
    /// The dashed "New recurring bill / income" row `A` and `I` park at the
    /// cursor for the rule pane's arrow to point at. Create turns it into a
    /// real row in that spot; Cancel removes it.
    Placeholder,
}

impl RowState {
    pub fn slug(&self) -> &'static str {
        match self {
            RowState::Ok => "ok",
            RowState::Skeleton => "skeleton",
            RowState::Error { .. } => "error",
            RowState::Placeholder => "placeholder",
        }
    }
}

/// What kind of form a row currently has open, or has parked.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum EditKind {
    /// `e` — the row opens into its own form, in place.
    Occurrence,
    /// `E` / `A` / `I` — the rule pane, a column beside the ledger, with an
    /// arrow pointing back at this row.
    Rule,
}

/// Independent channels. Each composes with any other, which is the entire
/// reason these are not enum variants.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct RowFlags {
    /// Cursor position: accent left rail, gutter `0`, tinted ground.
    pub focused: bool,
    /// `x` — a marker for a batch operation. Never a delete.
    pub selected: bool,
    /// Matched the current `/` search.
    pub matched: bool,
    /// Soft-deleted: dimmed, struck through. `u` clears the mark on this same
    /// row — restore happens in place, it does not insert a superseding row.
    pub deleted: bool,
    /// Hidden from other members, never from the admin.
    pub private: bool,
    /// Generated by a recurring rule.
    pub recurring: bool,
    /// A form is open on this row right now.
    pub editing: Option<EditKind>,
    /// A changed draft parked here: dashed border, `unsaved · e to resume`.
    /// Several can be parked at once, and leaving a row does not discard one.
    pub draft: Option<EditKind>,
    /// The change-log pane is open and pointing at this row.
    pub log_open: bool,
}

impl RowFlags {
    pub fn focused() -> Self {
        RowFlags {
            focused: true,
            ..Default::default()
        }
    }
}

/// Which way the money goes. Direction lives here, never in an amount's sign.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum TxKind {
    In,
    Out,
}

impl TxKind {
    pub fn code(self) -> &'static str {
        match self {
            TxKind::In => "in",
            TxKind::Out => "out",
        }
    }

    pub fn parse(s: &str) -> Option<TxKind> {
        match s {
            "in" => Some(TxKind::In),
            "out" => Some(TxKind::Out),
            _ => None,
        }
    }

    pub fn is_income(self) -> bool {
        matches!(self, TxKind::In)
    }
}

/// `household` by default; a `private` row is hidden from other members but
/// never from the admin.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
pub enum Visibility {
    #[default]
    Household,
    Private,
}

impl Visibility {
    pub fn code(self) -> &'static str {
        match self {
            Visibility::Household => "household",
            Visibility::Private => "private",
        }
    }

    pub fn parse(s: &str) -> Option<Visibility> {
        match s {
            "household" => Some(Visibility::Household),
            "private" => Some(Visibility::Private),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_unique() {
        // They become CSS state classes and sandbox URL segments; a collision
        // would silently style two different states the same way.
        let mut seen: Vec<&str> = RowStatus::ALL.iter().map(|s| s.slug()).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before);
    }

    #[test]
    fn income_and_spend_never_share_a_status() {
        for s in RowStatus::ALL {
            let income = matches!(s, RowStatus::Expected | RowStatus::Received);
            assert_eq!(s.is_income(), income);
        }
        // A bill is paid, income is received. The words do not cross over.
        assert!(!RowStatus::Paid.is_income());
        assert!(RowStatus::Received.is_income());
    }

    #[test]
    fn only_tracked_states_can_be_unresolved() {
        // Past its due date and still nothing filed.
        assert!(RowStatus::ScheduledOverdue.is_unresolved(false));
        assert!(RowStatus::Pending.is_unresolved(true));
        assert!(RowStatus::Expected.is_unresolved(true));
        assert!(!RowStatus::Pending.is_unresolved(false));
        // An untracked row is never an issue, however old — there is no
        // evidence to act on, so flagging it would be nagging about nothing.
        assert!(!RowStatus::Plain.is_unresolved(true));
        assert!(!RowStatus::ManualPaid.is_unresolved(true));
        assert!(!RowStatus::Paid.is_unresolved(true));
    }

    #[test]
    fn flags_compose() {
        // The combination that breaks a single flat enum.
        let f = RowFlags {
            focused: true,
            selected: true,
            private: true,
            draft: Some(EditKind::Occurrence),
            ..Default::default()
        };
        assert!(f.focused && f.selected && f.private);
        assert_eq!(f.draft, Some(EditKind::Occurrence));
        assert!(!f.deleted);
    }
}
