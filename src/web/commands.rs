//! The `:` bar's vocabulary — one table, and everything that names a command
//! reads it.
//!
//! Before this there were four lists: the bar's hint (`command_hint`), the
//! `?` keymap's Type section, the Type table in `design/spec.md` and the
//! dispatcher's `case`s in `ledger.html`. `just cmd-check` held three of them
//! against each other, and the fourth drifted anyway — `?` kept advertising
//! `:add 50 mercado` long after `:add` was abandoned. Now the hint, the `?`
//! section, tab completion and the bar's contextual help are all rendered
//! from `ALL`, and `cmd-check` reads `ALL` against the dispatcher and the spec.
//!
//! **A command's arguments come from the same constants its write validates
//! against** — `ThemeName::ALL`, `Currency::ALL`, `settings::LEDGER_SORTS` and
//! so on — so Tab can never complete to a value the server then refuses.
//!
//! The names are identifiers a member types back, never catalog messages (the
//! same rule the sort-refusal message follows); what a command *does* is a
//! catalog message, because that is prose.

use serde::Serialize;

use crate::db::settings;
use crate::domain::money::Currency;
use crate::ui::i18n::Catalog;
use crate::ui::theme::ThemeName;

/// Who is offered a command.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Anyone,
    /// Only while admin powers are in effect — absent under preview, the same
    /// rule that removes the Household button rather than disabling it.
    Admin,
    /// Follows the ROLE rather than the powers: `:preview` is the toggle
    /// itself, and hiding it while previewing would say there is no way back.
    AdminRole,
}

pub struct Command {
    pub name: &'static str,
    /// Other spellings the dispatcher accepts. Completed and explained like
    /// the name, never listed in the hint — two spellings of one thing is
    /// noise there.
    pub aliases: &'static [&'static str],
    /// The closed set Tab completes the argument from; empty when the
    /// command takes none, or takes free text (`:grep`).
    pub args: fn() -> Vec<String>,
    pub who: Who,
    pub about: fn(&Catalog) -> String,
}

fn none() -> Vec<String> {
    Vec::new()
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

/// In the order a household reaches for them, which is the hint's order.
pub static ALL: [Command; 15] = [
    Command {
        name: "help",
        aliases: &["h"],
        args: none,
        who: Who::Anyone,
        about: |t| t.command_about_help().to_string(),
    },
    Command {
        name: "profile",
        aliases: &["me"],
        args: none,
        who: Who::Anyone,
        about: |t| t.command_about_profile().to_string(),
    },
    Command {
        name: "sort",
        aliases: &[],
        args: || owned(&settings::LEDGER_SORTS),
        who: Who::Anyone,
        about: |t| t.command_about_sort().to_string(),
    },
    Command {
        name: "group",
        aliases: &[],
        args: || owned(&settings::LEDGER_GROUPS),
        who: Who::Anyone,
        about: |t| t.command_about_group().to_string(),
    },
    Command {
        name: "set",
        aliases: &[],
        // The Vim spellings the dispatcher maps onto `line_numbers`; the long
        // ones (`relativenumber`, …) work too, but Tab offers the short ones.
        args: || owned(&["rnu", "nu", "nonu"]),
        who: Who::Anyone,
        about: |t| t.command_about_set().to_string(),
    },
    Command {
        name: "theme",
        aliases: &[],
        args: || ThemeName::ALL.iter().map(|n| n.key().to_string()).collect(),
        who: Who::Anyone,
        about: |t| t.command_about_theme().to_string(),
    },
    Command {
        name: "lang",
        aliases: &[],
        args: || owned(&settings::LANGUAGES),
        who: Who::Anyone,
        about: |t| t.command_about_lang().to_string(),
    },
    Command {
        name: "grep",
        aliases: &[],
        args: none,
        who: Who::Anyone,
        about: |t| t.command_about_grep().to_string(),
    },
    Command {
        name: "trash",
        aliases: &[],
        args: none,
        who: Who::Anyone,
        about: |t| t.command_about_trash().to_string(),
    },
    Command {
        name: "household",
        aliases: &["config"],
        args: none,
        who: Who::Admin,
        about: |t| t.command_about_household().to_string(),
    },
    Command {
        name: "members",
        aliases: &[],
        args: none,
        who: Who::Admin,
        about: |t| t.command_about_members().to_string(),
    },
    Command {
        name: "invite",
        aliases: &[],
        args: none,
        who: Who::Admin,
        about: |t| t.command_about_invite().to_string(),
    },
    Command {
        name: "currency",
        aliases: &[],
        args: || {
            Currency::ALL
                .iter()
                .map(|c| c.code().to_ascii_lowercase())
                .collect()
        },
        who: Who::Admin,
        about: |t| t.command_about_currency().to_string(),
    },
    Command {
        name: "preview",
        aliases: &[],
        args: none,
        who: Who::AdminRole,
        about: |t| t.command_about_preview().to_string(),
    },
    Command {
        name: "logout",
        aliases: &[],
        args: none,
        who: Who::Anyone,
        about: |t| t.command_about_logout().to_string(),
    },
];

/// The commands this viewer is offered. `admin` is whether admin powers are
/// in effect; `holds_role` whether the member holds the admin role at all.
pub fn visible(admin: bool, holds_role: bool) -> impl Iterator<Item = &'static Command> {
    ALL.iter().filter(move |c| match c.who {
        Who::Anyone => true,
        Who::Admin => admin,
        Who::AdminRole => holds_role,
    })
}

/// The bar's resting hint — one name per command.
pub fn hint(admin: bool, holds_role: bool) -> String {
    visible(admin, holds_role)
        .map(|c| c.name)
        .collect::<Vec<_>>()
        .join(" · ")
}

/// What the bar's Alpine component completes and explains from.
#[derive(Serialize)]
pub struct CommandView {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub args: Vec<String>,
    pub about: String,
}

pub fn views(t: &Catalog, admin: bool, holds_role: bool) -> Vec<CommandView> {
    visible(admin, holds_role)
        .map(|c| CommandView {
            name: c.name,
            aliases: c.aliases,
            args: (c.args)(),
            about: (c.about)(t),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_aliases_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in &ALL {
            for n in std::iter::once(&c.name).chain(c.aliases) {
                assert!(seen.insert(*n), "{n} is named twice");
            }
        }
    }

    #[test]
    fn a_member_is_not_offered_what_only_an_admin_may_run() {
        let member = hint(false, false);
        assert!(!member.contains("household") && !member.contains("currency"));
        assert!(!member.contains("preview"));
        // Previewing: the powers are off, but the toggle back stays.
        let previewing = hint(false, true);
        assert!(!previewing.contains("members") && previewing.contains("preview"));
        assert!(hint(true, true).contains("currency"));
    }

    #[test]
    fn every_completion_is_a_value_the_write_accepts() {
        for c in &ALL {
            for v in (c.args)() {
                let ok = match c.name {
                    "theme" => ThemeName::parse(&v).is_some(),
                    "currency" => Currency::parse(&v).is_some(),
                    "lang" => settings::LANGUAGES.contains(&v.as_str()),
                    "sort" => settings::LEDGER_SORTS.contains(&v.as_str()),
                    "group" => settings::LEDGER_GROUPS.contains(&v.as_str()),
                    "set" => ["rnu", "nu", "nonu"].contains(&v.as_str()),
                    other => panic!("{other} completes arguments nobody checks"),
                };
                assert!(ok, ":{} {v} would be refused", c.name);
            }
        }
    }
}
