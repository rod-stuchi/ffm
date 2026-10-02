//! Rendering the change log — a pure read of what the writer has been
//! recording since before the first mutation existed.
//!
//! The one interesting decision here is that **`kind` travels with the
//! value**. A logged amount is stored in minor units and rendered with the
//! currency it is read under, without the pane inferring anything from the
//! field's name — which is also why a field this app has never heard of still
//! renders sensibly rather than blowing up.

use crate::db::changes::{Entry, FieldEntry};
use crate::domain::money::{Cents, Currency};
use crate::ui::fmt;
use crate::ui::i18n::Catalog;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

/// An em dash, for a value that was not there. Reads as "nothing" rather than
/// as an empty gap the eye skips.
const NOTHING: &str = "—";

pub struct FieldView {
    pub label: String,
    pub old: String,
    pub new: String,
}

pub struct EntryView {
    pub id: i64,
    /// `14:32`, in the household's own time — the log stores UTC, and nobody
    /// reads their own ledger in UTC.
    pub time: String,
    /// Who, or what. The materializer and the system have no actor, and the
    /// pane says which rather than inventing a person.
    pub actor: String,
    pub verb: String,
    pub is_rule: bool,
    /// `3 fields`, when there is a diff to OPEN. A single field is shown
    /// inline instead, so it carries no count either — "edited · 1 field"
    /// above the one line it is describing says nothing the line does not.
    pub count: Option<String>,
    /// One field, rendered under the entry with no fold at all.
    ///
    /// Expanding to reveal a single line is a poor trade, and one field is
    /// the common case: a privacy toggle, a reorder, a re-attribution and a
    /// single edit each produce exactly one. (Delete and restore produce
    /// none, and already offer nothing to open.)
    pub inline: bool,
    /// `undo of 18:39` — an undo is an entry, never a rewrite.
    pub undo_of: Option<String>,
    pub note: Option<String>,
    pub fields: Vec<FieldView>,
}

/// Entries under the day they happened, newest first.
pub struct DayGroup {
    pub label: String,
    pub entries: Vec<EntryView>,
    /// How many entries come before this group, so the template can give
    /// every entry a **flat** index across the day headings. The cursor walks
    /// entries, not days, and `j` from the last entry of one day belongs on
    /// the first of the next.
    pub offset: usize,
}

/// Groups a rule's and a row's entries by day and formats every value.
pub fn render(
    entries: &[Entry],
    t: &Catalog,
    currency: Currency,
    today: NaiveDate,
) -> Vec<DayGroup> {
    let mut groups: Vec<DayGroup> = Vec::new();

    // The time an entry points at, for "undo of 18:39". Built first because an
    // undo may sit above the change it reverses.
    let times: std::collections::HashMap<i64, String> =
        entries.iter().map(|e| (e.id, local_time(&e.at))).collect();

    for e in entries {
        let day = local_date(&e.at).unwrap_or(today);
        let label = day_label(day, today, t);

        let view = EntryView {
            id: e.id,
            time: local_time(&e.at),
            actor: actor_label(e, t),
            verb: verb(&e.action, t),
            is_rule: e.is_rule,
            // Delete and restore carry no fields: it is the row's existence
            // that changed, not one of its values.
            count: (e.fields.len() > 1).then(|| t.log_fields(e.fields.len() as i64)),
            inline: e.fields.len() == 1,
            undo_of: e
                .undo_of
                .and_then(|id| times.get(&id))
                .map(|at| t.log_undo_of(at)),
            note: e.note.clone(),
            fields: e
                .fields
                .iter()
                .map(|f| FieldView {
                    label: field_label(&f.field, t),
                    old: value(f, f.old.as_deref(), currency, t),
                    new: value(f, f.new.as_deref(), currency, t),
                })
                .collect(),
        };

        match groups.last_mut() {
            Some(g) if g.label == label => g.entries.push(view),
            _ => groups.push(DayGroup {
                label,
                entries: vec![view],
                // Filled in below, once every group is known.
                offset: 0,
            }),
        }
    }

    let mut seen = 0;
    for g in &mut groups {
        g.offset = seen;
        seen += g.entries.len();
    }
    groups
}

/// Formats one stored value by its `kind`.
///
/// Storage-neutral in, household-formatted out — and an unknown kind falls
/// through to the raw text rather than panicking, because the log is
/// append-only and will outlive several versions of this function.
fn value(f: &FieldEntry, raw: Option<&str>, currency: Currency, t: &Catalog) -> String {
    let Some(raw) = raw else {
        return NOTHING.to_string();
    };
    match f.kind.as_str() {
        "money" => raw
            .parse::<i64>()
            .map(|c| fmt::money(Cents(c), currency))
            .unwrap_or_else(|_| raw.to_string()),
        "date" => NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .map(fmt::date)
            .unwrap_or_else(|_| raw.to_string()),
        "bool" => if raw == "1" { t.log_on() } else { t.log_off() }.to_string(),
        // `day`, `text` and `path` are already what they look like.
        _ => raw.to_string(),
    }
}

pub(crate) fn verb(action: &str, t: &Catalog) -> String {
    match action {
        "create" => t.log_action_create(),
        "edit" => t.log_action_edit(),
        "delete" => t.log_action_delete(),
        "restore" => t.log_action_restore(),
        "reattribute" => t.log_action_reattribute(),
        "reorder" => t.log_action_reorder(),
        "privacy" => t.log_action_privacy(),
        "rule_create" => t.log_action_rule_create(),
        "rule_edit" => t.log_action_rule_edit(),
        "file_attach" => t.log_action_file_attach(),
        "file_unlink" => t.log_action_file_unlink(),
        "file_upload" => t.log_action_file_upload(),
        "file_delete" => t.log_action_file_delete(),
        "import" => t.log_action_import(),
        "materialize" => t.log_action_materialize(),
        "password_reset" => t.log_action_password_reset(),
        "sessions_revoked" => t.log_action_sessions_revoked(),
        // The schema pins the list, so this is unreachable — but the log is
        // append-only and older than any one version of this table.
        other => return other.to_string(),
    }
    .to_string()
}

fn field_label(field: &str, t: &Catalog) -> String {
    match field {
        "amount" => t.log_field_amount(),
        "paid" => t.log_field_paid(),
        "title" => t.log_field_title(),
        "date" => t.log_field_date(),
        "due_day" => t.log_field_due_day(),
        "due_date" => t.log_field_due_date(),
        "bucket" => t.log_field_bucket(),
        "bill_name" => t.log_field_bill_name(),
        "instance" => t.log_field_instance(),
        "cadence" => t.log_field_cadence(),
        "start_date" => t.log_field_start_date(),
        "end_date" => t.log_field_end_date(),
        "tracked" => t.log_field_tracked(),
        "path" => t.log_field_path(),
        "kind" => t.log_field_kind(),
        "original_filename" => t.log_field_original_filename(),
        other => return other.replace('_', " "),
    }
    .to_string()
}

fn actor_label(e: &Entry, t: &Catalog) -> String {
    match (e.actor.as_deref(), e.source.as_str()) {
        (Some(name), _) => name.to_string(),
        (None, "materializer") => t.log_materializer().to_string(),
        (None, "import") => t.log_import().to_string(),
        _ => t.log_system().to_string(),
    }
}

/// `today` / `yesterday` / the date. Relative labels only where they are
/// unambiguous — two days back is a date, because "two days ago" makes a
/// reader count.
/// A change's fields as the statusline says them — `amount R$ 12,00 →
/// R$ 10,00` — formatted exactly as the log pane formats them. A stored path
/// is left out: a vault path is a line of its own, not a clause.
pub(crate) fn field_changes(fields: &[FieldEntry], currency: Currency, t: &Catalog) -> Vec<String> {
    fields
        .iter()
        .filter(|f| f.kind != "path")
        .map(|f| {
            format!(
                "{} {} → {}",
                field_label(&f.field, t),
                value(f, f.old.as_deref(), currency, t),
                value(f, f.new.as_deref(), currency, t),
            )
        })
        .collect()
}

pub(crate) fn day_label(day: NaiveDate, today: NaiveDate, t: &Catalog) -> String {
    if day == today {
        t.log_today().to_string()
    } else if Some(day) == today.pred_opt() {
        t.log_yesterday().to_string()
    } else {
        fmt::date(day)
    }
}

/// The log stores UTC; nobody reads their own ledger in UTC.
fn local(at: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(at)
        .map(|d| d.with_timezone(&chrono::Local).naive_local())
        .ok()
        .or_else(|| {
            // A timestamp written before this app existed, or by hand.
            NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M:%SZ")
                .ok()
                .map(|n| {
                    DateTime::<Utc>::from_naive_utc_and_offset(n, Utc)
                        .with_timezone(&chrono::Local)
                        .naive_local()
                })
        })
}

pub(crate) fn local_time(at: &str) -> String {
    local(at)
        .map(|d| d.format("%H:%M").to_string())
        .unwrap_or_else(|| at.to_string())
}

pub(crate) fn local_date(at: &str) -> Option<NaiveDate> {
    local(at).map(|d| d.date())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::changes::Entry;
    use crate::ui::i18n::Lang;

    fn entry(at: &str, action: &str, fields: Vec<FieldEntry>) -> Entry {
        Entry {
            id: 1,
            at: at.into(),
            actor: Some("Rodrigo".into()),
            source: "user".into(),
            action: action.into(),
            entity: "transaction".into(),
            is_rule: false,
            undo_of: None,
            note: None,
            fields,
        }
    }

    fn field(name: &str, kind: &str, old: Option<&str>, new: Option<&str>) -> FieldEntry {
        FieldEntry {
            field: name.into(),
            kind: kind.into(),
            old: old.map(String::from),
            new: new.map(String::from),
        }
    }

    #[test]
    fn money_is_rendered_in_the_currency_it_is_read_under() {
        let t = Catalog::new(Lang::En);
        let f = field("amount", "money", Some("34062"), Some("35210"));
        assert_eq!(value(&f, f.old.as_deref(), Currency::Brl, &t), "R$ 340,62");
        assert_eq!(value(&f, f.new.as_deref(), Currency::Usd, &t), "$352.10");
    }

    #[test]
    fn a_missing_value_reads_as_nothing_rather_than_as_a_gap() {
        let t = Catalog::new(Lang::En);
        let f = field("bucket", "text", None, Some("Consumo"));
        assert_eq!(value(&f, f.old.as_deref(), Currency::Brl, &t), "—");
        assert_eq!(value(&f, f.new.as_deref(), Currency::Brl, &t), "Consumo");
    }

    #[test]
    fn a_day_is_not_a_date_and_a_bool_is_not_a_one() {
        let t = Catalog::new(Lang::En);
        // due_day is a day of the month, and formatting it as a date would be
        // inventing a month and a year nobody wrote down.
        let day = field("due_day", "day", Some("11"), Some("18"));
        assert_eq!(value(&day, day.new.as_deref(), Currency::Brl, &t), "18");

        let date = field("date", "date", None, Some("2026-09-11"));
        assert_eq!(
            value(&date, date.new.as_deref(), Currency::Brl, &t),
            "11/09/2026"
        );

        let flag = field("tracked", "bool", Some("0"), Some("1"));
        assert_eq!(value(&flag, flag.old.as_deref(), Currency::Brl, &t), "off");
        assert_eq!(value(&flag, flag.new.as_deref(), Currency::Brl, &t), "on");
    }

    #[test]
    fn a_kind_this_version_has_never_heard_of_still_renders() {
        // The log is append-only and will outlive several versions of this
        // function, so an unknown kind falls through to its raw text.
        let t = Catalog::new(Lang::En);
        let f = field("something", "quantum", None, Some("42"));
        assert_eq!(value(&f, f.new.as_deref(), Currency::Brl, &t), "42");
        assert_eq!(field_label("some_new_field", &t), "some new field");
        assert_eq!(verb("teleported", &t), "teleported");
    }

    #[test]
    fn entries_are_grouped_by_day_newest_first() {
        let t = Catalog::new(Lang::En);
        let today = chrono::Local::now().date_naive();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        let groups = render(
            &[
                entry(
                    &now,
                    "edit",
                    vec![field("amount", "money", Some("1"), Some("2"))],
                ),
                entry(&now, "create", vec![]),
                entry("2020-01-02T10:00:00Z", "create", vec![]),
            ],
            &t,
            Currency::Brl,
            today,
        );

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].label, "today");
        assert_eq!(groups[0].entries.len(), 2);
        assert_eq!(
            groups[1].label, "02/01/2020",
            "older than yesterday is a date"
        );

        // A field count only where there is a diff to OPEN. One field is
        // shown inline instead, so it carries no count either: "edited · 1
        // field" above the single line it describes says nothing the line
        // does not. Delete and restore have neither — it is the row's
        // existence that moved, not one of its values.
        assert_eq!(groups[0].entries[0].count, None);
        assert!(groups[0].entries[0].inline, "one field is shown inline");
        assert_eq!(groups[0].entries[1].count, None);
        assert!(
            !groups[0].entries[1].inline,
            "no fields at all is not an inline field"
        );

        // The cursor walks ENTRIES, not days, so each group knows how many
        // came before it and `j` off the end of one day lands on the next.
        assert_eq!(groups[0].offset, 0);
        assert_eq!(groups[1].offset, groups[0].entries.len());
    }

    #[test]
    fn two_or_more_fields_still_fold_and_still_count() {
        let t = Catalog::new(Lang::En);
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let groups = render(
            &[entry(
                &now,
                "edit",
                vec![
                    field("title", "text", Some("a"), Some("b")),
                    field("bucket", "text", Some("c"), Some("d")),
                ],
            )],
            &t,
            Currency::Brl,
            chrono::Local::now().date_naive(),
        );
        assert_eq!(groups[0].entries[0].count.as_deref(), Some("2 fields"));
        assert!(
            !groups[0].entries[0].inline,
            "more than one field is worth a fold"
        );
    }

    #[test]
    fn an_actor_less_entry_names_what_did_it() {
        let t = Catalog::new(Lang::En);
        let mut e = entry("2026-09-01T03:00:00Z", "materialize", vec![]);
        e.actor = None;
        e.source = "materializer".into();
        // Attributing this to whoever happened to be signed in would be a lie
        // the log would then keep forever.
        assert_eq!(actor_label(&e, &t), "materializer");
    }
}
