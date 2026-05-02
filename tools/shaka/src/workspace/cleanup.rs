use super::issue_link;
use super::{die, workspace_path, BOLD, DIM, GREEN, RED, RESET};
use crate::{gh, jj};
use std::fs;
use std::path::Path;

/// A workspace that has a merged PR and is eligible for cleanup.
struct MergedWorkspace {
    name: String,
    pr_url: String,
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
            });
        }
    }

    if candidates.is_empty() {
        println!("no workspaces with merged PRs found");
        return;
    }

    for candidate in &candidates {
        if dry_run {
            println!(
                "{DIM}dry-run:{RESET} would forget workspace {BOLD}{}{RESET} (merged: {})",
                candidate.name, candidate.pr_url
            );
        } else {
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
/// first hit.
fn resolve_merged_pr(repo_root: &Path, repo: &str, name: &str) -> Option<String> {
    // 1. Persisted issue link.
    let issue_from_link = issue_link::read(repo_root, name)
        .map_err(|e| eprintln!("{DIM}warn:{RESET} reading issue link for {name}: {e}"))
        .ok()
        .flatten()
        .map(|l| l.issue);

    // 2. Name inference: i<N>.
    let issue_from_name = name
        .strip_prefix('i')
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|_| issue_from_link.is_none());

    if let Some(issue) = issue_from_link.or(issue_from_name) {
        match gh::merged_pr_for_issue(issue) {
            Ok(Some(pr)) => return Some(pr.url),
            Ok(None) => {} // issue not yet closed by a merged PR
            Err(e) => {
                eprintln!("{DIM}warn:{RESET} could not query PR for issue #{issue}: {e}");
            }
        }
    }

    // 3. Bookmark scan (legacy path; works only pre-sync).
    let revset = format!("main@origin..{name}@");
    let bookmarks = jj::bookmarks_on(&revset).unwrap_or_default();
    for bookmark in &bookmarks {
        match gh::pr_for_head(repo, bookmark) {
            Ok(Some(pr)) if pr.state == gh::PrState::Merged => return Some(pr.url),
            Ok(_) => {}
            Err(e) => {
                eprintln!("{DIM}warn:{RESET} could not query PR for bookmark {bookmark}: {e}");
            }
        }
    }

    None
}
