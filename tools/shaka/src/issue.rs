mod audit;
mod brief;
mod list;

use clap::{Subcommand, ValueEnum};

use crate::gh::ListState;

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum StateArg {
    Open,
    Closed,
    All,
}

impl From<StateArg> for ListState {
    fn from(s: StateArg) -> Self {
        match s {
            StateArg::Open => ListState::Open,
            StateArg::Closed => ListState::Closed,
            StateArg::All => ListState::All,
        }
    }
}

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
    /// List issues with optional filters
    List {
        /// Repository in owner/repo format (auto-detected from git remote if omitted)
        #[arg(long)]
        repo: Option<String>,
        /// Filter by state
        #[arg(long, value_enum, default_value_t = StateArg::Open)]
        state: StateArg,
        /// Filter by label (repeatable; AND semantics)
        #[arg(long = "label")]
        labels: Vec<String>,
        /// Filter by milestone title
        #[arg(long)]
        milestone: Option<String>,
        /// Full-text search query, passed through to GitHub's issue
        /// search syntax. Combinable with `--state`, `--label`, and
        /// `--milestone`.
        #[arg(long)]
        search: Option<String>,
        /// Maximum number of issues to return
        #[arg(long, default_value_t = 30)]
        limit: u32,
        /// Emit normalized JSON instead of the human-readable tree
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
        IssueCommand::List {
            repo,
            state,
            labels,
            milestone,
            search,
            limit,
            json,
        } => list::run(repo, state.into(), labels, milestone, search, limit, json),
    }
}
