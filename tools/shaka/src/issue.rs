mod audit;
mod brief;
mod create;
mod label;
mod labels;
mod list;
mod next;

use clap::{ArgGroup, Subcommand, ValueEnum};

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
    /// Manage the canonical GitHub label set declared in .shaka/labels.cue
    #[command(subcommand)]
    Label(LabelCommand),
    /// Fetch + view + comments for one or more issues in one shot
    Brief {
        /// Issue number(s). Single (`586`) or comma-delimited
        /// (`345,427,483`) — multi-issue invocations render each brief
        /// separated by a blank line and share one `jj git fetch`.
        numbers: String,
        /// Skip the `jj git fetch` step
        #[arg(long)]
        no_fetch: bool,
        /// Emit structured JSON instead of the human-readable tree.
        /// Always wraps results in `{"briefs": [...]}` so single- and
        /// multi-issue invocations have the same shape.
        #[arg(long)]
        json: bool,
    },
    /// Create a GitHub issue with enforced scope label + conv-commit title
    #[command(group(ArgGroup::new("body_src").required(true).args(["body", "body_file"])))]
    Create {
        /// Target repo in owner/repo format (auto-detected from git remote
        /// if omitted). When set, `--scope` is validated against this
        /// repo's `.shaka/labels.cue` fetched over the API — so a consumer
        /// repo can file issues into the canonical repo with the right
        /// scope gate.
        #[arg(long)]
        repo: Option<String>,
        /// Issue title — must match `<type>(<scope>): <subject>`
        /// (`shaka commit lint`'s validator)
        #[arg(long)]
        title: String,
        /// Issue body text
        #[arg(long)]
        body: Option<String>,
        /// Path to a file containing the issue body
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        /// Scope label — must be one of the canonical scope labels
        /// declared in the target repo's `.shaka/labels.cue` (local tree
        /// when `--repo` is omitted; the foreign repo's when it is set)
        #[arg(long)]
        scope: String,
        /// Parent issue number; links via GitHub's native sub-issue
        /// API (no freeform `Sub-issue of #N` body text)
        #[arg(long)]
        parent: Option<u64>,
        /// Milestone title
        #[arg(long)]
        milestone: Option<String>,
        /// Extra labels beyond `--scope` (repeatable)
        #[arg(long = "label")]
        labels: Vec<String>,
        /// Print the planned `gh` calls without executing them
        #[arg(long)]
        dry_run: bool,
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
    /// List open issues not already in flight (no assignee, no open PR,
    /// no local `i<N>` workspace) — the unstarted work to pick from.
    Next {
        /// Repository in owner/repo format (auto-detected from git remote if omitted)
        #[arg(long)]
        repo: Option<String>,
        /// Maximum number of open issues to consider before filtering
        #[arg(long, default_value_t = 200)]
        limit: u32,
        /// Emit normalized JSON instead of the human-readable tree
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum LabelCommand {
    /// Diff canonical .shaka/labels.cue against GitHub; reconcile with --apply
    Sync {
        /// Repository in owner/repo format (auto-detected from git remote if omitted)
        #[arg(long)]
        repo: Option<String>,
        /// Apply create + edit operations; otherwise report drift only
        #[arg(long)]
        apply: bool,
        /// Also delete labels present on GitHub but not in .shaka/labels.cue
        /// (requires --apply)
        #[arg(long)]
        delete: bool,
    },
}

pub fn run(cmd: IssueCommand) {
    match cmd {
        IssueCommand::Audit { repo } => audit::run(repo),
        IssueCommand::Brief {
            numbers,
            no_fetch,
            json,
        } => brief::run(&numbers, no_fetch, json),
        IssueCommand::Create {
            repo,
            title,
            body,
            body_file,
            scope,
            parent,
            milestone,
            labels,
            dry_run,
        } => create::run(create::CreateArgs {
            repo,
            title,
            body,
            body_file,
            scope,
            parent,
            milestone,
            labels,
            dry_run,
        }),
        IssueCommand::Label(LabelCommand::Sync {
            repo,
            apply,
            delete,
        }) => label::run(repo, apply, delete),
        IssueCommand::List {
            repo,
            state,
            labels,
            milestone,
            search,
            limit,
            json,
        } => list::run(repo, state.into(), labels, milestone, search, limit, json),
        IssueCommand::Next { repo, limit, json } => next::run(repo, limit, json),
    }
}
