//! Types that money and time pass through. No I/O, no SQL, no templates.
//!
//! The split from `ui` is deliberate: `Cents` can be added, `ui::fmt::money`
//! can only be rendered — and it takes a `Currency` but never a `Lang`, which
//! is the three-axis rule expressed as a function signature.

// Scaffolding, phase 1: these types are complete and tested, but the handlers
// and templates that consume them do not exist yet. Blanket-allowing at the
// crate root would hide genuinely dead code later; this is scoped and comes out
// when the web layer lands (phase 2).
#![allow(dead_code)]

pub mod money;
pub mod period;
pub mod slug;
pub mod status;
