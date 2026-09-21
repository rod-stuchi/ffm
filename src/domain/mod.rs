//! Types that money and time pass through. No I/O, no SQL, no templates.
//!
//! The split from `ui` is deliberate: `Cents` can be added, `ui::fmt::money`
//! can only be rendered — and it takes a `Currency` but never a `Lang`, which
//! is the three-axis rule expressed as a function signature.

pub mod money;
pub mod period;
pub mod receipt_path;
pub mod slug;
pub mod sortkey;
pub mod status;
pub mod viewer;
