use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::Result;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};

pub fn run(jj: &dyn Jj, slug: String, workdir: PathBuf, no_sync: bool) -> Result<()> {
    let repo = Repository::open(Workdir::new(&workdir))?;
    let (handle, _post) = repo.load(&slug)?;
    let prev = handle.stage.demote()?;
    let message = format!("post({slug}): {} \u{2190} {prev}", handle.stage);
    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, no_sync);

    let new_handle = sync::commit_and_push(jj, &workdir, &opts, &message, || {
        repo.relocate(&slug, prev, OffsetDateTime::now_utc())
    })?;
    println!(
        "{slug}: {} -> {} ({})",
        handle.stage,
        new_handle.stage,
        new_handle.path.display()
    );
    Ok(())
}
