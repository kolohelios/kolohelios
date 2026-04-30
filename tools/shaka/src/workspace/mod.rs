mod forget;
mod list;
mod new;
mod status;

use std::path::{Path, PathBuf};

use clap::Subcommand;

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

#[derive(Subcommand)]
pub enum WorkspaceCommand {
    /// Create a new jj workspace as a sibling of the repo
    New {
        /// Workspace name (slug). Directory will be ../<repo-basename>-<name>.
        /// Mutually exclusive with --issue.
        #[arg(conflicts_with = "issue")]
        name: Option<String>,

        /// GitHub issue number. Derives name as i<N> and prints the issue title.
        /// Mutually exclusive with <name>.
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
}

pub fn run(cmd: WorkspaceCommand) {
    match cmd {
        WorkspaceCommand::New { name, issue } => new::run(name.as_deref(), issue),
        WorkspaceCommand::List => list::run(),
        WorkspaceCommand::Forget { name, force } => forget::run(&name, force),
        WorkspaceCommand::Status { json } => status::run(json),
    }
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
