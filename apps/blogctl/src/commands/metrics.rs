//! `blogctl metrics update <slug>` / `blogctl metrics show <slug>` —
//! per-target performance numbers. Stubs for now; behavior lands in
//! #436 (metrics update + show).

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::sync::Jj;
use crate::target::Target;

#[derive(Debug)]
pub struct UpdateArgs {
    pub slug: String,
    pub workdir: PathBuf,
    pub target: Target,
    pub impressions: u64,
    pub reactions: u64,
    pub comments: u64,
    pub reposts: u64,
    /// RFC 3339 timestamp; parsed by the command body. None means
    /// "use now()".
    pub sampled_at: Option<String>,
    pub no_sync: bool,
}

#[derive(Debug)]
pub struct ShowArgs {
    pub slug: String,
    pub workdir: PathBuf,
}

pub fn update(_jj: &dyn Jj, _args: UpdateArgs) -> Result<()> {
    Err(Error::Unimplemented("metrics update"))
}

pub fn show(_args: ShowArgs) -> Result<()> {
    Err(Error::Unimplemented("metrics show"))
}
