//! Analytics domain logic — pure functions over post + target data
//! that the `blogctl analytics *` commands consume.
//!
//! Kept separate from `commands::analytics` (the CLI layer) so the
//! math has no view of stdout, JSON encoding, or argument parsing —
//! and so the analytics commands route every raw arithmetic operation
//! through this module rather than reinventing it inline.

pub mod derived;
pub mod percentile;
pub mod summary;

pub use derived::DerivedMetrics;
pub use percentile::{percentiles_f64, percentiles_u64, Percentiles};
pub use summary::{compute as summary, DimensionSummary, Summary, ValueSummary, LOW_N_THRESHOLD};
