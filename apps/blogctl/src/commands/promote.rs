use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::Result;
use crate::storage::{Repository, Workdir};

pub fn run(slug: String, workdir: PathBuf) -> Result<()> {
    let repo = Repository::open(Workdir::new(&workdir))?;
    let (handle, _post) = repo.load(&slug)?;
    let next = handle.stage.promote()?;
    let new_handle = repo.relocate(&slug, next, OffsetDateTime::now_utc())?;
    println!(
        "{slug}: {} -> {} ({})",
        handle.stage,
        new_handle.stage,
        new_handle.path.display()
    );
    Ok(())
}
