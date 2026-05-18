//! `blogctl analytics {summary, compare, recommendations}` — read
//! every published post's classifications + metrics and surface
//! aggregates. Stubs for now; behavior lands in #439/#440/#441.

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::target::Target;

#[derive(Debug)]
pub struct SummaryArgs {
    pub workdir: PathBuf,
    pub target: Option<Target>,
    pub dimension: Option<String>,
    pub json: bool,
}

#[derive(Debug)]
pub struct CompareArgs {
    pub dim_a: String,
    pub dim_b: String,
    pub workdir: PathBuf,
    pub target: Option<Target>,
    pub min_n: usize,
    pub json: bool,
}

#[derive(Debug)]
pub struct RecommendationsArgs {
    pub workdir: PathBuf,
    pub target: Option<Target>,
    pub min_n: usize,
}

pub fn summary(_args: SummaryArgs) -> Result<()> {
    Err(Error::Unimplemented("analytics summary"))
}

pub fn compare(_args: CompareArgs) -> Result<()> {
    Err(Error::Unimplemented("analytics compare"))
}

pub fn recommendations(_args: RecommendationsArgs) -> Result<()> {
    Err(Error::Unimplemented("analytics recommendations"))
}
