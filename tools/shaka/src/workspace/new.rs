use super::issue_link::{self, IssueLink};
use super::{die, workspace_path, BOLD, DIM, GREEN, RESET};
use crate::{gh, jj};

pub fn run(name: Option<&str>, issue: Option<u64>) {
    // Resolve the workspace name and an optional confirmation suffix.
    let (derived_name, issue_info) = match (name, issue) {
        (Some(n), None) => (n.to_string(), None),
        (None, Some(n)) => {
            let title = match gh::issue_title(n) {
                Ok(t) => t,
                Err(e) => die(&format!("could not fetch issue #{n}: {e}")),
            };
            (format!("i{n}"), Some((n, title)))
        }
        _ => die("provide either a workspace <name> or --issue <N>"),
    };

    let repo_root = match jj::repo_root() {
        Ok(p) => p,
        Err(e) => die(&e.to_string()),
    };

    let path = workspace_path(&repo_root, &derived_name);
    if path.exists() {
        die(&format!("path already exists: {}", path.display()));
    }

    let workspaces = match jj::workspaces() {
        Ok(w) => w,
        Err(e) => die(&e.to_string()),
    };
    if workspaces.iter().any(|w| w.name == derived_name) {
        die(&format!("workspace already registered: {derived_name}"));
    }

    if let Err(e) = jj::workspace_add(&derived_name, &path) {
        die(&e.to_string());
    }

    if let Some((n, title)) = issue_info {
        // Persist the issue link so `cleanup` can find the merged PR even
        // after `repo sync` deletes the local bookmark (see issue #164).
        if let Err(e) = issue_link::write(&repo_root, &derived_name, &IssueLink { issue: n }) {
            die(&format!("failed to record issue link: {e}"));
        }
        println!(
            "{GREEN}{BOLD}created{RESET} workspace {BOLD}{derived_name}{RESET} at \
             {DIM}{}{RESET} (issue #{n}: {title})",
            path.display()
        );
    } else {
        println!(
            "{GREEN}{BOLD}created{RESET} workspace {BOLD}{derived_name}{RESET} at \
             {DIM}{}{RESET}",
            path.display()
        );
    }
}
