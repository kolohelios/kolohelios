use super::issue_link;
use super::staleness;
use super::{die, workspace_path, BOLD, DIM, GREEN, RED, RESET};
use crate::{gh, jj};
use std::fs;
use std::path::Path;

/// A workspace that has a merged PR and is eligible for cleanup.
struct MergedWorkspace {
    name: String,
    pr_url: String,
    /// A linked issue still open despite the merged PR — GitHub's
    /// body-keyword autoclose doesn't fire on shaka's rebase merge (#946),
    /// so cleanup closes it as a belt-and-suspenders.
    open_issue: Option<u64>,
}

/// Walk all workspaces, find ones whose work has landed via a merged PR,
/// and clean them up (or preview with --dry-run).
///
/// Detection strategy, in order:
///
/// 1. **Persisted issue link.** `shaka workspace new --issue N` records the
///    issue under `<repo_root>/.shaka/workspaces/<name>.json`. We query
///    GitHub for the PR that closed that issue (`gh issue view --json
///    closedByPullRequestsReferences`). This is the durable path and
///    survives `repo sync` deleting the local bookmark.
/// 2. **`i<N>` name inference.** For workspaces created with `--issue N`
///    before persistence existed, the name itself encodes the issue.
/// 3. **Bookmark scan.** For workspaces created with an arbitrary `<name>`,
///    enumerate bookmarks in `main@origin..<workspace>@` and ask GitHub for
///    each via `pr_for_head`. This breaks once the bookmark has been
///    deleted post-merge — the persisted link path above is what fixes
///    issue #164.
pub fn run(dry_run: bool) {
    let repo_root = match jj::repo_root() {
        Ok(p) => p,
        Err(e) => die(&e.to_string()),
    };

    // Detect the GitHub repo. If there's no remote (e.g. a local-only repo or
    // a fresh test environment), we skip the PR-state queries and report
    // nothing to clean up — no point erroring fatally for a read-only command.
    let repo = match gh::detect_repo() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{DIM}warn:{RESET} could not detect GitHub repo ({e}); skipping PR checks");
            println!("no workspaces with merged PRs found");
            return;
        }
    };

    let workspaces = match jj::workspaces() {
        Ok(w) => w,
        Err(e) => die(&e.to_string()),
    };

    let mut candidates: Vec<MergedWorkspace> = Vec::new();

    for ws in &workspaces {
        if ws.name == "default" {
            continue;
        }

        if let Some(pr_url) = resolve_merged_pr(&repo_root, &repo, &ws.name) {
            candidates.push(MergedWorkspace {
                name: ws.name.clone(),
                pr_url,
                open_issue: open_linked_issue(&repo_root, &repo, &ws.name),
            });
        }
    }

    if candidates.is_empty() {
        println!("no workspaces with merged PRs found");
        return;
    }

    for candidate in &candidates {
        if dry_run {
            if let Some(issue) = candidate.open_issue {
                println!(
                    "{DIM}dry-run:{RESET} would close issue {BOLD}#{issue}{RESET} (merged: {})",
                    candidate.pr_url
                );
            }
            println!(
                "{DIM}dry-run:{RESET} would forget workspace {BOLD}{}{RESET} (merged: {})",
                candidate.name, candidate.pr_url
            );
        } else {
            close_open_issue(&repo, candidate);

            let path = workspace_path(&repo_root, &candidate.name);

            if let Err(e) = jj::workspace_forget(&candidate.name) {
                eprintln!(
                    "{RED}{BOLD}error:{RESET} failed to forget workspace {}: {e}",
                    candidate.name
                );
                continue;
            }

            if path.exists() {
                if let Err(e) = fs::remove_dir_all(&path) {
                    eprintln!(
                        "{RED}{BOLD}error:{RESET} removed jj workspace {} but failed to \
                         remove directory {}: {e}",
                        candidate.name,
                        path.display()
                    );
                    continue;
                }
            }

            if let Err(e) = issue_link::remove(&repo_root, &candidate.name) {
                eprintln!(
                    "{DIM}warn:{RESET} failed to remove issue link for {}: {e}",
                    candidate.name
                );
            }

            println!(
                "{GREEN}{BOLD}forgot{RESET} workspace {BOLD}{}{RESET} (merged: {})",
                candidate.name, candidate.pr_url
            );
        }
    }
}

/// Try each detection strategy in order and return the merged-PR URL on the
/// first hit. Thin wrapper that bridges `cleanup`'s pre-existing call site
/// to `staleness::resolve_merged_pr` (the shared implementation).
fn resolve_merged_pr(repo_root: &Path, repo: &str, name: &str) -> Option<String> {
    let issue_from_link = issue_link::read(repo_root, name)
        .map_err(|e| eprintln!("{DIM}warn:{RESET} reading issue link for {name}: {e}"))
        .ok()
        .flatten()
        .map(|l| l.issue);

    let revset = format!("main@origin..{name}@");
    let bookmarks = jj::bookmarks_on(&revset).unwrap_or_default();

    staleness::resolve_merged_pr(repo, name, &bookmarks, issue_from_link)
}

/// If the workspace has a persisted issue link whose issue is still open
/// despite a merged closing PR, return the issue number. This is the #946
/// case: the rebase merge that landed the PR didn't trigger GitHub's
/// body-keyword autoclose, so the issue lingers open.
fn open_linked_issue(repo_root: &Path, repo: &str, name: &str) -> Option<u64> {
    let issue = issue_link::read(repo_root, name).ok().flatten()?.issue;
    match gh::issue_closure(repo, issue) {
        Ok(c) if c.issue_open && c.merged_closing_pr.is_some() => Some(issue),
        Ok(_) => None,
        Err(e) => {
            eprintln!("{DIM}warn:{RESET} could not check issue #{issue} state: {e}");
            None
        }
    }
}

/// Close the candidate's still-open linked issue, leaving a comment that
/// explains why shaka closed it. Soft-fails so a close error doesn't block
/// forgetting the workspace.
fn close_open_issue(repo: &str, candidate: &MergedWorkspace) {
    let Some(issue) = candidate.open_issue else {
        return;
    };
    let comment = format!(
        "Closing automatically: {} merged via shaka's rebase flow, which doesn't \
         trigger GitHub's PR-body autoclose. (shaka workspace cleanup, #946)",
        candidate.pr_url
    );
    match gh::close_issue(repo, issue, &comment) {
        Ok(()) => println!(
            "{GREEN}{BOLD}closed{RESET} issue {BOLD}#{issue}{RESET} (merged: {})",
            candidate.pr_url
        ),
        Err(e) => eprintln!("{RED}{BOLD}error:{RESET} failed to close issue #{issue}: {e}"),
    }
}
