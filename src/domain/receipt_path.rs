//! Where a tracked occurrence's files live.
//!
//! Fully computed, never authored. No regex, no pattern, no traversal: the path
//! is a function of the rule's taxonomy and the occurrence's period, so
//! checking whether a file exists is one `exists()` on one exact path rather
//! than a directory walk.
//!
//! **This is the only implementation.** The row renders its file rail from it
//! and the store writes to it; two builders that agreed on Tuesday is exactly
//! how a vault ends up with files the app cannot see.

use crate::domain::money::{Cents, Currency};
use crate::domain::period::Period;
use crate::domain::slug::slugify;
use crate::ui::fmt;

/// The closed, case-insensitive set. Whatever was uploaded, not a choice made
/// per rule — all four already render natively in a browser, which is why no
/// custom file renderer is built for them.
pub const EXTENSIONS: [&str; 4] = ["pdf", "jpg", "png", "txt"];

pub fn is_allowed_extension(ext: &str) -> bool {
    let lower = ext.to_ascii_lowercase();
    EXTENSIONS.contains(&lower.as_str())
}

/// The three kinds, each a separate fact about a period.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    /// `{month}_barcode.{ext}` — the bill as issued by the company. Purely
    /// informational: its presence never changes the paid/scheduled state.
    Barcode,
    /// `{month}_scheduled.{ext}` — optional proof a payment was set up ahead
    /// of the due date. A same-day payment skips it entirely.
    Scheduled,
    /// `{month}.{ext}` — the definitive receipt. Its presence *is* paid.
    Receipt,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Barcode, Kind::Scheduled, Kind::Receipt];

    pub fn code(self) -> &'static str {
        match self {
            Kind::Barcode => "barcode",
            Kind::Scheduled => "scheduled",
            Kind::Receipt => "receipt",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "barcode" => Some(Kind::Barcode),
            "scheduled" => Some(Kind::Scheduled),
            "receipt" => Some(Kind::Receipt),
            _ => None,
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Kind::Barcode => "_barcode",
            Kind::Scheduled => "_scheduled",
            // None: the bare name is what `paid` means.
            Kind::Receipt => "",
        }
    }
}

/// The member's folder in the vault: their slug, and nothing else.
///
/// It was `{uuid}-{slug}`, on the theory that a rename would otherwise move
/// the folder. But names are `UNIQUE` and cannot be changed, and slugs are
/// `UNIQUE` too, so the uuid bought nothing except a path nobody could read
/// at a glance — in a vault that exists to be browsed by hand. One function,
/// so the rule is written once.
pub fn vault(slug: &str) -> String {
    slug.to_string()
}

/// Everything a path needs. Gathered into one struct because a function taking
/// six strings in a row is a function whose arguments get swapped.
#[derive(Clone, Debug)]
pub struct Taxonomy<'a> {
    /// `{slug}` — the member's folder, from [`vault`]. Files live under their owner
    /// regardless of whether two members happen to use the same bucket name.
    pub vault: &'a str,
    pub bucket: &'a str,
    pub bill_name: &'a str,
    /// Absent when there is only one of something: "Power" alone is enough,
    /// and inventing a name for a distinction that does not exist is noise.
    pub instance: Option<&'a str>,
}

/// The folder a period's files live in, without the filename.
pub fn folder(tax: &Taxonomy<'_>, period: Period) -> String {
    let mut parts = vec![
        tax.vault.to_string(),
        slugify(tax.bucket),
        slugify(tax.bill_name),
    ];
    if let Some(instance) = tax.instance.filter(|i| !i.trim().is_empty()) {
        parts.push(slugify(instance));
    }
    parts.push(period.path_year());
    parts.join("/")
}

/// The full relative path for one file.
///
/// `amount` is only ever consulted for the receipt, and only when the rule
/// opted into it: `09.pdf` becomes `09_brl122.94.pdf`. A period with no amount
/// yet keeps the plain name and picks the suffix up later — writing
/// `09_brl0.00.pdf` would put a number in a filename that was never owed.
pub fn file(
    tax: &Taxonomy<'_>,
    period: Period,
    kind: Kind,
    ext: &str,
    amount: Option<(Cents, Currency)>,
) -> String {
    let mut name = period.path_month();
    if kind == Kind::Receipt
        && let Some((cents, currency)) = amount
    {
        name.push('_');
        name.push_str(&fmt::money_filename(cents, currency));
    }
    name.push_str(kind.suffix());
    name.push('.');
    name.push_str(&ext.to_ascii_lowercase());
    format!("{}/{}", folder(tax, period), name)
}

/// Where a soft-deleted file goes: a `.delete/` subfolder beside where it
/// lived. Never off disk — the same "nothing is destroyed" rule the ledger
/// follows, which is also why `delete` and a leading `.` are reserved
/// taxonomy values.
///
/// This is the **legacy** name, with no way to tell two deletes of the same
/// slot apart — the second one overwrote the first. It is only still read, so
/// a file deleted before [`deleted_path_for`] existed can be restored.
pub fn deleted_path(original: &str) -> String {
    match original.rsplit_once('/') {
        Some((dir, name)) => format!("{dir}/.delete/{name}"),
        None => format!(".delete/{original}"),
    }
}

/// Where one particular soft-deleted file goes: `.delete/` beside where it
/// lived, named after its own `receipts` row — `09_barcode.r42.pdf`.
///
/// Unique per row, which is the whole point. A slot is re-filled and deleted
/// again (a bill replaced twice, a receipt re-uploaded after a delete), and a
/// fixed `.delete/09_barcode.pdf` let the second delete overwrite the first —
/// a hard delete wearing a soft delete's name. The extension stays last so
/// the file still opens by double-click in a vault browsed by hand.
pub fn deleted_path_for(original: &str, receipt_id: i64) -> String {
    let (dir, name) = match original.rsplit_once('/') {
        Some((dir, name)) => (format!("{dir}/"), name),
        None => (String::new(), original),
    };
    let name = match name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.r{receipt_id}.{ext}"),
        None => format!("{name}.r{receipt_id}"),
    };
    format!("{dir}.delete/{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_deleted_file_gets_a_name_of_its_own() {
        assert_eq!(
            deleted_path_for("udo/mobilidade/moto/2026/09_barcode.pdf", 3),
            "udo/mobilidade/moto/2026/.delete/09_barcode.r3.pdf"
        );
        assert_ne!(
            deleted_path_for("a/09.pdf", 3),
            deleted_path_for("a/09.pdf", 4)
        );
    }

    fn tax() -> Taxonomy<'static> {
        Taxonomy {
            vault: "7c3f-rodrigo",
            bucket: "Consumo",
            bill_name: "Energia",
            instance: None,
        }
    }

    fn sep() -> Period {
        Period::parse("2026-09").unwrap()
    }

    #[test]
    fn the_three_kinds_differ_only_in_their_suffix() {
        assert_eq!(
            file(&tax(), sep(), Kind::Receipt, "pdf", None),
            "7c3f-rodrigo/consumo/energia/2026/09.pdf"
        );
        assert_eq!(
            file(&tax(), sep(), Kind::Scheduled, "pdf", None),
            "7c3f-rodrigo/consumo/energia/2026/09_scheduled.pdf"
        );
        assert_eq!(
            file(&tax(), sep(), Kind::Barcode, "pdf", None),
            "7c3f-rodrigo/consumo/energia/2026/09_barcode.pdf"
        );
    }

    #[test]
    fn the_instance_segment_is_absent_rather_than_empty() {
        let mut t = tax();
        t.instance = Some("House 33A");
        assert_eq!(
            file(&t, sep(), Kind::Receipt, "pdf", None),
            "7c3f-rodrigo/consumo/energia/house_33a/2026/09.pdf"
        );
        // Whitespace is not an instance.
        t.instance = Some("   ");
        assert!(!file(&t, sep(), Kind::Receipt, "pdf", None).contains("//"));
    }

    #[test]
    fn the_amount_suffix_is_receipt_only_and_needs_a_real_number() {
        let money = Some((Cents(12294), Currency::Brl));
        assert_eq!(
            file(&tax(), sep(), Kind::Receipt, "pdf", money),
            "7c3f-rodrigo/consumo/energia/2026/09_brl122.94.pdf"
        );
        // The app's own suffixes stay plain, so the three kinds remain
        // recognisable at a glance.
        assert_eq!(
            file(&tax(), sep(), Kind::Scheduled, "pdf", money),
            "7c3f-rodrigo/consumo/energia/2026/09_scheduled.pdf"
        );
        // No amount yet means no suffix — never a placeholder zero.
        assert_eq!(
            file(&tax(), sep(), Kind::Receipt, "pdf", None),
            "7c3f-rodrigo/consumo/energia/2026/09.pdf"
        );
    }

    #[test]
    fn nothing_in_a_name_needs_shell_quoting() {
        // The vault is meant to be browsed and moved around by hand, so
        // `cp .../09_brl9.99.pdf` has to just work.
        let p = file(
            &tax(),
            sep(),
            Kind::Receipt,
            "PDF",
            Some((Cents(999), Currency::Brl)),
        );
        assert!(
            p.chars().all(|c| c.is_alphanumeric() || "._-/".contains(c)),
            "{p}"
        );
        assert!(p.ends_with(".pdf"), "the extension is normalised: {p}");
    }

    #[test]
    fn a_soft_deleted_file_moves_beside_where_it_lived() {
        assert_eq!(
            deleted_path("7c3f-rodrigo/consumo/energia/2026/09.pdf"),
            "7c3f-rodrigo/consumo/energia/2026/.delete/09.pdf"
        );
    }

    #[test]
    fn the_extension_set_is_closed_and_case_insensitive() {
        assert!(is_allowed_extension("PDF"));
        assert!(is_allowed_extension("jpg"));
        assert!(!is_allowed_extension("exe"));
        assert!(!is_allowed_extension(""));
    }
}
