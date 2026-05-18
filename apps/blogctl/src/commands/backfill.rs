//! `blogctl backfill` — interactive walk of published posts to fill
//! missing classifications + metrics, or batch import from a JSON
//! file. Stub for now; behavior lands in #435 (backfill).

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::sync::Jj;

#[derive(Debug)]
pub struct BackfillArgs {
    pub workdir: PathBuf,
    /// Path to a JSON file of `{ slug → classifications/metrics }`
    /// entries. When set, runs in batch mode (no prompts).
    pub import: Option<PathBuf>,
    pub no_sync: bool,
}

pub fn run(_jj: &dyn Jj, _args: BackfillArgs) -> Result<()> {
    Err(Error::Unimplemented("backfill"))
}
