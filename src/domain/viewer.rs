//! Who is looking, and what they are allowed to see.
//!
//! Small on purpose, and in `domain` rather than in `web`, because the same
//! answer has to be given in two places that must never disagree: the `WHERE`
//! clause that decides which rows come back, **and** the sum that decides what
//! the pane's totals say. A hidden row that is filtered out of the list but
//! still counted in the balance announces its own existence.

/// The session's member, plus whether admin powers are in effect right now.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Viewer {
    /// Always the **session's** member — never the focused pane's. Focusing
    /// somebody else's ledger must not hand you their settings or their
    /// hidden rows.
    pub member_id: i64,
    /// The role on the row: 0 = admin.
    pub role_admin: bool,
    /// Preview-as-non-admin. Not impersonation: same session, same member,
    /// same pane, own private rows still fully visible. The single thing it
    /// changes is whether admin-only visibility and actions are in effect.
    pub preview: bool,
}

impl Viewer {
    pub fn new(member_id: i64, role_admin: bool) -> Viewer {
        Viewer {
            member_id,
            role_admin,
            preview: false,
        }
    }

    /// Whether admin powers apply **right now**. Every admin-only check reads
    /// this rather than `role_admin`, which is what makes preview mode one
    /// flag instead of a flag plus a hundred call sites to remember.
    pub fn is_admin(self) -> bool {
        self.role_admin && !self.preview
    }

    /// Ownership grants visibility independent of role, so an admin previewing
    /// as a non-admin still sees every one of their own private rows — that is
    /// exactly what makes preview *not* a session swap.
    pub fn sees_private_of(self, owner_member_id: i64) -> bool {
        owner_member_id == self.member_id || self.is_admin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_admin_sees_everyone_and_a_member_sees_only_their_own() {
        let admin = Viewer::new(1, true);
        let member = Viewer::new(2, false);

        assert!(admin.sees_private_of(2), "role 0 sees every private row");
        assert!(member.sees_private_of(2), "and everyone sees their own");
        assert!(!member.sees_private_of(1));
    }

    #[test]
    fn preview_removes_admin_reach_but_never_your_own_rows() {
        let previewing = Viewer {
            preview: true,
            ..Viewer::new(1, true)
        };
        assert!(!previewing.is_admin(), "admin-only actions refuse");
        assert!(
            !previewing.sees_private_of(2),
            "somebody else's private row is genuinely hidden, not grayed out"
        );
        assert!(
            previewing.sees_private_of(1),
            "ownership grants visibility independent of role — this is what \
             makes preview a view, not an impersonation"
        );
    }
}
