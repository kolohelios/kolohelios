//! `blogctl add-target <slug> --target <name>` — register a
//! distribution target on a post.
//!
//! Replaces the "hand-edit the frontmatter" gap that surfaced during
//! #474 (the e2e workflow test had to manually inject a `linkedin`
//! `TargetEntry` so `metrics update` had something to update).
//!
//! v1 fixes the new entry's status at `Planned`. `Published` and
//! `Retracted` are deferred because they legitimately require `url`
//! / `published_at` fields and the use case for "add-as-already-
//! published" hasn't appeared yet; rare cases can hand-edit
//! frontmatter for now. When that flow shows up, add `--status`
//! plus `--url` / `--published-at` alongside.

use std::fs;
use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};
use crate::target::{Target, TargetEntry, TargetStatus};

#[derive(Debug)]
pub struct AddTargetArgs {
    pub slug: String,
    pub workdir: PathBuf,
    pub target: Target,
    pub no_sync: bool,
}

pub fn run(jj: &dyn Jj, args: AddTargetArgs) -> Result<()> {
    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let (handle, mut post) = repo.load_raw(&args.slug)?;

    // Per the parse-time `validate_targets` invariant, each `Target`
    // appears at most once per post. Refuse early with the existing
    // duplicate-target error so the user knows to use `metrics
    // update` (or a future `set-target-status` command) instead of
    // adding a second entry.
    if post.metadata.targets.iter().any(|t| t.name == args.target) {
        return Err(Error::DuplicateTarget {
            path: handle.path.clone(),
            name: args.target,
        });
    }

    post.metadata.targets.push(TargetEntry {
        samples: Vec::new(),
        name: args.target,
        status: TargetStatus::Planned,
        url: None,
        published_at: None,
        metrics: None,
    });

    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, args.no_sync);
    let message = format!("post({}): add-target {}", args.slug, args.target);
    let path = handle.path.clone();
    let target_name = args.target;
    let slug = args.slug.clone();

    sync::commit_and_push(jj, &args.workdir, &opts, &message, || {
        post.metadata.updated_at = OffsetDateTime::now_utc();
        let rendered = post.render()?;
        fs::write(&path, rendered).map_err(|e| Error::io(&path, e))?;
        Ok(())
    })?;
    println!("{slug}: add-target {target_name} (planned)");
    Ok(())
}
