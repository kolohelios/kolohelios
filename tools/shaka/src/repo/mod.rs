mod audit;

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
}

pub fn run(cmd: RepoCommand) {
    match cmd {
        RepoCommand::Audit { repo, fix } => audit::run(repo, fix),
    }
}
