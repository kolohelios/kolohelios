mod cleanup;
mod forget;
mod issue_link;
mod list;
mod new;
mod status;

use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::jj;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

#[derive(Subcommand)]
pub enum WorkspaceCommand {
    /// Create a new jj workspace as a sibling of the repo
    New {
        /// Workspace name (slug). Directory will be `../<repo-basename>-<name>`.
        /// Mutually exclusive with --issue.
        #[arg(conflicts_with = "issue")]
        name: Option<String>,

        /// GitHub issue number. Derives name as `i<N>` and prints the issue title.
        /// Mutually exclusive with `<name>`.
        #[arg(long, conflicts_with = "name")]
        issue: Option<u64>,
    },
    /// List all jj workspaces in the repo
    List,
    /// De-register a jj workspace and remove its directory
    Forget {
        /// Workspace name to forget
        name: String,
        /// Forget even if the workspace has uncommitted or unpushed work
        #[arg(long)]
        force: bool,
    },
    /// Show status summary across all workspaces
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove workspaces whose PRs have been merged
    Cleanup {
        /// Preview what would be cleaned up without making any changes
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(cmd: WorkspaceCommand) {
    match cmd {
        WorkspaceCommand::New { name, issue } => new::run(name.as_deref(), issue),
        WorkspaceCommand::List => list::run(),
        WorkspaceCommand::Forget { name, force } => forget::run(&name, force),
        WorkspaceCommand::Status { json } => status::run(json),
        WorkspaceCommand::Cleanup { dry_run } => cleanup::run(dry_run),
    }
}

/// A workspace linked to a specific GitHub issue.
pub struct WorkspaceRef {
    pub name: String,
    pub path: PathBuf,
}

/// Find the workspace whose persisted issue link points at issue `n`. Skips
/// the default workspace and any workspace without a link file (legacy
/// workspaces created before the link was persisted, or ad-hoc named ones).
///
/// Uses [`jj::primary_workspace_root`] so this works identically whether
/// the caller is in the primary tree or a sibling — `.shaka/workspaces/`
/// only exists at the primary.
pub fn find_for_issue(n: u64) -> Option<WorkspaceRef> {
    let primary = jj::primary_workspace_root().ok()?;
    let workspaces = jj::workspaces().ok()?;
    for ws in workspaces {
        if ws.name == "default" {
            continue;
        }
        let link = issue_link::read(&primary, &ws.name).ok().flatten();
        if link.map(|l| l.issue) == Some(n) {
            return Some(WorkspaceRef {
                name: ws.name.clone(),
                path: workspace_path(&primary, &ws.name),
            });
        }
    }
    None
}

/// Path where a workspace directory lives, by convention:
/// `<repo-root>/../<repo-basename>-<name>`. Tying the prefix to the repo
/// basename keeps tests hermetic (each tempdir gets its own prefix) and
/// makes the relationship between the repo and its workspaces obvious on
/// disk.
pub(crate) fn workspace_path(repo_root: &Path, name: &str) -> PathBuf {
    let basename = repo_root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".to_string());
    let parent = repo_root.parent().unwrap_or(repo_root);
    parent.join(format!("{basename}-{name}"))
}

pub(crate) fn die(msg: &str) -> ! {
    eprintln!("{RED}{BOLD}error:{RESET} {msg}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn workspace_path_prefixes_with_basename() {
        let p = workspace_path(Path::new("/Users/me/code/kolohelios"), "feat-foo");
        assert_eq!(p, Path::new("/Users/me/code/kolohelios-feat-foo"));
    }

    #[test]
    fn workspace_path_handles_temp_style_parent() {
        let p = workspace_path(Path::new("/tmp/abc/repo"), "i42");
        assert_eq!(p, Path::new("/tmp/abc/repo-i42"));
    }

    #[test]
    fn issue_name_derivation() {
        // Derived workspace name for issue N is "i<N>".
        let n: u64 = 42;
        let derived = format!("i{n}");
        assert_eq!(derived, "i42");

        let n: u64 = 102;
        let derived = format!("i{n}");
        assert_eq!(derived, "i102");
    }
}
