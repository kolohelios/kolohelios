use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};

pub fn run(jj: &dyn Jj, slug: String, workdir: PathBuf, no_sync: bool) -> Result<()> {
    let repo = Repository::open(Workdir::new(&workdir))?;
    let (handle, post) = repo.load(&slug)?;
    let next = handle.stage.promote()?;
    let config = repo.read_config()?;

    // Gate on the current stage's exit predicates, if any are configured.
    // Absent config or empty `exit` means "no gate" — the predicate
    // language is opt-in per stage.
    if let Some(stage_cfg) = config.stage_config(post.metadata.kind, handle.stage) {
        for predicate in &stage_cfg.exit {
            if !predicate.evaluate(&post)? {
                return Err(Error::PromoteBlocked {
                    slug: slug.clone(),
                    from: handle.stage,
                    to: next,
                    predicate: predicate.to_string(),
                });
            }
        }
    }

    let message = format!("post({slug}): {} \u{2192} {next}", handle.stage);
    let opts = SyncOptions::from_config(&config.sync, no_sync);

    let new_handle = sync::commit_and_push(jj, &workdir, &opts, &message, || {
        repo.relocate(&slug, next, OffsetDateTime::now_utc())
    })?;
    println!(
        "{slug}: {} -> {} ({})",
        handle.stage,
        new_handle.stage,
        new_handle.path.display()
    );
    Ok(())
}
