use serde::Serialize;

use super::staleness::{self, Staleness};
use super::{die, workspace_path, BOLD, DIM, GREEN, RED, RESET, YELLOW};
use crate::{gh, jj};

#[derive(Serialize)]
struct WorkspaceStatus {
    name: String,
    path: String,
    change_id: String,
    description: String,
    bookmarks: Vec<String>,
    ahead: usize,
    dirty: bool,
    dirty_counts: DirtyCounts,
    staleness: Staleness,
}

#[derive(Serialize)]
struct DirtyCounts {
    added: usize,
    modified: usize,
    deleted: usize,
    total: usize,
}

pub fn run(json: bool) {
    let repo_root = match jj::repo_root() {
        Ok(p) => p,
        Err(e) => die(&e.to_string()),
    };

    let workspaces = match jj::workspaces() {
        Ok(w) => w,
        Err(e) => die(&e.to_string()),
    };

    // Detect the GitHub repo for the merged-PR lookup. Failure here is
    // non-fatal — we just classify every workspace as `active` rather
    // than refusing to render. A local-only repo (no remote) should
    // still produce a useful status table.
    let repo = gh::detect_repo().ok();

    let mut statuses: Vec<WorkspaceStatus> = Vec::new();

    for ws in &workspaces {
        let path = if ws.name == "default" {
            repo_root.clone()
        } else {
            workspace_path(&repo_root, &ws.name)
        };

        let bookmarks = jj::bookmarks_on_workspace(&ws.name).unwrap_or_default();

        let counts = jj::dirty_counts_for(&ws.name).unwrap_or_default();
        let dirty = counts.total() > 0;

        // ahead_count_for may fail if main@origin doesn't exist (fresh repo);
        // fall back to 0 so status still renders.
        let ahead = jj::ahead_count_for(&ws.name, "main@origin").unwrap_or(0);

        // Default workspace never carries issue-shaped work — it's the
        // primary tree, always `active` for our purposes. Skip the
        // network probe for it.
        let staleness = if ws.name == "default" {
            Staleness::Active
        } else if let Some(r) = &repo {
            staleness::classify(&repo_root, r, &ws.name, &bookmarks, ahead, dirty)
        } else {
            Staleness::Active
        };

        statuses.push(WorkspaceStatus {
            name: ws.name.clone(),
            path: path.display().to_string(),
            change_id: ws.change_id.clone(),
            description: ws.description_first_line.clone(),
            bookmarks,
            ahead,
            dirty,
            dirty_counts: DirtyCounts {
                added: counts.added,
                modified: counts.modified,
                deleted: counts.deleted,
                total: counts.total(),
            },
            staleness,
        });
    }

    if json {
        match serde_json::to_string_pretty(&statuses) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} failed to serialize workspace status: {e}");
                std::process::exit(1);
            }
        }
    } else {
        render_human(&statuses);
    }
}

fn render_human(statuses: &[WorkspaceStatus]) {
    for ws in statuses {
        // Header: name + path + (optional) staleness flag
        let flag = match &ws.staleness {
            Staleness::Active => String::new(),
            Staleness::Merged { .. } => format!("  {GREEN}[merged]{RESET}"),
            Staleness::Orphan => format!("  {YELLOW}[orphan]{RESET}"),
        };
        println!("{BOLD}{}{RESET}  {DIM}{}{RESET}{flag}", ws.name, ws.path);

        // Current @: change id + description
        let desc = if ws.description.is_empty() {
            format!("{DIM}(no description){RESET}")
        } else {
            ws.description.clone()
        };
        println!("  @ {DIM}{}{RESET}  {desc}", ws.change_id);

        // Bookmarks
        let bookmark_label = if ws.bookmarks.is_empty() {
            format!("{DIM}none{RESET}")
        } else {
            ws.bookmarks.join(", ")
        };
        println!("  {BOLD}bookmarks:{RESET}  {bookmark_label}");

        // Ahead
        let ahead_label = match ws.ahead {
            0 => format!("{DIM}0 commits ahead of main@origin{RESET}"),
            1 => format!("{GREEN}1 commit ahead of main@origin{RESET}"),
            n => format!("{GREEN}{n} commits ahead of main@origin{RESET}"),
        };
        println!("  {BOLD}ahead:{RESET}      {ahead_label}");

        // Dirty
        let dirty_label = if !ws.dirty {
            format!("{DIM}clean{RESET}")
        } else {
            format!(
                "{YELLOW}{}+{RESET} {YELLOW}{}~{RESET} {YELLOW}{}-{RESET}",
                ws.dirty_counts.added, ws.dirty_counts.modified, ws.dirty_counts.deleted
            )
        };
        println!("  {BOLD}working copy:{RESET} {dirty_label}");

        // For merged workspaces, surface the PR URL so the user can
        // verify before running cleanup. Orphans get nothing because
        // there's nothing to verify against.
        if let Staleness::Merged { pr_url } = &ws.staleness {
            println!("  {BOLD}merged:{RESET}     {DIM}{pr_url}{RESET}");
        }

        println!();
    }

    // Trailing summary if any workspaces are stale. Matches the
    // wording from the cleanup-hint side of #204 so the two surfaces
    // teach the same lesson.
    let merged = statuses
        .iter()
        .filter(|s| matches!(s.staleness, Staleness::Merged { .. }))
        .count();
    let orphan = statuses
        .iter()
        .filter(|s| matches!(s.staleness, Staleness::Orphan))
        .count();
    let stale = merged + orphan;
    if stale > 0 {
        let plural = if stale == 1 { "" } else { "s" };
        println!(
            "{YELLOW}{stale} workspace{plural} stale ({merged} merged, {orphan} orphan); \
             run shaka workspace cleanup{RESET}"
        );
    }
}
