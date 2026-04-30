use super::{die, workspace_path, BOLD, DIM, GREEN, RED, RESET};
use crate::{gh, jj};
use std::fs;

/// A workspace that has a merged PR and is eligible for cleanup.
struct MergedWorkspace {
    name: String,
    pr_url: String,
}

/// Walk all workspaces, find ones whose bookmarks ahead of `main@origin` have
/// a merged PR on the remote, and clean them up (or preview with --dry-run).
///
/// Design note: we use a bookmark-convention approach for v1. For each
/// workspace we enumerate all bookmarks in `main@origin..<workspace>@` and
/// query GitHub for each via `gh::pr_for_head`. If any bookmark resolves to a
/// merged PR the workspace is a cleanup candidate.
///
/// Trade-off: a workspace with multiple bookmarks where only one has a merged
/// PR will still be cleaned up. This is intentional — if the branch landed,
/// the workspace's purpose is done. A future revision could require all
/// bookmarks to be merged before cleaning.
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

        let revset = format!("main@origin..{}@", ws.name);
        // Workspace may not have any commits ahead of main@origin, or
        // main@origin doesn't exist in this repo — treat both as no bookmarks.
        let bookmarks = jj::bookmarks_on(&revset).unwrap_or_default();

        if bookmarks.is_empty() {
            continue;
        }

        for bookmark in &bookmarks {
            match gh::pr_for_head(&repo, bookmark) {
                Ok(Some(pr)) if pr.state == gh::PrState::Merged => {
                    candidates.push(MergedWorkspace {
                        name: ws.name.clone(),
                        pr_url: pr.url.clone(),
                    });
                    // One merged bookmark is enough to flag this workspace.
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("{DIM}warn:{RESET} could not query PR for bookmark {bookmark}: {e}");
                }
            }
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

            println!(
                "{GREEN}{BOLD}forgot{RESET} workspace {BOLD}{}{RESET} (merged: {})",
                candidate.name, candidate.pr_url
            );
        }
    }
}
