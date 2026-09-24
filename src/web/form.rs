//! Per-field validation errors, and the one rule about where they are shown.
//!
//! **A message belongs next to the field it is about.** The app used to put
//! every failure in one `.warnbox` at the top of the card, which is how a
//! `/setup` submission ended up saying "password too short" above two password
//! boxes that both looked full — the message and the field it described
//! disagreed on screen, and "which box do I retype" is half of fixing it.
//!
//! So a rejected form renders **both**:
//!
//! - the message under its own field, with the field itself marked;
//! - a summary in the card's `.warnbox`, which is what a screen reader
//!   announces on submit and what a long form needs to be scannable.
//!
//! Two places for one message is a real cost, which is why they are generated
//! from one `FieldErrors` rather than written twice.
//!
//! **Focus moves to the first bad field, server-side.** `/login` already did
//! this for its single field; here it is one behaviour for every form, and it
//! needs no JavaScript — the template asks `errors.focus("password")` and
//! renders `autofocus` on exactly one input. A form with no errors autofocuses
//! whatever it normally would.

/// The fields a submission was rejected on, in the order they were checked —
/// which is the order they appear on the form, so the summary reads top to
/// bottom and `focus()` lands on the first one somebody would reach.
#[derive(Clone, Debug, Default)]
pub struct FieldErrors {
    items: Vec<(String, String)>,
}

impl FieldErrors {
    pub fn new() -> FieldErrors {
        FieldErrors::default()
    }

    /// Records a failure. The **first** message for a field wins: validation
    /// runs cheapest-first, so "this is required" should not be replaced by
    /// "this is not a number" for the same empty box.
    pub fn add(&mut self, field: &str, message: impl Into<String>) {
        if self.get(field).is_none() {
            self.items.push((field.to_string(), message.into()));
        }
    }

    /// `add` as a builder step, for the validation chains that read better
    /// as an expression.
    pub fn with(mut self, field: &str, message: impl Into<String>) -> FieldErrors {
        self.add(field, message);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn is_bad(&self) -> bool {
        !self.items.is_empty()
    }

    /// The message for one field, for rendering under it.
    pub fn get(&self, field: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, m)| m.as_str())
    }

    pub fn has(&self, field: &str) -> bool {
        self.get(field).is_some()
    }

    /// True for exactly one field: the first one that failed. The template
    /// renders `autofocus` off this, so the cursor lands on the field the
    /// summary's first line is about.
    pub fn focus(&self, field: &str) -> bool {
        self.items.first().map(|(f, _)| f == field).unwrap_or(false)
    }

    /// True when nothing failed, so a template can keep its ordinary
    /// autofocus without duplicating the "are there errors" test at every
    /// field that might want it.
    pub fn none(&self) -> bool {
        self.items.is_empty()
    }

    /// The card-level summary, in field order.
    pub fn messages(&self) -> Vec<&str> {
        self.items.iter().map(|(_, m)| m.as_str()).collect()
    }
}

/// A number field left blank is **no value**, not a malformed one.
///
/// A form posts every input it holds, including the ones `x-show` has hidden,
/// and an untouched number box posts `""`. Deserialized as a plain
/// `Option<i64>`, that empty string is a parse error — the whole body is
/// refused with a 422 before a single field is checked, and HTMX swaps nothing
/// on a 4xx. That is exactly how the rule pane's Create came to do nothing at
/// all: "Pages in this file" sits hidden under a switch that is off, blank,
/// and it took the submission down with it.
///
/// Anything that is present but not a number is still `None` here; the
/// handler's own validation is what decides whether that is an error worth a
/// message beside the field.
pub fn blank_as_none<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = serde::Deserialize::deserialize(d)?;
    Ok(raw.and_then(|s| s.trim().parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_message_for_a_field_wins() {
        // Validation runs cheapest-first, so "required" must not be
        // overwritten by "not a number" for the same empty box.
        let mut e = FieldErrors::new();
        e.add("amount", "required");
        e.add("amount", "not a number");
        assert_eq!(e.get("amount"), Some("required"));
        assert_eq!(e.messages(), vec!["required"]);
    }

    #[test]
    fn focus_is_the_first_field_and_only_the_first() {
        let mut e = FieldErrors::new();
        e.add("password", "too short");
        e.add("confirm", "does not match");
        assert!(e.focus("password"));
        assert!(!e.focus("confirm"));
        assert!(!e.focus("name"));
    }

    #[test]
    fn a_clean_form_focuses_nothing_and_says_nothing() {
        let e = FieldErrors::new();
        assert!(e.none());
        assert!(!e.focus("anything"));
        assert!(e.messages().is_empty());
    }

    #[test]
    fn the_summary_keeps_the_order_fields_were_checked_in() {
        // Which is the order they sit on the form, so the warnbox reads top
        // to bottom rather than in whatever order a handler happened to run.
        let e = FieldErrors::new()
            .with("household", "required")
            .with("password", "too short");
        assert_eq!(e.messages(), vec!["required", "too short"]);
    }
}
