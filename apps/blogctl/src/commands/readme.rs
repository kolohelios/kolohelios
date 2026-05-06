use std::path::PathBuf;

use crate::error::Result;
use crate::storage::{Repository, Workdir};

pub fn regenerate(workdir: PathBuf) -> Result<()> {
    let repo = Repository::open(Workdir::new(&workdir))?;
    repo.write_readme(true)?;
    println!("regenerated {}", repo.workdir().readme_path().display());
    Ok(())
}
