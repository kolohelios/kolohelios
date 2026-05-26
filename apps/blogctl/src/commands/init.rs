use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};

pub fn run(jj: &dyn Jj, workdir: PathBuf, no_sync: bool) -> Result<()> {
    // Short-circuit before the sync orchestration so the user sees
    // `WorkdirAlreadyInitialized` rather than whatever jj precheck
    // happens to fail first (conflicted bookmark, missing remote, …).
    let wd = Workdir::new(&workdir);
    if wd.config_path().exists() {
        return Err(Error::WorkdirAlreadyInitialized(wd.root().to_path_buf()));
    }

    // `init` creates `.blog-os.toml`, so we have no config to read
    // SyncConfig from yet. Use the built-in defaults; if the user
    // wants different sync settings they can edit the file after.
    let opts = SyncOptions::with_defaults(no_sync);
    sync::commit_and_push(jj, &workdir, &opts, "chore: scaffold workdir", || {
        let repo = Repository::unchecked(Workdir::new(&workdir));
        repo.init()
    })?;
    println!("initialized blogctl workdir at {}", workdir.display());
    Ok(())
}
