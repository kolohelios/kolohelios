use std::path::PathBuf;

use crate::error::Result;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};

pub fn regenerate(jj: &dyn Jj, workdir: PathBuf, no_sync: bool) -> Result<()> {
    let repo = Repository::open(Workdir::new(&workdir))?;
    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, no_sync);
    let readme_path = repo.workdir().readme_path();

    sync::commit_and_push(
        jj,
        &workdir,
        &opts,
        "docs: regenerate workdir README",
        || {
            repo.write_readme(true)?;
            Ok(())
        },
    )?;
    println!("regenerated {}", readme_path.display());
    Ok(())
}
