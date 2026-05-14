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
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
