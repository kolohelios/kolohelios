//! Version-control sync: drives `jj` so every blogctl write turns into
//! a commit and a best-effort push. The `Jj` trait is the only thing
//! the rest of the crate sees; tests inject a `FakeJj` that records
//! calls without shelling out.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::error::{Error, Result};

const JJ: &str = "jj";

/// Workdir-level VCS availability. Read by the per-command sync hook
/// to decide whether to skip silently (`JjNotInstalled`, `NotAJjRepo`)
/// or proceed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Ok,
    JjNotInstalled,
    NotAJjRepo,
}

/// Outcome of a `jj git push`. Push is best-effort — a network or
/// non-fast-forward failure becomes `Failed(reason)`, never an `Err`.
/// Errors are reserved for cases where we couldn't even attempt the
/// push (process-spawn failure with a non-`NotFound` kind).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushOutcome {
    Pushed,
    NothingToPush,
    Failed(String),
}

/// Outcome of `rebase_onto_remote`. Conflicts are first-class in `jj`
/// (the rebase itself succeeds; `@` ends up with conflict markers).
/// We surface the conflict explicitly so the sync hook can hard-fail
/// with a clear message rather than silently push a conflicted commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseOutcome {
    /// `@` was already a descendant of `<bookmark>@<remote>`, or the
    /// rebase landed cleanly.
    Clean,
    /// Rebase completed but produced conflict markers — the user must
    /// resolve via `jj` before blogctl can proceed.
    Conflicted,
}

/// The `jj` operations blogctl needs. Behind a trait so tests can swap
/// in `FakeJj`; the rest of the crate stays oblivious.
///
/// The full per-command flow:
///
/// 1. `status` — bail if `jj` isn't installed or the workdir isn't a
///    repo (skip-with-warn at the call site).
/// 2. `fetch` — pull `<remote>` so we know the latest `<bookmark>@<remote>`.
/// 3. `rebase_onto_remote` — rebase `@` (and any prior-but-unpushed
///    ancestors) onto `<bookmark>@<remote>`. Conflicts hard-fail.
/// 4. `new_change` — create a new empty change on top of `@` with the
///    deterministic message. The pending file write lands inside it.
/// 5. *(command body writes files)*
/// 6. `set_bookmark_to_head` — advance the bookmark to `@`.
/// 7. `push` — best-effort; failure warns but doesn't fail the command.
pub trait Jj: Send + Sync {
    /// Report whether `jj` is installed and the workdir is a `jj`
    /// repo. Used by sync hooks to decide whether to proceed.
    fn status(&self, workdir: &Path) -> Result<Status>;

    /// Fetch `remote`. Pulls remote-tracking refs (including
    /// `<bookmark>@<remote>`) but does not modify local bookmarks.
    fn fetch(&self, workdir: &Path, remote: &str) -> Result<()>;

    /// Rebase `@` (and any of its ancestors back to `<bookmark>`) onto
    /// `<bookmark>@<remote>`. No-op if `@` is already a descendant of
    /// the remote tip. Returns whether the rebase produced conflicts;
    /// callers hard-fail on `Conflicted` rather than continue.
    fn rebase_onto_remote(
        &self,
        workdir: &Path,
        bookmark: &str,
        remote: &str,
    ) -> Result<RebaseOutcome>;

    /// Snapshot any pending working-copy changes into the current `@`
    /// (auto-snapshot), then create a new empty change on top with
    /// `message` as its description. After this returns, `@` is empty
    /// and a subsequent file write lands inside it.
    fn new_change(&self, workdir: &Path, message: &str) -> Result<()>;

    /// Move (or create) `bookmark` to point at `@`. Refuses non-FF
    /// moves — blogctl's flow always advances forward.
    fn set_bookmark_to_head(&self, workdir: &Path, bookmark: &str) -> Result<()>;

    /// Push `bookmark` to `remote`, allowing first-time bookmark
    /// creation on the remote (`--allow-new`). Push is best-effort —
    /// see `PushOutcome`.
    fn push(&self, workdir: &Path, remote: &str, bookmark: &str) -> Result<PushOutcome>;
}

/// Real `jj` binary. Each method shells out via `std::process::Command`;
/// no caching, no batching.
#[derive(Debug, Default)]
pub struct RealJj;

impl Jj for RealJj {
    fn status(&self, workdir: &Path) -> Result<Status> {
        let out = match Command::new(JJ)
            .arg("status")
            .arg("--no-pager")
            .current_dir(workdir)
            .output()
        {
            Ok(o) => o,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Status::JjNotInstalled);
            }
            Err(e) => {
                return Err(Error::JjInvoke {
                    command: format!("{JJ} status"),
                    source: e,
                });
            }
        };
        if out.status.success() {
            return Ok(Status::Ok);
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("There is no jj repo") || stderr.contains("no jj repo") {
            return Ok(Status::NotAJjRepo);
        }
        Err(Error::JjCommandFailed {
            command: format!("{JJ} status"),
            status: out.status.code().unwrap_or(-1),
            stderr: stderr.into_owned(),
        })
    }

    fn fetch(&self, workdir: &Path, remote: &str) -> Result<()> {
        run_strict(workdir, &["git", "fetch", "--remote", remote])?;
        Ok(())
    }

    fn rebase_onto_remote(
        &self,
        workdir: &Path,
        bookmark: &str,
        remote: &str,
    ) -> Result<RebaseOutcome> {
        // Rebase the chain `<bookmark>..@` onto `<bookmark>@<remote>`.
        // If `@` is already a descendant of the remote tip, jj
        // reports "Nothing changed" and exits 0 — that's the Clean
        // no-op path.
        let dest = format!("{bookmark}@{remote}");
        let revset = format!("{bookmark}..@");
        let command_str = format!("{JJ} rebase -s {revset} -d {dest}");
        let out = Command::new(JJ)
            .args(["rebase", "-s", revset.as_str(), "-d", dest.as_str()])
            .current_dir(workdir)
            .output()
            .map_err(|io_err| Error::JjInvoke {
                command: command_str.clone(),
                source: io_err,
            })?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // Empty range or already-up-to-date: jj exits non-zero on
            // some versions; treat the known "no work" phrases as Clean
            // so this method is a true no-op when nothing needs moving.
            if stderr.contains("Nothing changed") || stderr.contains("No revisions to rebase") {
                return Ok(RebaseOutcome::Clean);
            }
            return Err(Error::JjCommandFailed {
                command: command_str,
                status: out.status.code().unwrap_or(-1),
                stderr: stderr.into_owned(),
            });
        }
        // Rebase succeeded — check whether `@` came out conflicted.
        let conflict = Command::new(JJ)
            .args([
                "log",
                "--no-graph",
                "-r",
                "@",
                "-T",
                "if(conflict, \"yes\", \"no\")",
            ])
            .current_dir(workdir)
            .output()
            .map_err(|source| Error::JjInvoke {
                command: format!("{JJ} log -r @ -T conflict"),
                source,
            })?;
        if !conflict.status.success() {
            return Err(Error::JjCommandFailed {
                command: format!("{JJ} log -r @ -T conflict"),
                status: conflict.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&conflict.stderr).into_owned(),
            });
        }
        if String::from_utf8_lossy(&conflict.stdout).trim() == "yes" {
            Ok(RebaseOutcome::Conflicted)
        } else {
            Ok(RebaseOutcome::Clean)
        }
    }

    fn new_change(&self, workdir: &Path, message: &str) -> Result<()> {
        run_strict(workdir, &["new", "--no-edit=false", "-m", message])?;
        Ok(())
    }

    fn set_bookmark_to_head(&self, workdir: &Path, bookmark: &str) -> Result<()> {
        // `bookmark set` creates the bookmark if absent and moves it
        // forward if present. Non-FF moves error without
        // `--allow-backwards`, which is the safety we want.
        run_strict(workdir, &["bookmark", "set", bookmark, "-r", "@"])?;
        Ok(())
    }

    fn push(&self, workdir: &Path, remote: &str, bookmark: &str) -> Result<PushOutcome> {
        let args = [
            "git",
            "push",
            "--remote",
            remote,
            "--bookmark",
            bookmark,
            "--allow-new",
        ];
        let out = match Command::new(JJ).args(args).current_dir(workdir).output() {
            Ok(o) => o,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Lost the binary between `status` and now — unusual,
                // but report as a failure rather than panic.
                return Ok(PushOutcome::Failed("jj not found".into()));
            }
            Err(e) => {
                return Err(Error::JjInvoke {
                    command: format!("{JJ} {}", args.join(" ")),
                    source: e,
                });
            }
        };
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stdout.contains("Nothing changed") || stderr.contains("Nothing changed") {
                Ok(PushOutcome::NothingToPush)
            } else {
                Ok(PushOutcome::Pushed)
            }
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Ok(PushOutcome::Failed(if stderr.is_empty() {
                format!("jj git push exited with status {}", out.status)
            } else {
                stderr
            }))
        }
    }
}

/// Shell `jj <args>` in `workdir`, treating any non-zero exit as a
/// hard error. Used by methods where we've already confirmed the
/// workdir is a `jj` repo (so a failure here is a real problem).
fn run_strict(workdir: &Path, args: &[&str]) -> Result<()> {
    let out = Command::new(JJ)
        .args(args)
        .current_dir(workdir)
        .output()
        .map_err(|source| Error::JjInvoke {
            command: format!("{JJ} {}", args.join(" ")),
            source,
        })?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::JjCommandFailed {
            command: format!("{JJ} {}", args.join(" ")),
            status: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

/// Test double for `Jj`. Records every call as a `Call` so assertions
/// can pin down both the order and the arguments. Outcomes are
/// configurable so push-failure and not-installed paths get covered
/// without real `jj` involvement.
#[derive(Debug, Default)]
pub struct FakeJj {
    inner: Mutex<FakeState>,
}

#[derive(Debug, Default)]
struct FakeState {
    calls: Vec<Call>,
    status: Option<Status>,
    rebase_outcome: Option<RebaseOutcome>,
    push_outcome: Option<PushOutcome>,
}

/// One recorded call against `FakeJj`. Tests pop these and assert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Status {
        workdir: PathBuf,
    },
    Fetch {
        workdir: PathBuf,
        remote: String,
    },
    Rebase {
        workdir: PathBuf,
        bookmark: String,
        remote: String,
    },
    NewChange {
        workdir: PathBuf,
        message: String,
    },
    SetBookmark {
        workdir: PathBuf,
        bookmark: String,
    },
    Push {
        workdir: PathBuf,
        remote: String,
        bookmark: String,
    },
}

impl FakeJj {
    pub fn new() -> Self {
        Self::default()
    }

    /// Force the next `status()` call to return `s`. Defaults to
    /// `Status::Ok` when unset.
    pub fn with_status(self, s: Status) -> Self {
        self.inner.lock().unwrap().status = Some(s);
        self
    }

    /// Force `rebase_onto_remote()` to return `o`. Defaults to
    /// `RebaseOutcome::Clean` when unset.
    pub fn with_rebase_outcome(self, o: RebaseOutcome) -> Self {
        self.inner.lock().unwrap().rebase_outcome = Some(o);
        self
    }

    /// Force the next `push()` call to return `o`. Defaults to
    /// `PushOutcome::Pushed` when unset.
    pub fn with_push_outcome(self, o: PushOutcome) -> Self {
        self.inner.lock().unwrap().push_outcome = Some(o);
        self
    }

    /// Snapshot of recorded calls in order.
    pub fn calls(&self) -> Vec<Call> {
        self.inner.lock().unwrap().calls.clone()
    }
}

impl Jj for FakeJj {
    fn status(&self, workdir: &Path) -> Result<Status> {
        let mut s = self.inner.lock().unwrap();
        s.calls.push(Call::Status {
            workdir: workdir.to_path_buf(),
        });
        Ok(s.status.clone().unwrap_or(Status::Ok))
    }

    fn fetch(&self, workdir: &Path, remote: &str) -> Result<()> {
        self.inner.lock().unwrap().calls.push(Call::Fetch {
            workdir: workdir.to_path_buf(),
            remote: remote.to_string(),
        });
        Ok(())
    }

    fn rebase_onto_remote(
        &self,
        workdir: &Path,
        bookmark: &str,
        remote: &str,
    ) -> Result<RebaseOutcome> {
        let mut s = self.inner.lock().unwrap();
        s.calls.push(Call::Rebase {
            workdir: workdir.to_path_buf(),
            bookmark: bookmark.to_string(),
            remote: remote.to_string(),
        });
        Ok(s.rebase_outcome.clone().unwrap_or(RebaseOutcome::Clean))
    }

    fn new_change(&self, workdir: &Path, message: &str) -> Result<()> {
        self.inner.lock().unwrap().calls.push(Call::NewChange {
            workdir: workdir.to_path_buf(),
            message: message.to_string(),
        });
        Ok(())
    }

    fn set_bookmark_to_head(&self, workdir: &Path, bookmark: &str) -> Result<()> {
        self.inner.lock().unwrap().calls.push(Call::SetBookmark {
            workdir: workdir.to_path_buf(),
            bookmark: bookmark.to_string(),
        });
        Ok(())
    }

    fn push(&self, workdir: &Path, remote: &str, bookmark: &str) -> Result<PushOutcome> {
        let mut s = self.inner.lock().unwrap();
        s.calls.push(Call::Push {
            workdir: workdir.to_path_buf(),
            remote: remote.to_string(),
            bookmark: bookmark.to_string(),
        });
        Ok(s.push_outcome.clone().unwrap_or(PushOutcome::Pushed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn wd() -> PathBuf {
        PathBuf::from("/tmp/blogctl-fake")
    }

    #[test]
    fn fake_records_full_flow_in_order() {
        let jj = FakeJj::new();
        jj.status(&wd()).unwrap();
        jj.fetch(&wd(), "origin").unwrap();
        jj.rebase_onto_remote(&wd(), "main", "origin").unwrap();
        jj.new_change(&wd(), "post(x): draft \"x\"").unwrap();
        jj.set_bookmark_to_head(&wd(), "main").unwrap();
        jj.push(&wd(), "origin", "main").unwrap();
        assert_eq!(
            jj.calls(),
            vec![
                Call::Status { workdir: wd() },
                Call::Fetch {
                    workdir: wd(),
                    remote: "origin".into(),
                },
                Call::Rebase {
                    workdir: wd(),
                    bookmark: "main".into(),
                    remote: "origin".into(),
                },
                Call::NewChange {
                    workdir: wd(),
                    message: "post(x): draft \"x\"".into(),
                },
                Call::SetBookmark {
                    workdir: wd(),
                    bookmark: "main".into(),
                },
                Call::Push {
                    workdir: wd(),
                    remote: "origin".into(),
                    bookmark: "main".into(),
                },
            ]
        );
    }

    #[test]
    fn fake_status_defaults_to_ok() {
        let jj = FakeJj::new();
        assert_eq!(jj.status(&wd()).unwrap(), Status::Ok);
    }

    #[test]
    fn fake_status_can_be_overridden() {
        let jj = FakeJj::new().with_status(Status::JjNotInstalled);
        assert_eq!(jj.status(&wd()).unwrap(), Status::JjNotInstalled);
    }

    #[test]
    fn fake_push_defaults_to_pushed() {
        let jj = FakeJj::new();
        assert_eq!(
            jj.push(&wd(), "origin", "main").unwrap(),
            PushOutcome::Pushed
        );
    }

    #[test]
    fn fake_push_can_be_overridden_with_failure() {
        let jj = FakeJj::new().with_push_outcome(PushOutcome::Failed("network".into()));
        assert_eq!(
            jj.push(&wd(), "origin", "main").unwrap(),
            PushOutcome::Failed("network".into())
        );
    }

    #[test]
    fn fake_push_outcome_persists_across_calls() {
        // The outcome is sticky — once configured, subsequent pushes
        // return the same outcome. Matches how the per-command sync
        // hook will call push once per invocation.
        let jj = FakeJj::new().with_push_outcome(PushOutcome::NothingToPush);
        assert_eq!(
            jj.push(&wd(), "origin", "main").unwrap(),
            PushOutcome::NothingToPush
        );
        assert_eq!(
            jj.push(&wd(), "origin", "main").unwrap(),
            PushOutcome::NothingToPush
        );
    }

    #[test]
    fn fake_rebase_defaults_to_clean() {
        let jj = FakeJj::new();
        assert_eq!(
            jj.rebase_onto_remote(&wd(), "main", "origin").unwrap(),
            RebaseOutcome::Clean
        );
    }

    #[test]
    fn fake_rebase_can_be_overridden_with_conflict() {
        let jj = FakeJj::new().with_rebase_outcome(RebaseOutcome::Conflicted);
        assert_eq!(
            jj.rebase_onto_remote(&wd(), "main", "origin").unwrap(),
            RebaseOutcome::Conflicted
        );
    }
}
