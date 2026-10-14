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
    /// An amount: pixelated with every other `.money` while `gh` hides them.
    pub money: bool,
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
            note: e.note.as_deref().map(|n| note_text(n, t)),
            fields: e
                .fields
                .iter()
                .map(|f| FieldView {
                    label: field_label(&f.field, t),
                    old: value(f, f.old.as_deref(), currency, t),
                    new: value(f, f.new.as_deref(), currency, t),
                    money: f.kind == "money",
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
    // A re-attribution records the stored names; people read them capitalized.
    if f.field == "member" {
        return fmt::name(raw);
    }
    // Values stored as codes — the words are the member's language.
    let word = match (f.field.as_str(), raw) {
        ("visibility", "household") => Some(t.log_value_household()),
        ("visibility", "private") => Some(t.log_value_private()),
        ("kind", "barcode") => Some(t.file_bill()),
        ("kind", "scheduled") => Some(t.file_scheduled()),
        ("kind", "receipt") => Some(t.file_receipt()),
        ("status", "active") => Some(t.log_value_active()),
        ("status", "inactive") => Some(t.log_value_inactive()),
        _ => None,
    };
    if let Some(w) = word {
        return w.to_string();
    }
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
        "issuer" => t.log_field_issuer(),
        "card" => t.log_field_card(),
        "pdf_password" => t.log_field_pdf_password(),
        "cadence" => t.log_field_cadence(),
        "start_date" => t.log_field_start_date(),
        "end_date" => t.log_field_end_date(),
        "tracked" => t.log_field_tracked(),
        "path" => t.log_field_path(),
        "kind" => t.log_field_kind(),
        "original_filename" => t.log_field_original_filename(),
        "rows" => t.log_field_rows(),
        "position" => t.log_field_position(),
        "sort_key" => t.log_field_sort_key(),
        "visibility" => t.log_field_visibility(),
        "months" => t.log_field_months(),
        "company_code" => t.log_field_company_code(),
        "client_code" => t.log_field_client_code(),
        "barcode" => t.log_field_barcode(),
        "status" => t.log_field_status(),
        "recipe" => t.log_field_recipe(),
        "member" => t.log_field_member(),
        "kept" => t.log_field_kept(),
        other => return other.replace('_', " "),
    }
    .to_string()
}

/// A change's note, in the member's language.
///
/// Notes are stored in English and stay that way — the trash, `u` and every
/// restore match on them (`db::trash`), so rewriting what is stored would
/// break the very thing they mark. They are worded here, at display, from the
/// constants that wrote them; a note this does not know shows as stored.
pub(crate) fn note_text(note: &str, t: &Catalog) -> String {
    use crate::services::receipts as r;
    let fixed = match note {
        "manual" => Some(t.log_note_manual()),
        "superseded" => Some(t.log_note_superseded()),
        r::SWAPPED => Some(t.log_note_swapped()),
        r::WITH_ROW => Some(t.log_note_with_row()),
        r::MOVED => Some(t.log_note_moved()),
        r::FOUND_BY_HAND => Some(t.log_note_found_by_hand()),
        r::REMOVED_BY_HAND => Some(t.log_note_removed_by_hand()),
        r::REFILED => Some(t.log_note_refiled()),
        crate::services::prune::PRUNED => Some(t.log_note_pruned()),
        crate::db::order::COPIED => Some(t.log_note_order_copied()),
        crate::web::import::ATTACHED_UNCONFIRMED => Some(t.log_note_attached_unconfirmed()),
        crate::db::auth::CONSOLE_RESET => Some(t.log_note_console_reset()),
        crate::db::members::SIGNED_OUT_OTHERS => Some(t.log_note_signed_out_others()),
        _ => None,
    };
    if let Some(s) = fixed {
        return s.to_string();
    }
    if let Some(rest) = note.strip_prefix(crate::db::order::COPIED_FROM) {
        return match rest.strip_suffix(crate::db::order::UNDONE) {
            Some(period) => t.log_note_order_from_undone(period),
            None => t.log_note_order_from(rest),
        };
    }
    if let Some(detail) = note
        .strip_prefix(crate::web::import::ATTACHED_MISMATCH)
        .and_then(|d| d.strip_prefix(": "))
    {
        return t.log_note_attached_mismatch(detail);
    }
    if let Some(source) = note.strip_prefix(crate::db::recipes::TAUGHT_BY) {
        return t.log_note_recipe(source);
    }
    note.to_string()
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

    /// Notes are stored in English, so the trash can match on them; a
    /// Portuguese reader still reads Portuguese — fixed, with a value inside,
    /// and an unknown one shown as it was written.
    #[test]
    fn a_note_is_worded_for_whoever_reads_it() {
        let pt = Catalog::new(Lang::PtBr);
        assert_eq!(
            note_text(crate::services::receipts::WITH_ROW, &pt),
            "foi junto com a linha"
        );
        assert_eq!(
            note_text("superseded", &pt),
            "substituído por um arquivo mais novo"
        );
        assert_eq!(
            note_text("order copied from 10/2026 — undone", &pt),
            "ordem copiada de 10/2026 — desfeito"
        );
        assert_eq!(
            note_text("bill recipe taught by codex", &pt),
            "receita da conta ensinada por codex"
        );
        assert_eq!(note_text("something older", &pt), "something older");
    }

    #[test]
    fn a_field_and_its_coded_values_are_worded_too() {
        let pt = Catalog::new(Lang::PtBr);
        assert_eq!(field_label("position", &pt), "posição");
        let f = field("visibility", "text", Some("household"), Some("private"));
        assert_eq!(
            value(&f, f.old.as_deref(), Currency::Brl, &pt),
            "compartilhado"
        );
        assert_eq!(value(&f, f.new.as_deref(), Currency::Brl, &pt), "privado");
        let k = field("kind", "text", None, Some("receipt"));
        assert_eq!(
            value(&k, k.new.as_deref(), Currency::Brl, &pt),
            "Comprovante"
        );
    }
}
