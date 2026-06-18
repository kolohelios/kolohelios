mod audit;
mod bump;
pub(crate) mod bump_locks;
pub mod describe;
pub(crate) mod policy;
mod pr;
mod rebase_open_prs;
mod rebase_wip;
pub mod send;
mod ship;
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

        /// Emit a machine-readable JSON report instead of grouped tables.
        /// Read-only, so it cannot be combined with --fix.
        #[arg(long, conflicts_with = "fix")]
        json: bool,
    },
    /// Fetch from origin and rebase the current change onto main@origin
    Sync {
        /// Print the commands that would run without executing them
        #[arg(long)]
        dry_run: bool,
        /// Suppress the post-sync hint listing workspaces eligible for
        /// `shaka workspace cleanup`. The sync itself runs unchanged.
        #[arg(long)]
        no_cleanup_hint: bool,
    },
    /// Refresh an in-flight PR's bookmark onto the latest main@origin
    ///
    /// Fetches, rebases the whole stack rooted at the bookmark onto
    /// `main@origin`, force-pushes the bookmark, and prints the PR url
    /// (if one exists). The bookmark is auto-detected as the unique
    /// bookmark in `main@origin..@`; pass `--bookmark` to disambiguate
    /// or override.
    Bump {
        /// Bookmark to refresh (auto-detected from main@origin..@ if omitted)
        #[arg(long)]
        bookmark: Option<String>,

        /// Print the commands that would run without executing them
        #[arg(long)]
        dry_run: bool,
    },
    /// Fetch, rebase onto main@origin, push, and open a PR
    ///
    /// Always fetches and rebases the bookmark onto the latest
    /// `main@origin` immediately before push so the PR opens on a
    /// current base. If the rebase pulls in new ancestors (i.e.,
    /// `main` moved during the work), re-runs `shaka preflight --since
    /// main@origin` first — new commits can break our changes in ways
    /// `jj` doesn't flag with conflict markers. If the rebase is a
    /// no-op, preflight is skipped.
    Send {
        /// Bookmark name (auto-derived from the change description if omitted)
        #[arg(long)]
        bookmark: Option<String>,

        /// Push only — do not create a PR even if one is missing
        #[arg(long)]
        no_pr: bool,

        /// Skip queueing GitHub auto-merge after PR creation. The PR
        /// will be opened (and any existing PR left in place) but the
        /// merge has to be triggered manually.
        #[arg(long)]
        no_auto_merge: bool,

        /// Print the commands that would run without executing them
        #[arg(long)]
        dry_run: bool,
    },
    /// Ensure a GitHub PR exists for the current change (push, then create if needed)
    Pr {
        /// Bookmark name (auto-detected from the current change if omitted)
        #[arg(long)]
        bookmark: Option<String>,

        /// Skip queueing GitHub auto-merge after PR creation. The PR
        /// will be opened (and any existing PR left in place) but the
        /// merge has to be triggered manually.
        #[arg(long)]
        no_auto_merge: bool,

        /// Print the commands that would run without executing them
        #[arg(long)]
        dry_run: bool,
    },
    /// Run the opinionated ship workflow: rebase on main@origin, lint
    /// commits, self-review the diff, run preflight, push, and ensure a PR.
    ///
    /// Halts on the first failure. Errors from `commit lint` (non-conformant
    /// commits) hard-stop — fix via `jj describe` and rerun. The push step
    /// updates an existing PR's head ref automatically; PR title/body are
    /// not re-synced.
    Ship {
        /// Bookmark name (auto-detected from the current change if omitted)
        #[arg(long)]
        bookmark: Option<String>,

        /// Skip the preflight step (use when you've already run it locally)
        #[arg(long)]
        skip_preflight: bool,

        /// Print the steps that would run without executing them
        #[arg(long)]
        dry_run: bool,
    },
    /// Print a one-shot summary of the current working-copy state
    Status {
        /// Emit JSON instead of human-readable output
        #[arg(long)]
        json: bool,
    },
    /// Synthesize a PR title and body from commits in main@origin..@
    ///
    /// Title is the tip commit's title verbatim. Body concatenates each
    /// commit's "why" paragraph (the body up to any trailer footer like
    /// `Closes #N` or `Co-authored-by:`) in chronological order, joined
    /// by blank lines. Per project convention, no test plan section.
    Describe {
        /// Emit JSON (`{"title": "...", "body": "..."}`) instead of
        /// human-readable text
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
    /// Rebase every local WIP bookmark onto main@origin
    ///
    /// Fetches first, then for each local bookmark ahead of `main@origin`
    /// (excluding `main` itself) runs `jj rebase -b <name> -d main@origin`
    /// and classifies the outcome as clean / conflict / up-to-date. With
    /// `--push`, the clean ones get pushed; conflicts are left for manual
    /// resolution.
    RebaseWip {
        /// Also push the cleanly-rebased bookmarks
        #[arg(long)]
        push: bool,
    },
    /// Run `nix flake update <input>` across every project that consumes the
    /// input, leaving the changed `flake.lock`s in the working copy.
    ///
    /// Intended to run from a scheduled CI workflow. Without `--pr-branch`,
    /// changes stay in the working copy. With `--pr-branch <name>`, after
    /// bumping shaka switches to that branch, commits, force-pushes, and
    /// opens (or, if one is already open for the branch, updates) a single
    /// lockstep PR. Discovery is grep-based; the audit rule
    /// `kolohelios-nix-via-flakehub` enforces that consumers pin via the
    /// canonical FlakeHub URL.
    BumpLocks {
        /// Flake input name to update across all consuming projects
        #[arg(long)]
        input: String,
        /// If set, after bumping, switch to this branch, commit the changes,
        /// force-push, and open or update a PR. Requires GH_TOKEN with PR
        /// write scope and a clean working copy on entry. Required when
        /// `--repo` is set.
        #[arg(long)]
        pr_branch: Option<String>,
        /// If set, operate on the named remote repo (`<owner>/<slug>`)
        /// instead of the current monorepo. Clones into a temp directory,
        /// bumps the input in the root flake, and opens/updates a PR via
        /// `--pr-branch` (which is required in this mode). The workflow
        /// must set `GH_TOKEN` and pre-configure git identity + credential
        /// helper (`gh auth setup-git`) before invoking shaka.
        #[arg(long)]
        repo: Option<String>,
        /// After creating or updating the PR, queue GitHub auto-merge
        /// (`gh pr merge --auto --rebase`). The PR merges as soon as the
        /// repo's required checks pass. Requires `allow_auto_merge: true`
        /// on the target repo; merge strategy is fixed to rebase to match
        /// repo policy.
        #[arg(long)]
        auto_merge: bool,
    },
}

pub fn run(cmd: RepoCommand) {
    match cmd {
        RepoCommand::Audit { repo, fix, json } => audit::run(repo, fix, json),
        RepoCommand::Sync {
            dry_run,
            no_cleanup_hint,
        } => sync::run(dry_run, no_cleanup_hint),
        RepoCommand::Bump { bookmark, dry_run } => bump::run(bookmark, dry_run),
        RepoCommand::Send {
            bookmark,
            no_pr,
            no_auto_merge,
            dry_run,
        } => send::run(bookmark, no_pr, no_auto_merge, dry_run),
        RepoCommand::Pr {
            bookmark,
            no_auto_merge,
            dry_run,
        } => pr::run(bookmark, no_auto_merge, dry_run),
        RepoCommand::Ship {
            bookmark,
            skip_preflight,
            dry_run,
        } => ship::run(bookmark, skip_preflight, dry_run),
        RepoCommand::Status { json } => status::run(json),
        RepoCommand::Describe { json } => describe::run(json),
        RepoCommand::RebaseOpenPrs { dry_run } => rebase_open_prs::run(dry_run),
        RepoCommand::RebaseWip { push } => rebase_wip::run(push),
        RepoCommand::BumpLocks {
            input,
            pr_branch,
            repo,
            auto_merge,
        } => bump_locks::run(input, pr_branch, repo, auto_merge),
    }
}
