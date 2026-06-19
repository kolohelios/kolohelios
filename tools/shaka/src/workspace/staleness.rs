//! Workspace staleness classification — shared between `shaka workspace
//! status` (annotates each workspace inline) and `shaka repo sync`
//! (appends a cleanup hint).
//!
//! Three categories:
//!
//! - **`Active`** — has WIP somewhere (bookmark, ahead of main, or dirty
//!   working copy). No action implied.
//! - **`Merged`** — bookmark or persisted issue link points at a PR that
//!   has merged on the remote. Carries the PR URL so consumers can
//!   surface it without re-querying.
//! - **`Orphan`** — no bookmarks, 0 commits ahead, clean working copy.
//!   The workspace's purpose is unrecoverable from VCS state.
//!
//! `Orphan` is computed locally with no network. `Merged` requires at
//! least one GitHub API call; `classify` skips the network entirely
//! when a workspace has no bookmarks and no persisted issue link
//! (in that case it can only be `Orphan` or `Active`).

use std::path::Path;

use super::issue_link;
use crate::gh;
use crate::jj;
use crate::term::{DIM, RESET};

/// Why a workspace is considered stale — or that it isn't. `Active`
/// is the no-action default; the two stale variants name a specific
/// reason that maps to a recovery action.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Staleness {
    Active,
    Merged { pr_url: String },
    Orphan,
}

/// Classify a workspace from its already-fetched state. `bookmarks`,
/// `ahead`, and `dirty` are the same signals `shaka workspace status`
/// already computes; this function avoids re-querying jj.
///
/// `repo` is the GitHub `owner/repo` — needed only for the bookmark-scan
/// fallback path when neither a persisted issue link nor an `i<N>` name
/// resolves the merged PR.
pub fn classify(
    repo_root: &Path,
    repo: &str,
    name: &str,
    bookmarks: &[String],
    ahead: usize,
    dirty: bool,
) -> Staleness {
    let issue_link = issue_link::read(repo_root, name)
        .map_err(|e| eprintln!("{DIM}warn:{RESET} reading issue link for {name}: {e}"))
        .ok()
        .flatten();

    // Cheap path: if there's no bookmark, no issue link, and the
    // workspace is at main with a clean working copy, it's orphan.
    // Avoids API calls for the common "I forgot to forget" case.
    if bookmarks.is_empty() && issue_link.is_none() && ahead == 0 && !dirty {
        return Staleness::Orphan;
    }

    if let Some(pr_url) = resolve_merged_pr(repo, name, bookmarks, issue_link.map(|l| l.issue)) {
        return Staleness::Merged { pr_url };
    }

    Staleness::Active
}

/// Walk every workspace and classify each. Default workspace is omitted
/// since it has no meaningful staleness (it carries no specific work).
/// Returns `(name, staleness)` pairs in workspace order.
pub fn classify_all(repo_root: &Path, repo: &str) -> Vec<(String, Staleness)> {
    let workspaces = jj::workspaces().unwrap_or_default();
    let mut out = Vec::with_capacity(workspaces.len());
    for ws in &workspaces {
        if ws.name == "default" {
            continue;
        }
        let bookmarks = jj::bookmarks_on_workspace(&ws.name).unwrap_or_default();
        let counts = jj::dirty_counts_for(&ws.name).unwrap_or_default();
        let dirty = counts.total() > 0;
        let ahead = jj::ahead_count_for(&ws.name, "main@origin").unwrap_or(0);
        let staleness = classify(repo_root, repo, &ws.name, &bookmarks, ahead, dirty);
        out.push((ws.name.clone(), staleness));
    }
    out
}

/// Try each detection strategy in order and return the merged-PR URL on
/// the first hit. Lifted from `cleanup::resolve_merged_pr` so both the
/// cleanup command and the staleness classifier share one implementation.
pub(super) fn resolve_merged_pr(
    repo: &str,
    name: &str,
    bookmarks: &[String],
    issue_from_link: Option<u64>,
) -> Option<String> {
    let issue_from_name = name
        .strip_prefix('i')
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|_| issue_from_link.is_none());

    if let Some(issue) = issue_from_link.or(issue_from_name) {
        // A merged closing PR means the work landed — even if GitHub left
        // the issue open because the rebase merge skipped the body-keyword
        // autoclose (#946). `issue_closure` reports that case; the older
        // `merged_pr_for_issue` missed it by gating on a closed issue.
        match gh::issue_closure(repo, issue) {
            Ok(c) => {
                if let Some(pr) = c.merged_closing_pr {
                    return Some(pr.url);
                }
            }
            Err(e) => {
                eprintln!("{DIM}warn:{RESET} could not query PR for issue #{issue}: {e}");
            }
        }
    }

    for bookmark in bookmarks {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from("/tmp/shaka-staleness-test")
    }

    #[test]
    fn orphan_when_no_signal_and_clean() {
        // No bookmarks, no issue link (read returns None for a
        // tempdir-less root), 0 ahead, clean. Skips the network path.
        let s = classify(&root(), "o/r", "ad-hoc", &[], 0, false);
        assert_eq!(s, Staleness::Orphan);
    }

    #[test]
    fn active_when_dirty_with_no_other_signal() {
        let s = classify(&root(), "o/r", "ad-hoc", &[], 0, true);
        assert_eq!(s, Staleness::Active);
    }

    #[test]
    fn active_when_ahead_with_no_other_signal() {
        let s = classify(&root(), "o/r", "ad-hoc", &[], 3, false);
        assert_eq!(s, Staleness::Active);
    }
}
