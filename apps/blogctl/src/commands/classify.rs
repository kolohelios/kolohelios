//! `blogctl classify <slug>` — set classification dimensions on a
//! post. Stub for now; behavior lands in #437 (taxonomy + classify).

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::sync::Jj;

#[derive(Debug, Default)]
pub struct ClassifyArgs {
    pub slug: String,
    pub workdir: PathBuf,
    pub format: Option<String>,
    pub hook: Option<String>,
    pub tone: Option<String>,
    pub audience: Option<String>,
    pub strategic_role: Option<String>,
    pub theme: Vec<String>,
    pub clear_format: bool,
    pub clear_hook: bool,
    pub clear_tone: bool,
    pub clear_audience: bool,
    pub clear_strategic_role: bool,
    pub clear_theme: bool,
    pub no_sync: bool,
}

pub fn run(_jj: &dyn Jj, _args: ClassifyArgs) -> Result<()> {
    Err(Error::Unimplemented("classify"))
}
