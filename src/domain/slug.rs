//! Slugifying a taxonomy field into a path segment.
//!
//! `bucket` / `bill_name` / `instance` are free text a household types, and
//! they become directories in the receipt vault. That makes them the one place
//! user input reaches the filesystem, so the rules are narrow on purpose:
//! lowercase, spaces to underscores, and nothing that could escape or collide.

/// Reserved because `.delete/` is where soft-deleted files go. A bucket that
/// slugified to `delete` would put live receipts in the graveyard.
const RESERVED: &[&str] = &["delete"];

/// `"Consumption"` -> `consumption`, `"House 8898"` -> `house_8898`.
///
/// Accents are kept rather than transliterated: a household filing under
/// `Água` gets `água`, which every filesystem this runs on handles, and which
/// stays recognisable when they open the folder themselves — the vault being
/// human-browsable is the entire reason the filesystem is authoritative.
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_sep = true; // leading separators are dropped
    for c in s.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            last_sep = false;
        } else if !last_sep {
            out.push('_');
            last_sep = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

/// Why a taxonomy value cannot be used as a path segment.
#[derive(Debug, PartialEq, Eq)]
pub enum SlugError {
    /// Nothing survived slugification — `"!!!"` is not a folder name.
    Empty,
    /// Collides with the soft-delete folder.
    Reserved,
    /// Would hide the folder, or worse, escape upward.
    Dotted,
}

/// The schema has CHECKs for the `.` prefix and the literal `delete`, but they
/// run on the raw text. This runs on the *slug*, which is what actually reaches
/// the filesystem — `" Delete "` passes the CHECK and fails here.
pub fn path_segment(s: &str) -> Result<String, SlugError> {
    let raw = s.trim();
    if raw.starts_with('.') {
        return Err(SlugError::Dotted);
    }
    let slug = slugify(raw);
    if slug.is_empty() {
        return Err(SlugError::Empty);
    }
    if RESERVED.contains(&slug.as_str()) {
        return Err(SlugError::Reserved);
    }
    Ok(slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_the_examples_from_the_spec() {
        assert_eq!(slugify("Consumption"), "consumption");
        assert_eq!(slugify("Power"), "power");
        assert_eq!(slugify("House 8898"), "house_8898");
        assert_eq!(slugify("company_001"), "company_001");
        assert_eq!(slugify("Energia · Enel SP"), "energia_enel_sp");
    }

    #[test]
    fn collapses_runs_and_trims_edges() {
        assert_eq!(slugify("  many   spaces  "), "many_spaces");
        assert_eq!(slugify("a---b"), "a_b");
        assert_eq!(slugify("!!!leading"), "leading");
        assert_eq!(slugify("trailing!!!"), "trailing");
    }

    #[test]
    fn keeps_accents_so_the_folder_stays_recognisable() {
        // The household opens this directory themselves; `agua` would be a
        // worse name than `água` for someone looking for the water bill.
        assert_eq!(slugify("Água"), "água");
        assert_eq!(slugify("Condomínio"), "condomínio");
    }

    #[test]
    fn refuses_what_would_break_the_vault() {
        assert_eq!(path_segment("delete"), Err(SlugError::Reserved));
        // The schema CHECK compares raw text, so these get past it and have to
        // be caught here, on the slug that actually becomes a directory.
        assert_eq!(path_segment("  Delete  "), Err(SlugError::Reserved));
        assert_eq!(path_segment("DELETE"), Err(SlugError::Reserved));

        assert_eq!(path_segment(".hidden"), Err(SlugError::Dotted));
        assert_eq!(path_segment(".."), Err(SlugError::Dotted));
        assert_eq!(path_segment("!!!"), Err(SlugError::Empty));
        assert_eq!(path_segment("   "), Err(SlugError::Empty));
    }

    #[test]
    fn a_slug_can_never_contain_a_separator() {
        // The property that matters: whatever a household types, it cannot
        // introduce a path component or climb out of its own folder.
        for input in ["a/b", "../../etc/passwd", "a\\b", "x\0y", "a:b"] {
            let s = slugify(input);
            assert!(
                !s.contains('/') && !s.contains('\\') && !s.contains(':') && !s.contains('\0'),
                "{input:?} slugified to {s:?}"
            );
        }
        assert_eq!(slugify("../../etc/passwd"), "etc_passwd");
    }
}
