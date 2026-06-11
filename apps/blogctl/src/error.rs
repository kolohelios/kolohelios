use std::path::PathBuf;

use snafu::Snafu;

use crate::stage::Stage;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    #[snafu(display("workdir not initialized at {} (run `blogctl init`)", path.display()))]
    WorkdirNotInitialized { path: PathBuf },

    #[snafu(display("workdir already initialized at {}", path.display()))]
    WorkdirAlreadyInitialized { path: PathBuf },

    #[snafu(display("post not found: {slug}"))]
    PostNotFound { slug: String },

    #[snafu(display(
        "post {slug:?} disagrees with its location: directory says {dir_stage}, frontmatter says {fm_stage}"
    ))]
    StageMismatch {
        slug: String,
        dir_stage: Stage,
        fm_stage: Stage,
    },

    #[snafu(display("multiple posts share slug {slug:?}: {paths}"))]
    DuplicateSlug { slug: String, paths: String },

    #[snafu(display("cannot promote from {stage}: already at the final workflow stage"))]
    PromoteFromTerminal { stage: Stage },

    #[snafu(display(
        "cannot promote {slug:?} from {from} to {to}: exit predicate {predicate} not satisfied"
    ))]
    PromoteBlocked {
        slug: String,
        from: Stage,
        to: Stage,
        predicate: String,
    },

    #[snafu(display(
        "cannot draft post {slug:?} from stage {stage}: drafting requires the post to be in the `ideation` stage"
    ))]
    DraftWrongStage { slug: String, stage: Stage },

    #[snafu(display(
        "cannot refine post {slug:?} from stage {stage}: refining requires the post to be in the `editing` stage"
    ))]
    RefineWrongStage { slug: String, stage: Stage },

    #[snafu(display(
        "cannot final-edit post {slug:?} from stage {stage}: the polish pass requires the post to be in the `final-editing` stage"
    ))]
    FinalEditWrongStage { slug: String, stage: Stage },

    #[snafu(display("cannot demote from {stage}: already at the first workflow stage"))]
    DemoteFromInitial { stage: Stage },

    #[snafu(display("cannot demote a published post (use --force when implemented)"))]
    DemotePublished,

    #[snafu(display("invalid stage {value:?}"))]
    InvalidStage { value: String },

    #[snafu(display("invalid kind {value:?}: expected `post` or `article`"))]
    InvalidKind { value: String },

    #[snafu(display("invalid target {value:?}: expected `linkedin` or `blog`"))]
    InvalidTarget { value: String },

    #[snafu(display(
        "invalid target status {value:?}: expected `planned`, `published`, or `retracted`"
    ))]
    InvalidTargetStatus { value: String },

    #[snafu(display(
        "duplicate target {name} in {} (each target may appear at most once)",
        path.display()
    ))]
    DuplicateTarget {
        path: PathBuf,
        name: crate::target::Target,
    },

    #[snafu(display(
        "target {name} in {} has status `published` but `url` is not set",
        path.display()
    ))]
    PublishedTargetMissingUrl {
        path: PathBuf,
        name: crate::target::Target,
    },

    #[snafu(display(
        "target {name} in {} has status `published` but `published_at` is not set",
        path.display()
    ))]
    PublishedTargetMissingPublishedAt {
        path: PathBuf,
        name: crate::target::Target,
    },

    #[snafu(display("unknown theme {theme:?}: known themes are {}", known.join(", ")))]
    UnknownTheme { theme: String, known: Vec<String> },

    #[snafu(display("invalid slug {value:?}: {reason}"))]
    InvalidSlug { value: String, reason: String },

    #[snafu(display("invalid title {value:?}: cannot derive slug"))]
    EmptyTitle { value: String },

    #[snafu(display("missing frontmatter delimiter `---` at start of {}", path.display()))]
    FrontmatterMissingOpen { path: PathBuf },

    #[snafu(display("missing closing frontmatter delimiter `---` in {}", path.display()))]
    FrontmatterMissingClose { path: PathBuf },

    #[snafu(display("could not parse frontmatter in {}: {source}", path.display()))]
    FrontmatterParse {
        path: PathBuf,
        source: serde_yaml_ng::Error,
    },

    #[snafu(display("could not serialize frontmatter: {source}"))]
    FrontmatterSerialize { source: serde_yaml_ng::Error },

    #[snafu(display("could not parse config {}: {source}", path.display()))]
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[snafu(display("could not serialize config: {source}"))]
    ConfigSerialize { source: toml::ser::Error },

    #[snafu(display("could not format timestamp: {source}"))]
    TimeFormat { source: time::error::Format },

    #[snafu(display("io error at {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("could not resolve current working directory: {source}"))]
    CurrentDirUnavailable { source: std::io::Error },

    #[snafu(display(
        "workdir unhealthy: {findings} finding(s); run `blogctl fix` to repair the auto-correctable subset"
    ))]
    WorkdirUnhealthy { findings: usize },

    #[snafu(display(
        "OPENROUTER_API_KEY not set — invoke through `op run --env-file=.env -- ...` or set it in the environment"
    ))]
    OpenrouterApiKeyMissing,

    #[snafu(display("OpenRouter request failed: {message}"))]
    OpenrouterRequest { message: String },

    #[snafu(display("OpenRouter response contained no choices"))]
    OpenrouterEmptyResponse,

    #[snafu(display("could not parse predicate: {message}"))]
    PredicateParse { message: String },

    #[snafu(display("could not evaluate predicate {predicate:?}: {reason}"))]
    PredicateEval { predicate: String, reason: String },

    #[snafu(display("could not invoke `{command}`: {source}"))]
    JjInvoke {
        command: String,
        source: std::io::Error,
    },

    #[snafu(display("`{command}` failed (exit {status}): {stderr}"))]
    JjCommandFailed {
        command: String,
        status: i32,
        stderr: String,
    },

    #[snafu(display(
        "bookmark `{bookmark}` is in a conflicted state ({} candidate(s): {}); resolve via `jj bookmark set {bookmark} -r <REVISION>`",
        candidates.len(),
        candidates.join(", "),
    ))]
    JjBookmarkConflicted {
        bookmark: String,
        candidates: Vec<String>,
    },

    #[snafu(display(
        "rebase of @ onto {bookmark}@{remote} produced conflicts — resolve via `jj` before re-running blogctl"
    ))]
    SyncRebaseConflict { bookmark: String, remote: String },

    #[snafu(display(
        "@ has commits from side branch(es) {} below it; run `blogctl update` to bring @ on top of {bookmark}",
        side_bookmarks.join(", "),
    ))]
    WorkdirOnSideBranch {
        bookmark: String,
        side_bookmarks: Vec<String>,
    },

    #[snafu(display(
        "local {bookmark} has {count} unpushed commit(s); resolve via `jj git push --bookmark {bookmark}` before running `blogctl update`"
    ))]
    UpdateHasUnpushedCommits { bookmark: String, count: usize },

    #[snafu(display(
        "`blogctl update` requires sync to be enabled (config: `sync.enabled = true`, and don't pass `--no-sync`)"
    ))]
    UpdateRequiresSyncEnabled,

    #[snafu(display(
        "`blogctl update` needs `jj`: install it (and run `jj git init --colocate` in the workdir)"
    ))]
    UpdateRequiresJj,

    #[snafu(display(
        "`blogctl {command}` is not yet implemented (CLI surface only; behavior lands in a follow-up)"
    ))]
    Unimplemented { command: &'static str },

    #[snafu(display(
        "invalid classification in {}: {dimension}={value:?} is not in the taxonomy. allowed values: {}",
        path.display(),
        allowed.join(", "),
    ))]
    InvalidClassification {
        path: PathBuf,
        dimension: String,
        value: String,
        allowed: Vec<String>,
    },

    #[snafu(display("could not serialize analytics summary to JSON: {source}"))]
    SummaryJson { source: serde_json::Error },

    #[snafu(display(
        "target {target} is not on post {slug:?} — promote it via the targets editing flow before running `metrics update`"
    ))]
    TargetNotInPost {
        slug: String,
        target: crate::target::Target,
    },

    #[snafu(display("invalid --sampled-at {value:?}: expected RFC 3339 timestamp ({source})"))]
    InvalidSampledAt {
        value: String,
        source: time::error::Parse,
    },

    #[snafu(display("invalid --published-at {value:?}: expected RFC 3339 timestamp ({source})"))]
    InvalidPublishedAt {
        value: String,
        source: time::error::Parse,
    },

    #[snafu(display("could not parse backfill import file: {source}"))]
    BackfillImportParse { source: serde_json::Error },

    #[snafu(display("backfill completed with {warnings} warning(s) — see stderr for details"))]
    BackfillPartialFailure { warnings: usize },

    #[snafu(display(
        "invalid --stale-after {value:?}: expected a humantime duration like `7d` or `24h` ({source})"
    ))]
    InvalidStaleAfter {
        value: String,
        source: humantime::DurationError,
    },

    #[snafu(display("could not open LinkedIn export {}: {source}", path.display()))]
    LinkedinOpen {
        path: PathBuf,
        source: calamine::Error,
    },

    #[snafu(display("LinkedIn export {} has no `TOP POSTS` sheet", path.display()))]
    LinkedinSheetMissing { path: PathBuf },

    #[snafu(display("could not read the `TOP POSTS` sheet in {}: {source}", path.display()))]
    LinkedinSheetRead {
        path: PathBuf,
        source: calamine::Error,
    },

    #[snafu(display(
        "LinkedIn export {} has no `Post URL` header in its `TOP POSTS` sheet",
        path.display()
    ))]
    LinkedinHeaderMissing { path: PathBuf },

    #[snafu(display(
        "post URL {url:?} in {} has no `urn:li:activity:<id>` segment",
        path.display()
    ))]
    LinkedinMissingUrn { path: PathBuf, url: String },

    #[snafu(display(
        "could not parse publish date {value:?} in {}: expected M/D/YYYY ({source})",
        path.display()
    ))]
    LinkedinPublishDate {
        path: PathBuf,
        value: String,
        source: time::error::Parse,
    },

    #[snafu(display(
        "could not derive a snapshot date from export filename {name:?}: expected `Content_<YYYY-MM-DD>_<YYYY-MM-DD>_*.xlsx`"
    ))]
    LinkedinFilename { name: String },
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
