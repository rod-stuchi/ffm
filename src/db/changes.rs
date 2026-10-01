//! The change log's write path — the app's one audit mechanism.
//!
//! This exists **before the first mutation**, on purpose. The log is
//! append-only, enforced by `BEFORE UPDATE`/`BEFORE DELETE` triggers, so an
//! entry that was not written at the time can never be honestly backfilled
//! afterwards: the timestamp would be a lie and the actor a guess. A mutation
//! phase built first would leave a permanent hole in the record.
//!
//! Three things are shaped by the type system rather than left to a comment:
//!
//! * **An entry and the row it describes commit together or not at all.**
//!   [`Change::write`] takes `&mut Transaction`, so there is no way to write
//!   one without the other being in the same SQLite transaction.
//! * **Action and entity are not independent.** `password_reset` on a
//!   transaction is nonsense, and the schema has a `CHECK` saying so — but a
//!   `CHECK` only fires at runtime. The constructors here take an action enum
//!   *per entity*, so the invalid pairs cannot be spelled.
//! * **A user action has an actor.** [`Actor::User`] carries a member id
//!   rather than leaving `member_id` an `Option` that a handler forgets.

use crate::domain::money::Cents;
use crate::domain::period::Period;
use chrono::NaiveDate;
use sqlx::{Sqlite, SqlitePool, Transaction};

// ---------------------------------------------------------------------------
// who, and in what capacity
// ---------------------------------------------------------------------------

/// Who did it. The session's member — **never** the focused pane's: focusing
/// somebody else's ledger does not make them the author of your edit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Actor {
    /// A person, at a keyboard. The id is not optional, which is the schema's
    /// `source <> 'user' OR member_id IS NOT NULL` expressed as a type.
    User(i64),
    /// A bill import. Somebody attached the file, so there is usually an actor.
    Import(Option<i64>),
    /// The hourly reconciler.
    Materializer,
    /// The app acting on its own — superseding a scheduled file when the
    /// receipt lands, for instance.
    System,
}

impl Actor {
    fn source(self) -> &'static str {
        match self {
            Actor::User(_) => "user",
            Actor::Import(_) => "import",
            Actor::Materializer => "materializer",
            Actor::System => "system",
        }
    }

    /// The member behind this actor, if there is one. Public because it is
    /// also what `member_invites.created_by` stores, and a console-issued
    /// reset legitimately has nobody to name there.
    pub fn member_id(self) -> Option<i64> {
        match self {
            Actor::User(id) => Some(id),
            Actor::Import(id) => id,
            Actor::Materializer | Actor::System => None,
        }
    }
}

// ---------------------------------------------------------------------------
// what happened, per entity
// ---------------------------------------------------------------------------

/// What can happen to a transaction. Deliberately not one flat `Action` enum
/// shared by every entity: that is what makes `password_reset` on a ledger row
/// expressible in the first place.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TxAction {
    Create,
    Edit,
    /// Soft: the row is marked, never removed.
    Delete,
    /// In place, on the same row — there is no superseding clone.
    Restore,
    Reattribute,
    Reorder,
    Privacy,
    Import,
    Materialize,
}

/// A rule. Note `Delete`/`Restore` map to the plain verbs with
/// `entity = 'rule'` — there is deliberately no `rule_delete`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RuleAction {
    Create,
    Edit,
    Delete,
    Restore,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FileAction {
    /// A barcode bound to a rule in its own editor.
    Attach,
    Unlink,
    Upload,
    /// Soft: the file moves to a sibling `.delete/`, never off disk.
    Delete,
}

/// The account actions an admin can take against somebody else. The first two
/// carry no fields — a password has no old value worth recording, and writing
/// one down would be worse than useless.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MemberAction {
    PasswordReset,
    SessionsRevoked,
    /// Turned off or back on — an `edit` with one `status` field,
    /// `active` ↔ `inactive`, so the log reads which way it went.
    Status,
    /// Into the trash, and back. Only a member with no rows can go there.
    Remove,
    Restore,
}

impl TxAction {
    fn code(self) -> &'static str {
        match self {
            TxAction::Create => "create",
            TxAction::Edit => "edit",
            TxAction::Delete => "delete",
            TxAction::Restore => "restore",
            TxAction::Reattribute => "reattribute",
            TxAction::Reorder => "reorder",
            TxAction::Privacy => "privacy",
            TxAction::Import => "import",
            TxAction::Materialize => "materialize",
        }
    }
}

impl RuleAction {
    fn code(self) -> &'static str {
        match self {
            RuleAction::Create => "rule_create",
            RuleAction::Edit => "rule_edit",
            RuleAction::Delete => "delete",
            RuleAction::Restore => "restore",
        }
    }
}

impl FileAction {
    fn code(self) -> &'static str {
        match self {
            FileAction::Attach => "file_attach",
            FileAction::Unlink => "file_unlink",
            FileAction::Upload => "file_upload",
            FileAction::Delete => "file_delete",
        }
    }
}

impl MemberAction {
    fn code(self) -> &'static str {
        match self {
            MemberAction::PasswordReset => "password_reset",
            MemberAction::SessionsRevoked => "sessions_revoked",
            MemberAction::Status => "edit",
            MemberAction::Remove => "delete",
            MemberAction::Restore => "restore",
        }
    }
}

// ---------------------------------------------------------------------------
// values
// ---------------------------------------------------------------------------

/// A field's value, with the `kind` that travels **with** it.
///
/// The pane formats `old -> new` from this kind rather than inferring one from
/// the field's name. That is what keeps a logged amount readable years later:
/// it is stored in minor units and rendered with the currency it was recorded
/// under, never re-interpreted through whatever the household has configured
/// today.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Money(Option<Cents>),
    Date(Option<NaiveDate>),
    /// A day of the month — a rule's `due_day`, which is not a date.
    Day(Option<u32>),
    Bool(bool),
    Text(Option<String>),
    Path(Option<String>),
}

impl Value {
    pub fn text(s: impl Into<String>) -> Value {
        Value::Text(Some(s.into()))
    }

    pub fn path(s: impl Into<String>) -> Value {
        Value::Path(Some(s.into()))
    }

    fn kind(&self) -> &'static str {
        match self {
            Value::Money(_) => "money",
            Value::Date(_) => "date",
            Value::Day(_) => "day",
            Value::Bool(_) => "bool",
            Value::Text(_) => "text",
            Value::Path(_) => "path",
        }
    }

    /// Storage-neutral: minor units for money, ISO-8601 for dates. Never the
    /// household's display format, which can change under it.
    fn stored(&self) -> Option<String> {
        match self {
            Value::Money(v) => v.map(|c| c.get().to_string()),
            Value::Date(v) => v.map(|d| d.format("%Y-%m-%d").to_string()),
            Value::Day(v) => v.map(|d| d.to_string()),
            Value::Bool(v) => Some(if *v { "1" } else { "0" }.to_string()),
            Value::Text(v) | Value::Path(v) => v.clone(),
        }
    }
}

struct Field {
    name: String,
    kind: &'static str,
    old: Option<String>,
    new: Option<String>,
}

// ---------------------------------------------------------------------------
// the entry
// ---------------------------------------------------------------------------

/// One user action — one save — plus the fields it actually touched.
#[derive(Debug)]
pub struct Change {
    actor: Actor,
    action: &'static str,
    entity: &'static str,
    entity_id: i64,
    rule_id: Option<i64>,
    period: Option<Period>,
    undo_of: Option<i64>,
    note: Option<String>,
    fields: Vec<Field>,
}

impl std::fmt::Debug for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {:?} -> {:?}", self.name, self.old, self.new)
    }
}

impl Change {
    pub fn transaction(actor: Actor, action: TxAction, id: i64) -> Change {
        Change::new(actor, action.code(), "transaction", id)
    }

    pub fn rule(actor: Actor, action: RuleAction, id: i64) -> Change {
        Change::new(actor, action.code(), "rule", id)
    }

    pub fn file(actor: Actor, action: FileAction, id: i64) -> Change {
        Change::new(actor, action.code(), "file", id)
    }

    pub fn member(actor: Actor, action: MemberAction, id: i64) -> Change {
        Change::new(actor, action.code(), "member", id)
    }

    fn new(actor: Actor, action: &'static str, entity: &'static str, entity_id: i64) -> Change {
        Change {
            actor,
            action,
            entity,
            entity_id,
            rule_id: None,
            period: None,
            undo_of: None,
            note: None,
            fields: Vec::new(),
        }
    }

    /// Stamp the rule behind this row, which is what makes "this occurrence
    /// plus the rule it came from" a single query — exactly what `gl` shows.
    pub fn from_rule(mut self, rule_id: Option<i64>) -> Change {
        self.rule_id = rule_id;
        self
    }

    /// Which month's view this happened in. Null for rule-level changes: a rule
    /// does not live in a month.
    pub fn in_period(mut self, period: Period) -> Change {
        self.period = Some(period);
        self
    }

    /// An undo is an **entry**, never a rewrite: the log reads "deleted", then
    /// "restored · undo of 18:39".
    pub fn undoing(mut self, change_id: i64) -> Change {
        self.undo_of = Some(change_id);
        self
    }

    /// One line, for the cases where the verb alone is not the whole story —
    /// "superseded by receipt upload".
    pub fn note(mut self, note: impl Into<String>) -> Change {
        self.note = Some(note.into());
        self
    }

    /// Record a field **only if it actually changed**. One row per field
    /// touched is the entire reason `change_field` is a second table; writing
    /// unchanged fields would turn every save into noise the pane has to
    /// filter back out.
    pub fn diff(mut self, field: &str, old: Value, new: Value) -> Change {
        debug_assert_eq!(
            old.kind(),
            new.kind(),
            "{field}: both sides of a diff must be the same kind"
        );
        if old != new {
            self.fields.push(Field {
                name: field.to_string(),
                kind: new.kind(),
                old: old.stored(),
                new: new.stored(),
            });
        }
        self
    }

    /// Record a field **even when it did not move**.
    ///
    /// The exception to the rule above, and it has exactly one caller: a
    /// re-attribution's `sort_key`, which `u` needs verbatim to put the row
    /// back where it was. Two members' keys are generated independently, so
    /// the destination's appended key can happen to equal the source's — and
    /// `diff` would then drop the one value the undo depends on, silently and
    /// only sometimes. This field is filtered out of what the log pane reads,
    /// so it stays plumbing rather than becoming something a reader sees.
    pub fn always(mut self, field: &str, old: Value, new: Value) -> Change {
        debug_assert_eq!(
            old.kind(),
            new.kind(),
            "{field}: both sides of a diff must be the same kind"
        );
        self.fields.push(Field {
            name: field.to_string(),
            kind: new.kind(),
            old: old.stored(),
            new: new.stored(),
        });
        self
    }

    /// True when nothing actually moved. A handler can use this to skip writing
    /// an entry for a save that changed nothing — a log full of empty edits is
    /// a log nobody reads.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// Writes the entry and its fields, returning the new `change.id`.
    ///
    /// Takes the caller's transaction rather than the pool: the entry and the
    /// row it describes must commit together, or the log is a record of things
    /// that may or may not have happened.
    pub async fn write(self, tx: &mut Transaction<'_, Sqlite>) -> sqlx::Result<i64> {
        let at = crate::clock::stamp();
        let source = self.actor.source();
        let member_id = self.actor.member_id();
        let period = self.period.map(|p| p.to_string());

        let id = sqlx::query_scalar!(
            r#"INSERT INTO change
                   (at, member_id, source, action, entity, entity_id,
                    rule_id, period, undo_of, note)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
               RETURNING id AS "id!: i64""#,
            at,
            member_id,
            source,
            self.action,
            self.entity,
            self.entity_id,
            self.rule_id,
            period,
            self.undo_of,
            self.note,
        )
        .fetch_one(&mut **tx)
        .await?;

        for f in self.fields {
            sqlx::query!(
                r#"INSERT INTO change_field (change_id, field, old_value, new_value, kind)
                   VALUES (?1, ?2, ?3, ?4, ?5)"#,
                id,
                f.name,
                f.old,
                f.new,
                f.kind,
            )
            .execute(&mut **tx)
            .await?;
        }

        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_records_only_what_moved() {
        let c = Change::transaction(Actor::User(1), TxAction::Edit, 7)
            .diff(
                "amount",
                Value::Money(Some(Cents(34062))),
                Value::Money(Some(Cents(35210))),
            )
            .diff("title", Value::text("Energia"), Value::text("Energia"));
        assert_eq!(c.field_count(), 1);
        assert!(!c.is_empty());
    }

    #[test]
    fn money_is_logged_in_minor_units_not_in_the_household_format() {
        // A stored amount must not depend on the currency the household
        // happens to be configured with when somebody reads the log.
        assert_eq!(
            Value::Money(Some(Cents(35210))).stored().as_deref(),
            Some("35210")
        );
        assert_eq!(Value::Money(None).stored(), None);
    }

    #[test]
    fn dates_are_iso_and_a_day_is_not_a_date() {
        let d = NaiveDate::from_ymd_opt(2026, 9, 11).unwrap();
        assert_eq!(Value::Date(Some(d)).stored().as_deref(), Some("2026-09-11"));
        assert_eq!(Value::Date(Some(d)).kind(), "date");
        // due_day is a day of the month, not a date, and the pane formats the
        // two differently.
        assert_eq!(Value::Day(Some(11)).stored().as_deref(), Some("11"));
        assert_eq!(Value::Day(Some(11)).kind(), "day");
    }

    #[test]
    fn only_a_user_action_carries_an_actor() {
        assert_eq!(Actor::User(3).member_id(), Some(3));
        assert_eq!(Actor::Materializer.member_id(), None);
        assert_eq!(Actor::System.source(), "system");
        assert_eq!(Actor::Import(Some(2)).member_id(), Some(2));
    }

    #[test]
    fn a_rule_delete_uses_the_plain_verb() {
        // There is deliberately no 'rule_delete': deleting a rule is `delete`
        // with entity = 'rule'. Written as a test so it is not "fixed" later.
        assert_eq!(RuleAction::Delete.code(), "delete");
        assert_eq!(RuleAction::Create.code(), "rule_create");
    }
}

// ---------------------------------------------------------------------------
// reading it back — `gl`
// ---------------------------------------------------------------------------

/// Which entries the pane is showing. The chips narrow; they never widen past
/// what this row and its rule can account for.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Filter {
    /// This occurrence and its files — a file entry belongs to the row it is
    /// attached to, it simply is not the transaction record itself.
    #[default]
    Row,
    /// Only the rule behind it.
    Rule,
    /// Both, in date order, so the story reads chronologically.
    All,
}

impl Filter {
    pub fn code(self) -> &'static str {
        match self {
            Filter::Row => "row",
            Filter::Rule => "rule",
            Filter::All => "all",
        }
    }

    pub fn parse(s: &str) -> Filter {
        match s {
            "rule" => Filter::Rule,
            "all" => Filter::All,
            _ => Filter::Row,
        }
    }
}

/// One field of one entry, still in storage form: the pane formats it by
/// `kind` so a logged amount never depends on the household's *current*
/// currency setting.
#[derive(Clone, Debug)]
pub struct FieldEntry {
    pub field: String,
    pub old: Option<String>,
    pub new: Option<String>,
    pub kind: String,
}

/// One save.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: i64,
    /// ISO-8601 UTC, as stored.
    pub at: String,
    /// The member who did it, or `None` for the materializer and the system —
    /// nobody typed those, and the pane says so rather than inventing an actor.
    pub actor: Option<String>,
    pub source: String,
    pub action: String,
    pub entity: String,
    /// True when this is the rule's own history rather than the row's, so the
    /// two are never confused where they sit side by side.
    pub is_rule: bool,
    pub undo_of: Option<i64>,
    pub note: Option<String>,
    pub fields: Vec<FieldEntry>,
}

/// The history behind one row.
///
/// `rule_id` is stamped on transaction changes precisely so this is a single
/// query — "this occurrence plus the rule behind it" rather than two reads and
/// a merge. A file entry counts as the row's own history.
pub async fn history(
    pool: &sqlx::SqlitePool,
    transaction_id: i64,
    rule_id: Option<i64>,
    filter: Filter,
    receipt_ids: &[i64],
) -> sqlx::Result<Vec<Entry>> {
    let want_row = matches!(filter, Filter::Row | Filter::All);
    let want_rule = matches!(filter, Filter::Rule | Filter::All);
    let rule_id = rule_id.unwrap_or(-1);
    // A file entry names a `receipts` row, not the transaction, so the caller
    // passes the ids that belong to this occurrence. Encoded as a JSON array
    // because SQLite has no array parameter and building SQL by hand is how
    // an injection gets written.
    let receipts = serde_json::to_string(receipt_ids).unwrap_or_else(|_| "[]".into());

    let rows = sqlx::query!(
        r#"SELECT c.id AS "id!: i64", c.at AS "at!: String",
                  c.source AS "source!: String", c.action AS "action!: String",
                  c.entity AS "entity!: String",
                  c.undo_of AS "undo_of: i64", c.note AS "note: String",
                  m.name AS "actor: String"
           FROM change c
           LEFT JOIN members m ON m.id = c.member_id
           WHERE (?3 AND ((c.entity = 'transaction' AND c.entity_id = ?1)
                          OR (c.entity = 'file'
                              AND c.entity_id IN (SELECT value FROM json_each(?5)))))
              OR (?4 AND c.entity = 'rule' AND c.entity_id = ?2)
           ORDER BY c.at DESC, c.id DESC"#,
        transaction_id,
        rule_id,
        want_row,
        want_rule,
        receipts,
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let fields = sqlx::query!(
            r#"SELECT field AS "field!: String", old_value AS "old: String",
                      new_value AS "new: String", kind AS "kind!: String"
               FROM change_field WHERE change_id = ?1
                 -- `sort_key` is recorded so an undo can put a re-attributed
                 -- row back where it was, not for a reader: a fractional key
                 -- means nothing to anyone, and showing it would make every
                 -- re-attribution read as a two-field change.
                 AND field <> 'sort_key'
               ORDER BY id"#,
            r.id
        )
        .fetch_all(pool)
        .await?;

        out.push(Entry {
            id: r.id,
            at: r.at,
            actor: r.actor,
            source: r.source,
            is_rule: r.entity == "rule",
            entity: r.entity,
            action: r.action,
            undo_of: r.undo_of,
            note: r.note,
            fields: fields
                .into_iter()
                .map(|f| FieldEntry {
                    field: f.field,
                    old: f.old,
                    new: f.new,
                    kind: f.kind,
                })
                .collect(),
        });
    }
    Ok(out)
}

/// One change's fields, as the log pane reads them — `sort_key` left out for
/// the same reason it is there.
pub async fn fields(pool: &SqlitePool, change_id: i64) -> sqlx::Result<Vec<FieldEntry>> {
    let rows = sqlx::query!(
        r#"SELECT field AS "field!: String", old_value AS "old: String",
                  new_value AS "new: String", kind AS "kind!: String"
           FROM change_field WHERE change_id = ?1 AND field <> 'sort_key'
           ORDER BY id"#,
        change_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|f| FieldEntry {
            field: f.field,
            old: f.old,
            new: f.new,
            kind: f.kind,
        })
        .collect())
}

/// The entry that reversed `change_id` — what an undo or a redo just wrote.
pub async fn reversal_of(pool: &SqlitePool, change_id: i64) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM change WHERE undo_of = ?1
           ORDER BY id DESC LIMIT 1"#,
        change_id
    )
    .fetch_optional(pool)
    .await
}

/// The action an entry reversed: what a redo re-applies.
pub async fn reversed_action(pool: &SqlitePool, change_id: i64) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar!(
        r#"SELECT p.action AS "action!: String"
           FROM change c JOIN change p ON p.id = c.undo_of WHERE c.id = ?1"#,
        change_id
    )
    .fetch_optional(pool)
    .await
}
