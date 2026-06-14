// Non-test code must not `.unwrap()`; `not(test)` exempts unit tests,
// and integration tests compile as separate crates (no attribute).
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod analytics;
pub mod classifications;
pub mod cli;
pub mod commands;
pub mod datetime;
pub mod error;
pub mod fetch;
pub mod kind;
pub mod linkedin;
pub mod openrouter;
pub mod post;
pub mod predicate;
pub mod slug;
pub mod stage;
pub mod storage;
pub mod sync;
pub mod target;
pub mod taxonomy;

pub use analytics::DerivedMetrics;
pub use classifications::Classifications;
pub use error::{Error, Result};
pub use fetch::{FakeFetcher, Fetcher, UreqFetcher};
pub use kind::Kind;
pub use linkedin::{FetchedPost, PostSnapshot};
pub use post::{Post, PostMetadata};
pub use stage::Stage;
pub use storage::{Repository, Workdir};
pub use target::{MetricSample, Target, TargetEntry, TargetStatus};
pub use taxonomy::{Dimension, Taxonomy};
