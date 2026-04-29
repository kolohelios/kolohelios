mod forget;
mod list;
mod new;

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
        /// Workspace name (slug). Directory will be ../<repo-basename>-<name>
        name: String,
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
}

pub fn run(cmd: WorkspaceCommand) {
    match cmd {
        WorkspaceCommand::New { name } => new::run(&name),
        WorkspaceCommand::List => list::run(),
        WorkspaceCommand::Forget { name, force } => forget::run(&name, force),
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
}
