mod audit;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum IssueCommand {
    /// Audit open issues for missing scope labels
    Audit {
        /// Repository in owner/repo format (auto-detected from git remote if omitted)
        #[arg(long)]
        repo: Option<String>,
    },
}

pub fn run(cmd: IssueCommand) {
    match cmd {
        IssueCommand::Audit { repo } => audit::run(repo),
    }
}
