mod audit;
mod pr;
pub mod send;
mod sync;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum RepoCommand {
    /// Audit repository settings for security and quality
    Audit {
        /// Repository in owner/repo format (auto-detected from git remote if omitted)
        #[arg(long)]
        repo: Option<String>,

        /// Apply recommended settings automatically
        #[arg(long)]
        fix: bool,
    },
    /// Fetch from origin and rebase the current change onto main@origin
    Sync {
        /// Print the commands that would run without executing them
        #[arg(long)]
        dry_run: bool,
    },
    /// Set a bookmark on the current change, push it, and open a PR
    Send {
        /// Bookmark name (auto-derived from the change description if omitted)
        #[arg(long)]
        bookmark: Option<String>,

        /// Push only — do not create a PR even if one is missing
        #[arg(long)]
        no_pr: bool,

        /// Print the commands that would run without executing them
        #[arg(long)]
        dry_run: bool,
    },
    /// Ensure a GitHub PR exists for the current change (push, then create if needed)
    Pr {
        /// Bookmark name (auto-detected from the current change if omitted)
        #[arg(long)]
        bookmark: Option<String>,

        /// Print the commands that would run without executing them
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(cmd: RepoCommand) {
    match cmd {
        RepoCommand::Audit { repo, fix } => audit::run(repo, fix),
        RepoCommand::Sync { dry_run } => sync::run(dry_run),
        RepoCommand::Send {
            bookmark,
            no_pr,
            dry_run,
        } => send::run(bookmark, no_pr, dry_run),
        RepoCommand::Pr { bookmark, dry_run } => pr::run(bookmark, dry_run),
    }
}
