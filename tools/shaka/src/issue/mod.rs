mod audit;
mod brief;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum IssueCommand {
    /// Audit open issues for missing scope labels
    Audit {
        /// Repository in owner/repo format (auto-detected from git remote if omitted)
        #[arg(long)]
        repo: Option<String>,
    },
    /// Fetch + view + comments for an issue in one shot
    Brief {
        /// GitHub issue number
        number: u64,
        /// Skip the `jj git fetch` step
        #[arg(long)]
        no_fetch: bool,
        /// Emit structured JSON instead of the human-readable tree
        #[arg(long)]
        json: bool,
    },
}

pub fn run(cmd: IssueCommand) {
    match cmd {
        IssueCommand::Audit { repo } => audit::run(repo),
        IssueCommand::Brief {
            number,
            no_fetch,
            json,
        } => brief::run(number, no_fetch, json),
    }
}
