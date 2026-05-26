//! `blogctl update` — recovery command that brings the workdir into a
//! known-good shape against the remote without the writer ever touching
//! `jj` directly. Pairs with the #516 guardrail in `sync::commit_and_push`
//! (which surfaces the side-branch state this command recovers from).
//!
//! The sequence:
//! 1. Open the repo (refuse if not initialized).
//! 2. Check `jj.status` — hard-fail if jj missing or workdir isn't a repo.
//! 3. Refuse if `@` has uncommitted edits (would be stranded).
//! 4. Fetch remote.
//! 5. Refuse if local `<bookmark>` has unpushed commits.
//! 6. `jj new <bookmark>@<remote>` — position `@` directly on the remote tip.
//! 7. `jj bookmark set <bookmark> -r <bookmark>@<remote>` — fast-forward
//!    the local bookmark.
//!
//! Each step prints a one-line progress note so it's obvious what
//! happened if a write-shaped command later trips on something else.

use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::storage::{Repository, Workdir};
use crate::sync::{Jj, Status, SyncOptions};

pub fn run(jj: &dyn Jj, workdir: PathBuf, no_sync: bool) -> Result<()> {
    let wd = Workdir::new(&workdir);
    let repo = Repository::open(wd)?;
    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, no_sync);

    if !opts.is_active() {
        return Err(Error::UpdateRequiresSyncEnabled);
    }

    match jj.status(&workdir)? {
        Status::Ok => {}
        Status::JjNotInstalled | Status::NotAJjRepo => {
            return Err(Error::UpdateRequiresJj);
        }
    }

    if !jj.change_is_empty(&workdir, "@")? {
        return Err(Error::UpdateHasUncommittedEdits);
    }

    println!("fetching {}", opts.config.remote);
    jj.fetch(&workdir, &opts.config.remote)?;

    if let Some(summary) = jj.unpushed_summary(
        &workdir,
        &opts.config.bookmark,
        &opts.config.remote,
        OffsetDateTime::now_utc(),
    )? {
        if summary.count > 0 {
            return Err(Error::UpdateHasUnpushedCommits {
                bookmark: opts.config.bookmark.clone(),
                count: summary.count,
            });
        }
    }

    let remote_ref = format!("{}@{}", opts.config.bookmark, opts.config.remote);

    println!("moving @ onto {remote_ref}");
    jj.new_change_at(&workdir, &remote_ref, None)?;

    println!("advancing {} to {remote_ref}", opts.config.bookmark);
    jj.set_bookmark_at(&workdir, &opts.config.bookmark, &remote_ref)?;

    println!("updated: @ is now on top of {remote_ref}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::TempDir;

    use crate::storage::Repository;
    use crate::sync::{Call, FakeJj, Status, UnpushedSummary};

    fn fresh_workdir() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().to_path_buf();
        Repository::unchecked(Workdir::new(&path)).init().unwrap();
        (tmp, path)
    }

    #[test]
    fn run_drives_full_sequence_on_happy_path() {
        let (_tmp, path) = fresh_workdir();
        let jj = FakeJj::new();
        run(&jj, path.clone(), false).unwrap();
        let calls = jj.calls();
        // Status → ChangeIsEmpty(@) → Fetch → UnpushedSummary
        // → NewChangeAt(main@origin, None) → SetBookmarkAt(main, main@origin)
        assert_eq!(calls.len(), 6, "got: {calls:?}");
        assert!(matches!(calls[0], Call::Status { .. }));
        assert!(matches!(
            calls[1],
            Call::ChangeIsEmpty { ref change, .. } if change == "@"
        ));
        assert!(matches!(
            calls[2],
            Call::Fetch { ref remote, .. } if remote == "origin"
        ));
        assert!(matches!(calls[3], Call::UnpushedSummary { .. }));
        assert!(matches!(
            &calls[4],
            Call::NewChangeAt { parent, message, .. }
                if parent == "main@origin" && message.is_none()
        ));
        assert!(matches!(
            &calls[5],
            Call::SetBookmarkAt { bookmark, target, .. }
                if bookmark == "main" && target == "main@origin"
        ));
    }

    #[test]
    fn run_refuses_when_no_sync_flag_set() {
        let (_tmp, path) = fresh_workdir();
        let jj = FakeJj::new();
        let err = run(&jj, path, true).unwrap_err();
        assert!(matches!(err, Error::UpdateRequiresSyncEnabled));
        // Refused before touching jj.
        assert!(jj.calls().is_empty());
    }

    #[test]
    fn run_refuses_when_sync_disabled_in_config() {
        let (_tmp, path) = fresh_workdir();
        // Rewrite .blog-os.toml with sync disabled.
        fs::write(
            path.join(".blog-os.toml"),
            "version = 1\n[sync]\nenabled = false\n",
        )
        .unwrap();
        let jj = FakeJj::new();
        let err = run(&jj, path, false).unwrap_err();
        assert!(matches!(err, Error::UpdateRequiresSyncEnabled));
        assert!(jj.calls().is_empty());
    }

    #[test]
    fn run_refuses_when_jj_not_installed() {
        let (_tmp, path) = fresh_workdir();
        let jj = FakeJj::new().with_status(Status::JjNotInstalled);
        let err = run(&jj, path, false).unwrap_err();
        assert!(matches!(err, Error::UpdateRequiresJj));
    }

    #[test]
    fn run_refuses_when_not_a_jj_repo() {
        let (_tmp, path) = fresh_workdir();
        let jj = FakeJj::new().with_status(Status::NotAJjRepo);
        let err = run(&jj, path, false).unwrap_err();
        assert!(matches!(err, Error::UpdateRequiresJj));
    }

    #[test]
    fn run_refuses_when_at_has_uncommitted_edits() {
        let (_tmp, path) = fresh_workdir();
        let jj = FakeJj::new().with_change_is_empty(false);
        let err = run(&jj, path, false).unwrap_err();
        assert!(matches!(err, Error::UpdateHasUncommittedEdits));
        // Refused before fetch.
        let calls = jj.calls();
        assert!(!calls.iter().any(|c| matches!(c, Call::Fetch { .. })));
    }

    #[test]
    fn run_refuses_when_local_bookmark_has_unpushed_commits() {
        let (_tmp, path) = fresh_workdir();
        let jj = FakeJj::new().with_unpushed_summary(Some(UnpushedSummary {
            count: 3,
            oldest_age_hours: 12,
        }));
        let err = run(&jj, path, false).unwrap_err();
        match err {
            Error::UpdateHasUnpushedCommits { bookmark, count } => {
                assert_eq!(bookmark, "main");
                assert_eq!(count, 3);
            }
            other => panic!("expected UpdateHasUnpushedCommits, got {other:?}"),
        }
        // Refused before NewChangeAt — fetch did fire (we need the
        // unpushed-summary input to be authoritative).
        let calls = jj.calls();
        assert!(!calls.iter().any(|c| matches!(c, Call::NewChangeAt { .. })));
        assert!(!calls
            .iter()
            .any(|c| matches!(c, Call::SetBookmarkAt { .. })));
    }
}
