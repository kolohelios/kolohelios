//! Analytics domain logic — pure functions over post + target data
//! that the `blogctl analytics *` commands consume.
//!
//! Kept separate from `commands::analytics` (the CLI layer) so the
//! math has no view of stdout, JSON encoding, or argument parsing —
//! and so the analytics commands route every raw arithmetic operation
//! through this module rather than reinventing it inline.

pub mod derived;

pub use derived::DerivedMetrics;
