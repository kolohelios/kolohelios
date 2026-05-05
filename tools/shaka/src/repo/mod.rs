mod audit;
mod bump_locks;
mod pr;
mod rebase_open_prs;
pub mod send;
mod status;
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
    /// Print a one-shot summary of the current working-copy state
    Status {
        /// Emit JSON instead of human-readable output
        #[arg(long)]
        json: bool,
    },
    /// Rebase every open PR whose base is `main` onto the current main@origin
    ///
    /// Intended to run from CI on `push: main`. PRs labeled `do-not-rebase`
    /// are skipped. Successful rebases force-push with a lease and post a
    /// `success` commit status (context: `auto-rebase`); conflicts post a
    /// `failure` status on the PR head and the workflow exits non-zero.
    RebaseOpenPrs {
        /// Print what would happen without rebasing or pushing
        #[arg(long)]
        dry_run: bool,
    },
    /// Run `nix flake update <input>` across every project that consumes the
    /// input, leaving the changed `flake.lock`s in the working copy.
    ///
    /// Intended to run from a scheduled CI workflow that commits the
    /// resulting diff and opens a single lockstep PR. Discovery is
    /// grep-based; the audit rule `kolohelios-nix-via-flakehub` enforces
    /// that consumers pin via the canonical FlakeHub URL.
    BumpLocks {
        /// Flake input name to update across all consuming projects
        #[arg(long)]
        input: String,
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
        RepoCommand::Status { json } => status::run(json),
        RepoCommand::RebaseOpenPrs { dry_run } => rebase_open_prs::run(dry_run),
        RepoCommand::BumpLocks { input } => bump_locks::run(input),
    }
}
