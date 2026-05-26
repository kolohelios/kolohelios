//! Version-control sync: drives `jj` so every blogctl write turns into
//! a commit and a best-effort push. The `Jj` trait is the only thing
//! the rest of the crate sees; tests inject a `FakeJj` that records
//! calls without shelling out.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::storage::SyncConfig;

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

/// Summary of commits on `<bookmark>` that are not yet on
/// `<bookmark>@<remote>`. Surfaced by `doctor` to catch the case
/// where auto-push has been silently failing for a while.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpushedSummary {
    /// How many commits are on the local bookmark but not the remote.
    pub count: usize,
    /// Hours since the oldest of those commits was authored.
    pub oldest_age_hours: i64,
}

/// The `jj` operations blogctl needs. Behind a trait so tests can swap
/// in `FakeJj`; the rest of the crate stays oblivious.
///
/// The full per-command flow:
///
/// 1. `status` — bail if `jj` isn't installed or the workdir isn't a
///    repo (skip-with-warn at the call site).
/// 2. `other_bookmarks_in_range` — refuse if `@`'s ancestry crosses an
///    unrelated branch tip; advancing `<bookmark>` would otherwise yank
///    those commits along.
/// 3. `fetch` — pull `<remote>` so we know the latest `<bookmark>@<remote>`.
/// 4. `rebase_onto_remote` — rebase `@` (and any prior-but-unpushed
///    ancestors) onto `<bookmark>@<remote>`. Conflicts hard-fail.
/// 5. `new_change` — create a new empty change on top of `@` with the
///    deterministic message. The pending file write lands inside it.
/// 6. *(command body writes files)*
/// 7. `set_bookmark_to_head` — advance the bookmark to `@`.
/// 8. `push` — best-effort; failure warns but doesn't fail the command.
pub trait Jj: Send + Sync {
    /// Report whether `jj` is installed and the workdir is a `jj`
    /// repo. Used by sync hooks to decide whether to proceed.
    fn status(&self, workdir: &Path) -> Result<Status>;

    /// Names of local bookmarks (other than `bookmark` itself) that sit
    /// on commits in `bookmark..@`. A non-empty result means `@`'s
    /// ancestry crosses an unrelated branch tip — pushing would yank
    /// those commits onto `bookmark`, which is almost never what the
    /// caller wants. Returns an empty vec when `bookmark` doesn't yet
    /// exist locally (brand-new workdir before the first write).
    fn other_bookmarks_in_range(&self, workdir: &Path, bookmark: &str) -> Result<Vec<String>>;

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

    /// Summarize commits on `<bookmark>` that are not yet on
    /// `<bookmark>@<remote>`. Returns `None` when there are no
    /// unpushed commits or when the remote ref doesn't exist (a
    /// brand-new bookmark before its first successful push). `now`
    /// is injected so tests are deterministic.
    fn unpushed_summary(
        &self,
        workdir: &Path,
        bookmark: &str,
        remote: &str,
        now: OffsetDateTime,
    ) -> Result<Option<UnpushedSummary>>;

    /// Whether the named change has no diff against its parent. Used
    /// by `blogctl update` to refuse moving `@` when doing so would
    /// strand uncommitted edits.
    fn change_is_empty(&self, workdir: &Path, change: &str) -> Result<bool>;

    /// Create a new empty change with `parent` as its parent. `message`
    /// is optional — `None` leaves the new change without a description.
    /// Companion to `new_change`, which always parents on `@`.
    fn new_change_at(&self, workdir: &Path, parent: &str, message: Option<&str>) -> Result<()>;

    /// Move (or create) `bookmark` to point at the revision `target`.
    /// Companion to `set_bookmark_to_head`, which always targets `@`.
    fn set_bookmark_at(&self, workdir: &Path, bookmark: &str, target: &str) -> Result<()>;
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

    fn other_bookmarks_in_range(&self, workdir: &Path, bookmark: &str) -> Result<Vec<String>> {
        // `local_bookmarks` excludes `<name>@<remote>` tracking refs, so
        // we don't surface `main@origin` as a "side branch". One name
        // per line keeps parsing trivial when a commit carries multiple
        // bookmarks.
        let revset = format!("{bookmark}..@");
        let template = "local_bookmarks.map(|b| b.name()).join(\"\\n\") ++ \"\\n\"";
        let command_str = format!("{JJ} log -r {revset} -T <local_bookmarks>");
        let out = Command::new(JJ)
            .args(["log", "-r", revset.as_str(), "--no-graph", "-T", template])
            .current_dir(workdir)
            .output()
            .map_err(|source| Error::JjInvoke {
                command: command_str.clone(),
                source,
            })?;
        if !out.status.success() {
            // Brand-new workdir: `<bookmark>` hasn't been created
            // locally yet, so the revset errors. Treat as "no side
            // branches" so the first write can proceed; downstream
            // `set_bookmark_to_head` will create the bookmark.
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("No such revision")
                || stderr.contains("doesn't exist")
                || stderr.contains("Revision") && stderr.contains("not found")
            {
                return Ok(Vec::new());
            }
            if let Some(candidates) = parse_conflicted_bookmark_stderr(&stderr, bookmark) {
                return Err(Error::JjBookmarkConflicted {
                    bookmark: bookmark.to_string(),
                    candidates,
                });
            }
            return Err(Error::JjCommandFailed {
                command: command_str,
                status: out.status.code().unwrap_or(-1),
                stderr: stderr.into_owned(),
            });
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut names: Vec<String> = stdout
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != bookmark)
            .map(str::to_string)
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
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
            if let Some(candidates) = parse_conflicted_bookmark_stderr(&stderr, bookmark) {
                return Err(Error::JjBookmarkConflicted {
                    bookmark: bookmark.to_string(),
                    candidates,
                });
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
        // `--no-edit` is a bare boolean flag in current jj; the old
        // `--no-edit=false` syntax errors with "unexpected value".
        // Default `jj new` already moves @ to the new commit, which
        // is exactly what we want here.
        run_strict(workdir, &["new", "-m", message])?;
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

    fn unpushed_summary(
        &self,
        workdir: &Path,
        bookmark: &str,
        remote: &str,
        now: OffsetDateTime,
    ) -> Result<Option<UnpushedSummary>> {
        // Revset: commits reachable from <bookmark> but not from
        // <bookmark>@<remote>. Template prints unix epoch seconds for
        // each commit's author timestamp, one per line.
        let revset = format!("{bookmark}@{remote}..{bookmark}");
        let template = "author.timestamp().format(\"%s\") ++ \"\\n\"";
        let out = Command::new(JJ)
            .args(["log", "-r", revset.as_str(), "--no-graph", "-T", template])
            .current_dir(workdir)
            .output()
            .map_err(|source| Error::JjInvoke {
                command: format!("{JJ} log -r {revset} -T <author-ts>"),
                source,
            })?;
        if !out.status.success() {
            // A conflicted bookmark is a real, actionable error — surface
            // it rather than silently treating it as "no unpushed info".
            let stderr = String::from_utf8_lossy(&out.stderr);
            if let Some(candidates) = parse_conflicted_bookmark_stderr(&stderr, bookmark) {
                return Err(Error::JjBookmarkConflicted {
                    bookmark: bookmark.to_string(),
                    candidates,
                });
            }
            // Otherwise this usually means <bookmark>@<remote> doesn't
            // exist yet (first push pending). Treat as "no
            // unpushed-from-remote info"; doctor's other findings will
            // still surface real problems.
            return Ok(None);
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut timestamps: Vec<i64> = stdout
            .lines()
            .filter_map(|s| s.trim().parse::<i64>().ok())
            .collect();
        if timestamps.is_empty() {
            return Ok(None);
        }
        timestamps.sort_unstable();
        let oldest = OffsetDateTime::from_unix_timestamp(timestamps[0]).unwrap_or(now);
        let age = now - oldest;
        Ok(Some(UnpushedSummary {
            count: timestamps.len(),
            oldest_age_hours: age.whole_hours(),
        }))
    }

    fn change_is_empty(&self, workdir: &Path, change: &str) -> Result<bool> {
        // `jj diff --summary -r <change>` lists one path per line for
        // any add/modify/delete/rename. Empty stdout = no diff.
        let command_str = format!("{JJ} diff --summary -r {change}");
        let out = Command::new(JJ)
            .args(["diff", "--summary", "-r", change])
            .current_dir(workdir)
            .output()
            .map_err(|source| Error::JjInvoke {
                command: command_str.clone(),
                source,
            })?;
        if !out.status.success() {
            return Err(Error::JjCommandFailed {
                command: command_str,
                status: out.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        Ok(out.stdout.iter().all(u8::is_ascii_whitespace))
    }

    fn new_change_at(&self, workdir: &Path, parent: &str, message: Option<&str>) -> Result<()> {
        let mut args: Vec<&str> = vec!["new", parent];
        if let Some(m) = message {
            args.extend(["-m", m]);
        }
        run_strict(workdir, &args)?;
        Ok(())
    }

    fn set_bookmark_at(&self, workdir: &Path, bookmark: &str, target: &str) -> Result<()> {
        run_strict(workdir, &["bookmark", "set", bookmark, "-r", target])?;
        Ok(())
    }
}

/// Per-invocation sync state: a `SyncConfig` (from `.blog-os.toml`)
/// plus the per-command `--no-sync` flag. Built once at the entry of
/// each write-shaped command and passed to `commit_and_push`.
#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub config: SyncConfig,
    pub no_sync_flag: bool,
}

impl SyncOptions {
    pub fn from_config(config: &SyncConfig, no_sync_flag: bool) -> Self {
        Self {
            config: config.clone(),
            no_sync_flag,
        }
    }

    /// Use built-in defaults — for `init`, which runs before
    /// `.blog-os.toml` exists.
    pub fn with_defaults(no_sync_flag: bool) -> Self {
        Self {
            config: SyncConfig::default(),
            no_sync_flag,
        }
    }

    pub fn is_active(&self) -> bool {
        self.config.enabled && !self.no_sync_flag
    }
}

/// Orchestrate one write-shaped blogctl command's commit-and-push:
///
/// 1. `status` — if `jj` is missing or workdir isn't a repo, warn to
///    stderr, then just run `write` and return.
/// 2. `other_bookmarks_in_range` — refuse with `WorkdirOnSideBranch` if
///    `@`'s ancestry crosses an unrelated branch tip. Otherwise
///    `set_bookmark_to_head` would yank those commits onto `<bookmark>`
///    and `jj git push` would reject whichever empty/no-description
///    commit is in the range.
/// 3. `fetch` — pull the latest `<remote>`. Fetch failure (offline,
///    no remote configured) warns and skips the rebase, then proceeds
///    with `new_change` / write / push as usual; push may also fail,
///    that's fine.
/// 4. `rebase_onto_remote` — rebase `@` onto `<bookmark>@<remote>`.
///    Conflicts are a hard error: the user must resolve before
///    blogctl can continue, otherwise we'd push a conflicted commit.
/// 5. `new_change(message)` — empty change with the deterministic
///    template-built message.
/// 6. `write` — runs the caller's write closure inside the new
///    change. `jj` auto-snapshots the resulting working-copy state.
/// 7. `set_bookmark_to_head` — advance the bookmark to `@`.
/// 8. `push` — best-effort. `Failed` warns to stderr; the command
///    still exits 0 because the commit is safely local.
///
/// If `opts.is_active()` is false (`enabled = false` or `--no-sync`),
/// the entire jj orchestration is skipped — `write` runs and returns
/// whatever it produced.
pub fn commit_and_push<F, T>(
    jj: &dyn Jj,
    workdir: &Path,
    opts: &SyncOptions,
    message: &str,
    write: F,
) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    if !opts.is_active() {
        return write();
    }

    match jj.status(workdir)? {
        Status::Ok => {}
        Status::JjNotInstalled => {
            eprintln!("warning: `jj` not on PATH; skipping VCS sync (file write only)");
            return write();
        }
        Status::NotAJjRepo => {
            eprintln!(
                "warning: {} is not a `jj` repo; skipping VCS sync (file write only)",
                workdir.display(),
            );
            eprintln!("hint: run `jj git init --colocate` in this workdir to enable sync");
            return write();
        }
    }

    // Refuse to advance the bookmark if `@`'s ancestry crosses an
    // unrelated branch tip — otherwise `set_bookmark_to_head` would
    // silently yank those commits onto `<bookmark>` and the push would
    // reject whichever empty/no-description commit is in the range.
    let side_bookmarks = jj.other_bookmarks_in_range(workdir, &opts.config.bookmark)?;
    if !side_bookmarks.is_empty() {
        return Err(Error::WorkdirOnSideBranch {
            bookmark: opts.config.bookmark.clone(),
            side_bookmarks,
        });
    }

    let fetch_ok = match jj.fetch(workdir, &opts.config.remote) {
        Ok(()) => true,
        Err(e) => {
            // Offline, unconfigured remote, or transient network failure.
            // Skip the rebase and continue — push may also fail, but
            // the working copy will still be committed locally.
            // The "no remote configured" case is the common pre-first-push
            // state of a fresh workdir; surfacing it as a warning every
            // write is noise the user can't act on.
            if !cause_is_missing_remote(&e) {
                eprintln!(
                    "warning: fetch from {} failed: {e}; skipping rebase",
                    opts.config.remote
                );
            }
            false
        }
    };

    if fetch_ok {
        match jj.rebase_onto_remote(workdir, &opts.config.bookmark, &opts.config.remote)? {
            RebaseOutcome::Clean => {}
            RebaseOutcome::Conflicted => {
                return Err(Error::SyncRebaseConflict {
                    bookmark: opts.config.bookmark.clone(),
                    remote: opts.config.remote.clone(),
                });
            }
        }
    }

    jj.new_change(workdir, message)?;
    let result = write()?;
    jj.set_bookmark_to_head(workdir, &opts.config.bookmark)?;

    match jj.push(workdir, &opts.config.remote, &opts.config.bookmark)? {
        PushOutcome::Pushed | PushOutcome::NothingToPush => {}
        PushOutcome::Failed(reason) => {
            // Same suppression as the fetch path: a fresh workdir with
            // no configured remote will surface this on every write.
            if !reason.contains("No git remotes") {
                eprintln!(
                    "warning: push to {}/{} failed: {reason}",
                    opts.config.remote, opts.config.bookmark,
                );
                eprintln!("hint: commit is local; retry with `jj git push` when ready");
            }
        }
    }

    Ok(result)
}

/// True when `err` represents `jj git fetch` failing because the
/// configured remote doesn't exist in the colocated `.git`. Surfaced
/// so the sync hook can suppress the otherwise-noisy warning for the
/// common pre-first-push state.
fn cause_is_missing_remote(err: &Error) -> bool {
    matches!(
        err,
        Error::JjCommandFailed { stderr, .. } if stderr.contains("No git remotes")
    )
}

/// Detect `jj`'s "conflicted bookmark" error in stderr and pull the
/// candidate commit IDs out of the accompanying hint. `jj` emits:
///
/// ```text
/// Error: Name `main` is conflicted
/// Hint: Use commit ID to select single revision from: e1214238e693, e92e7587f4f1
/// ```
///
/// Returns `Some(candidates)` when stderr names `bookmark` as
/// conflicted (the vec is empty if the hint line is absent or
/// unparsable — still a conflict). Returns `None` when stderr is
/// unrelated.
fn parse_conflicted_bookmark_stderr(stderr: &str, bookmark: &str) -> Option<Vec<String>> {
    let marker = format!("Name `{bookmark}` is conflicted");
    if !stderr.contains(&marker) {
        return None;
    }
    const HINT_PREFIX: &str = "Use commit ID to select single revision from:";
    let candidates = stderr
        .lines()
        .find_map(|line| line.split_once(HINT_PREFIX).map(|(_, rest)| rest))
        .map(|ids| {
            ids.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(candidates)
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
    unpushed_summary: Option<Option<UnpushedSummary>>,
    other_bookmarks: Option<Vec<String>>,
    change_is_empty: Option<bool>,
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
    UnpushedSummary {
        workdir: PathBuf,
        bookmark: String,
        remote: String,
    },
    OtherBookmarksInRange {
        workdir: PathBuf,
        bookmark: String,
    },
    ChangeIsEmpty {
        workdir: PathBuf,
        change: String,
    },
    NewChangeAt {
        workdir: PathBuf,
        parent: String,
        message: Option<String>,
    },
    SetBookmarkAt {
        workdir: PathBuf,
        bookmark: String,
        target: String,
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

    /// Force `unpushed_summary()` to return `s`. Default is
    /// `Ok(None)` (no unpushed commits) when unset. Pass
    /// `Some(Some(summary))` for a positive result.
    pub fn with_unpushed_summary(self, s: Option<UnpushedSummary>) -> Self {
        self.inner.lock().unwrap().unpushed_summary = Some(s);
        self
    }

    /// Force `other_bookmarks_in_range()` to return `names`. Default is
    /// an empty vec (no side branches in range) when unset.
    pub fn with_other_bookmarks(self, names: Vec<String>) -> Self {
        self.inner.lock().unwrap().other_bookmarks = Some(names);
        self
    }

    /// Force `change_is_empty()` to return `is_empty`. Default is
    /// `true` (matches the common case: an empty `@` between writes).
    pub fn with_change_is_empty(self, is_empty: bool) -> Self {
        self.inner.lock().unwrap().change_is_empty = Some(is_empty);
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

    fn other_bookmarks_in_range(&self, workdir: &Path, bookmark: &str) -> Result<Vec<String>> {
        let mut s = self.inner.lock().unwrap();
        s.calls.push(Call::OtherBookmarksInRange {
            workdir: workdir.to_path_buf(),
            bookmark: bookmark.to_string(),
        });
        Ok(s.other_bookmarks.clone().unwrap_or_default())
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

    fn unpushed_summary(
        &self,
        workdir: &Path,
        bookmark: &str,
        remote: &str,
        _now: OffsetDateTime,
    ) -> Result<Option<UnpushedSummary>> {
        let mut s = self.inner.lock().unwrap();
        s.calls.push(Call::UnpushedSummary {
            workdir: workdir.to_path_buf(),
            bookmark: bookmark.to_string(),
            remote: remote.to_string(),
        });
        Ok(s.unpushed_summary.clone().unwrap_or(None))
    }

    fn change_is_empty(&self, workdir: &Path, change: &str) -> Result<bool> {
        let mut s = self.inner.lock().unwrap();
        s.calls.push(Call::ChangeIsEmpty {
            workdir: workdir.to_path_buf(),
            change: change.to_string(),
        });
        Ok(s.change_is_empty.unwrap_or(true))
    }

    fn new_change_at(&self, workdir: &Path, parent: &str, message: Option<&str>) -> Result<()> {
        self.inner.lock().unwrap().calls.push(Call::NewChangeAt {
            workdir: workdir.to_path_buf(),
            parent: parent.to_string(),
            message: message.map(str::to_string),
        });
        Ok(())
    }

    fn set_bookmark_at(&self, workdir: &Path, bookmark: &str, target: &str) -> Result<()> {
        self.inner.lock().unwrap().calls.push(Call::SetBookmarkAt {
            workdir: workdir.to_path_buf(),
            bookmark: bookmark.to_string(),
            target: target.to_string(),
        });
        Ok(())
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
    fn cause_is_missing_remote_matches_jj_no_remotes_error() {
        let err = Error::JjCommandFailed {
            command: "jj git fetch --remote origin".into(),
            status: 1,
            stderr:
                "Warning: No git remotes matching 'origin'\nError: No git remotes to fetch from\n"
                    .into(),
        };
        assert!(cause_is_missing_remote(&err));
    }

    #[test]
    fn cause_is_missing_remote_ignores_unrelated_failures() {
        let err = Error::JjCommandFailed {
            command: "jj git fetch --remote origin".into(),
            status: 1,
            stderr: "Error: connection refused\n".into(),
        };
        assert!(!cause_is_missing_remote(&err));
    }

    #[test]
    fn parse_conflicted_bookmark_extracts_candidate_ids_from_hint() {
        let stderr = "\
Error: Name `main` is conflicted
Hint: Use commit ID to select single revision from: e1214238e693, e92e7587f4f1
Hint: Use `bookmarks(exact:main)` to select all revisions
Hint: To set which revision the bookmark points to, run `jj bookmark set main -r <REVISION>`
";
        let got = parse_conflicted_bookmark_stderr(stderr, "main").expect("matches");
        assert_eq!(got, vec!["e1214238e693", "e92e7587f4f1"]);
    }

    #[test]
    fn parse_conflicted_bookmark_returns_none_for_unrelated_stderr() {
        assert!(parse_conflicted_bookmark_stderr("Error: connection refused\n", "main").is_none());
    }

    #[test]
    fn parse_conflicted_bookmark_returns_none_for_different_bookmark() {
        // The conflict is on `feature/x`, not the bookmark we asked about.
        let stderr = "Error: Name `feature/x` is conflicted\n";
        assert!(parse_conflicted_bookmark_stderr(stderr, "main").is_none());
    }

    #[test]
    fn parse_conflicted_bookmark_handles_missing_hint_line() {
        // Older or differently-configured jj versions might omit the
        // candidate hint. Still a conflict — just no candidates to
        // surface.
        let stderr = "Error: Name `main` is conflicted\n";
        let got = parse_conflicted_bookmark_stderr(stderr, "main").expect("matches");
        assert!(got.is_empty());
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

    fn active_opts() -> SyncOptions {
        SyncOptions::with_defaults(false)
    }

    #[test]
    fn commit_and_push_runs_full_flow_in_order() {
        let jj = FakeJj::new();
        let executed = std::cell::Cell::new(false);
        commit_and_push(&jj, &wd(), &active_opts(), "msg", || {
            executed.set(true);
            Ok::<(), Error>(())
        })
        .unwrap();
        assert!(executed.get());
        let calls = jj.calls();
        assert_eq!(calls.len(), 7, "got: {calls:?}");
        assert!(matches!(calls[0], Call::Status { .. }));
        assert!(matches!(calls[1], Call::OtherBookmarksInRange { .. }));
        assert!(matches!(calls[2], Call::Fetch { .. }));
        assert!(matches!(calls[3], Call::Rebase { .. }));
        assert!(matches!(
            calls[4],
            Call::NewChange { ref message, .. } if message == "msg"
        ));
        assert!(matches!(calls[5], Call::SetBookmark { .. }));
        assert!(matches!(calls[6], Call::Push { .. }));
    }

    #[test]
    fn commit_and_push_skips_jj_when_disabled() {
        let jj = FakeJj::new();
        let mut opts = active_opts();
        opts.config.enabled = false;
        let executed = std::cell::Cell::new(false);
        commit_and_push(&jj, &wd(), &opts, "msg", || {
            executed.set(true);
            Ok::<(), Error>(())
        })
        .unwrap();
        assert!(executed.get(), "write must run even when sync is disabled");
        assert!(jj.calls().is_empty(), "no jj calls when sync is disabled");
    }

    #[test]
    fn commit_and_push_skips_jj_when_no_sync_flag_set() {
        let jj = FakeJj::new();
        let opts = SyncOptions::with_defaults(true);
        commit_and_push(&jj, &wd(), &opts, "msg", || Ok::<(), Error>(())).unwrap();
        assert!(jj.calls().is_empty());
    }

    #[test]
    fn commit_and_push_skips_jj_calls_when_jj_not_installed() {
        let jj = FakeJj::new().with_status(Status::JjNotInstalled);
        let executed = std::cell::Cell::new(false);
        commit_and_push(&jj, &wd(), &active_opts(), "msg", || {
            executed.set(true);
            Ok::<(), Error>(())
        })
        .unwrap();
        assert!(executed.get(), "write still runs even without jj");
        // Only the initial status check should have fired.
        assert_eq!(jj.calls().len(), 1);
        assert!(matches!(jj.calls()[0], Call::Status { .. }));
    }

    #[test]
    fn commit_and_push_skips_jj_calls_when_not_a_jj_repo() {
        let jj = FakeJj::new().with_status(Status::NotAJjRepo);
        commit_and_push(&jj, &wd(), &active_opts(), "msg", || Ok::<(), Error>(())).unwrap();
        assert_eq!(jj.calls().len(), 1);
    }

    #[test]
    fn commit_and_push_hard_fails_on_rebase_conflict() {
        let jj = FakeJj::new().with_rebase_outcome(RebaseOutcome::Conflicted);
        let executed = std::cell::Cell::new(false);
        let err = commit_and_push(&jj, &wd(), &active_opts(), "msg", || {
            executed.set(true);
            Ok::<(), Error>(())
        })
        .unwrap_err();
        assert!(matches!(err, Error::SyncRebaseConflict { .. }));
        assert!(
            !executed.get(),
            "write must NOT run when rebase produced conflicts"
        );
        let calls = jj.calls();
        // Status, OtherBookmarksInRange, Fetch, Rebase — then bail.
        assert_eq!(calls.len(), 4);
        assert!(!calls
            .iter()
            .any(|c| matches!(c, Call::NewChange { .. } | Call::Push { .. })));
    }

    #[test]
    fn commit_and_push_succeeds_when_push_fails_best_effort() {
        let jj = FakeJj::new().with_push_outcome(PushOutcome::Failed("non-FF".into()));
        let executed = std::cell::Cell::new(false);
        let result = commit_and_push(&jj, &wd(), &active_opts(), "msg", || {
            executed.set(true);
            Ok::<(), Error>(())
        });
        assert!(result.is_ok(), "push failure must NOT fail the command");
        assert!(executed.get(), "write must still have run");
        // Full call sequence happened, including push.
        assert_eq!(jj.calls().len(), 7);
    }

    #[test]
    fn commit_and_push_refuses_when_side_branch_in_range() {
        // `@` sitting on top of a feature branch means
        // `set_bookmark_to_head` would silently yank those commits onto
        // `<bookmark>`. Bail before any destructive work.
        let jj = FakeJj::new().with_other_bookmarks(vec!["chore/feature".into()]);
        let executed = std::cell::Cell::new(false);
        let err = commit_and_push(&jj, &wd(), &active_opts(), "msg", || {
            executed.set(true);
            Ok::<(), Error>(())
        })
        .unwrap_err();
        match err {
            Error::WorkdirOnSideBranch {
                bookmark,
                side_bookmarks,
            } => {
                assert_eq!(bookmark, "main");
                assert_eq!(side_bookmarks, vec!["chore/feature".to_string()]);
            }
            other => panic!("expected WorkdirOnSideBranch, got {other:?}"),
        }
        assert!(!executed.get(), "write must NOT run when precheck fails");
        let calls = jj.calls();
        // Status, OtherBookmarksInRange — then bail.
        assert_eq!(calls.len(), 2);
        assert!(matches!(calls[0], Call::Status { .. }));
        assert!(matches!(calls[1], Call::OtherBookmarksInRange { .. }));
    }

    #[test]
    fn commit_and_push_skips_precheck_when_disabled() {
        // `enabled = false` bypasses the entire jj orchestration, so
        // the side-branch precheck must not fire either.
        let jj = FakeJj::new().with_other_bookmarks(vec!["chore/feature".into()]);
        let mut opts = active_opts();
        opts.config.enabled = false;
        let executed = std::cell::Cell::new(false);
        commit_and_push(&jj, &wd(), &opts, "msg", || {
            executed.set(true);
            Ok::<(), Error>(())
        })
        .unwrap();
        assert!(executed.get(), "write must run when sync is disabled");
        assert!(
            jj.calls().is_empty(),
            "precheck must not fire when sync is disabled"
        );
    }

    #[test]
    fn commit_and_push_skips_precheck_when_jj_not_installed() {
        // Status::JjNotInstalled short-circuits before the precheck —
        // we can't ask jj about bookmarks if jj isn't on PATH.
        let jj = FakeJj::new()
            .with_status(Status::JjNotInstalled)
            .with_other_bookmarks(vec!["chore/feature".into()]);
        commit_and_push(&jj, &wd(), &active_opts(), "msg", || Ok::<(), Error>(())).unwrap();
        let calls = jj.calls();
        assert_eq!(calls.len(), 1);
        assert!(matches!(calls[0], Call::Status { .. }));
    }

    #[test]
    fn fake_other_bookmarks_defaults_to_empty() {
        let jj = FakeJj::new();
        assert!(jj
            .other_bookmarks_in_range(&wd(), "main")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn fake_other_bookmarks_can_be_overridden() {
        let jj = FakeJj::new().with_other_bookmarks(vec!["feature/x".into()]);
        assert_eq!(
            jj.other_bookmarks_in_range(&wd(), "main").unwrap(),
            vec!["feature/x".to_string()]
        );
    }
}
