use std::path::PathBuf;

use crate::stage::Stage;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("workdir not initialized at {0} (run `blogctl init`)")]
    WorkdirNotInitialized(PathBuf),

    #[error("workdir already initialized at {0}")]
    WorkdirAlreadyInitialized(PathBuf),

    #[error("post not found: {0}")]
    PostNotFound(String),

    #[error(
        "post {slug:?} disagrees with its location: directory says {dir_stage}, frontmatter says {fm_stage}"
    )]
    StageMismatch {
        slug: String,
        dir_stage: Stage,
        fm_stage: Stage,
    },

    #[error("multiple posts share slug {0:?}: {1}")]
    DuplicateSlug(String, String),

    #[error("cannot promote from {0}: already at the final workflow stage")]
    PromoteFromTerminal(Stage),

    #[error(
        "cannot promote {slug:?} from {from} to {to}: exit predicate {predicate} not satisfied"
    )]
    PromoteBlocked {
        slug: String,
        from: Stage,
        to: Stage,
        predicate: String,
    },

    #[error(
        "cannot draft post {slug:?} from stage {stage}: drafting requires the post to be in the `ideation` stage"
    )]
    DraftWrongStage { slug: String, stage: Stage },

    #[error(
        "cannot refine post {slug:?} from stage {stage}: refining requires the post to be in the `editing` stage"
    )]
    RefineWrongStage { slug: String, stage: Stage },

    #[error(
        "cannot final-edit post {slug:?} from stage {stage}: the polish pass requires the post to be in the `final-editing` stage"
    )]
    FinalEditWrongStage { slug: String, stage: Stage },

    #[error("cannot demote from {0}: already at the first workflow stage")]
    DemoteFromInitial(Stage),

    #[error("cannot demote a published post (use --force when implemented)")]
    DemotePublished,

    #[error("invalid stage {0:?}")]
    InvalidStage(String),

    #[error("invalid kind {0:?}: expected `post` or `article`")]
    InvalidKind(String),

    #[error("invalid target {0:?}: expected `linkedin` or `blog`")]
    InvalidTarget(String),

    #[error("invalid target status {0:?}: expected `planned`, `published`, or `retracted`")]
    InvalidTargetStatus(String),

    #[error("duplicate target {name} in {path} (each target may appear at most once)")]
    DuplicateTarget {
        path: PathBuf,
        name: crate::target::Target,
    },

    #[error("target {name} in {path} has status `published` but `url` is not set")]
    PublishedTargetMissingUrl {
        path: PathBuf,
        name: crate::target::Target,
    },

    #[error("target {name} in {path} has status `published` but `published_at` is not set")]
    PublishedTargetMissingPublishedAt {
        path: PathBuf,
        name: crate::target::Target,
    },

    #[error("unknown theme {theme:?}: known themes are {}", known.join(", "))]
    UnknownTheme { theme: String, known: Vec<String> },

    #[error("invalid slug {0:?}: {1}")]
    InvalidSlug(String, String),

    #[error("invalid title {0:?}: cannot derive slug")]
    EmptyTitle(String),

    #[error("missing frontmatter delimiter `---` at start of {0}")]
    FrontmatterMissingOpen(PathBuf),

    #[error("missing closing frontmatter delimiter `---` in {0}")]
    FrontmatterMissingClose(PathBuf),

    #[error("could not parse frontmatter in {path}: {source}")]
    FrontmatterParse {
        path: PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },

    #[error("could not serialize frontmatter: {0}")]
    FrontmatterSerialize(#[source] serde_yaml_ng::Error),

    #[error("could not parse config {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("could not serialize config: {0}")]
    ConfigSerialize(#[source] toml::ser::Error),

    #[error("could not format timestamp: {0}")]
    TimeFormat(#[source] time::error::Format),

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not resolve current working directory: {0}")]
    CurrentDirUnavailable(#[source] std::io::Error),

    #[error(
        "workdir unhealthy: {0} finding(s); run `blogctl fix` to repair the auto-correctable subset"
    )]
    WorkdirUnhealthy(usize),

    #[error(
        "OPENROUTER_API_KEY not set — invoke through `op run --env-file=.env -- ...` or set it in the environment"
    )]
    OpenrouterApiKeyMissing,

    #[error("OpenRouter request failed: {0}")]
    OpenrouterRequest(String),

    #[error("OpenRouter response contained no choices")]
    OpenrouterEmptyResponse,

    #[error("could not parse predicate: {0}")]
    PredicateParse(String),

    #[error("could not evaluate predicate {predicate:?}: {reason}")]
    PredicateEval { predicate: String, reason: String },

    #[error("could not invoke `{command}`: {source}")]
    JjInvoke {
        command: String,
        #[source]
        source: std::io::Error,
    },

    #[error("`{command}` failed (exit {status}): {stderr}")]
    JjCommandFailed {
        command: String,
        status: i32,
        stderr: String,
    },

    #[error(
        "bookmark `{bookmark}` is in a conflicted state ({} candidate(s): {}); resolve via `jj bookmark set {bookmark} -r <REVISION>`",
        candidates.len(),
        candidates.join(", "),
    )]
    JjBookmarkConflicted {
        bookmark: String,
        candidates: Vec<String>,
    },

    #[error(
        "rebase of @ onto {bookmark}@{remote} produced conflicts — resolve via `jj` before re-running blogctl"
    )]
    SyncRebaseConflict { bookmark: String, remote: String },

    #[error(
        "@ has commits from side branch(es) {} below it; run `blogctl update` to bring @ on top of {bookmark}",
        side_bookmarks.join(", "),
    )]
    WorkdirOnSideBranch {
        bookmark: String,
        side_bookmarks: Vec<String>,
    },

    #[error(
        "@ has uncommitted changes; commit via a blogctl write command (or `jj describe @`) before running `blogctl update`"
    )]
    UpdateHasUncommittedEdits,

    #[error(
        "local {bookmark} has {count} unpushed commit(s); resolve via `jj git push --bookmark {bookmark}` before running `blogctl update`"
    )]
    UpdateHasUnpushedCommits { bookmark: String, count: usize },

    #[error("`blogctl update` requires sync to be enabled (config: `sync.enabled = true`, and don't pass `--no-sync`)")]
    UpdateRequiresSyncEnabled,

    #[error(
        "`blogctl update` needs `jj`: install it (and run `jj git init --colocate` in the workdir)"
    )]
    UpdateRequiresJj,

    #[error(
        "`blogctl {0}` is not yet implemented (CLI surface only; behavior lands in a follow-up)"
    )]
    Unimplemented(&'static str),

    #[error(
        "invalid classification in {path}: {dimension}={value:?} is not in the taxonomy. allowed values: {}",
        allowed.join(", "),
    )]
    InvalidClassification {
        path: PathBuf,
        dimension: String,
        value: String,
        allowed: Vec<String>,
    },

    #[error("could not serialize analytics summary to JSON: {0}")]
    SummaryJson(#[source] serde_json::Error),

    #[error(
        "target {target} is not on post {slug:?} — promote it via the targets editing flow before running `metrics update`"
    )]
    TargetNotInPost {
        slug: String,
        target: crate::target::Target,
    },

    #[error("invalid --sampled-at {value:?}: expected RFC 3339 timestamp ({source})")]
    InvalidSampledAt {
        value: String,
        #[source]
        source: time::error::Parse,
    },

    #[error("could not parse backfill import file: {0}")]
    BackfillImportParse(#[source] serde_json::Error),

    #[error("backfill completed with {0} warning(s) — see stderr for details")]
    BackfillPartialFailure(usize),

    #[error("invalid --stale-after {value:?}: expected a humantime duration like `7d` or `24h` ({source})")]
    InvalidStaleAfter {
        value: String,
        #[source]
        source: humantime::DurationError,
    },
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
