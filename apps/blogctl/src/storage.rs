use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::post::{Post, PostMetadata};
use crate::stage::Stage;

const CONFIG_FILE: &str = ".blog-os.toml";
const HISTORY_DIR: &str = "history";
const HISTORY_PUBLISHED: &str = "history/published-posts";
const PROMPTS_DIR: &str = "prompts";

/// On-disk config persisted at `<workdir>/.blog-os.toml`. Versioned so we
/// can evolve the schema as features land (OpenRouter, Todoist, …).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub version: u32,
}

impl Config {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn current() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
        }
    }
}

/// A workdir rooted at a user-supplied path. Every method is path-pure —
/// no IO. The repository pairs a workdir with the filesystem.
#[derive(Debug, Clone)]
pub struct Workdir {
    root: PathBuf,
}

impl Workdir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE)
    }

    pub fn stage_dir(&self, stage: Stage) -> PathBuf {
        self.root.join(stage.dirname())
    }

    pub fn post_path(&self, stage: Stage, slug: &str) -> PathBuf {
        self.stage_dir(stage).join(format!("{slug}.md"))
    }

    /// All directories that should exist after `init`. Stage directories
    /// hold drafts; `history/` and `prompts/` are reserved for future
    /// features but are scaffolded eagerly so users don't have to mkdir
    /// before first use.
    pub fn directories(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Stage::ALL.iter().map(|s| self.stage_dir(*s)).collect();
        dirs.push(self.root.join(HISTORY_DIR));
        dirs.push(self.root.join(HISTORY_PUBLISHED));
        dirs.push(self.root.join(PROMPTS_DIR));
        dirs
    }
}

#[derive(Debug, Clone)]
pub struct PostHandle {
    pub stage: Stage,
    pub path: PathBuf,
    pub metadata: PostMetadata,
}

#[derive(Debug)]
pub struct Repository {
    workdir: Workdir,
}

impl Repository {
    pub fn open(workdir: Workdir) -> Result<Self> {
        if !workdir.config_path().is_file() {
            return Err(Error::WorkdirNotInitialized(workdir.root().to_path_buf()));
        }
        Ok(Self { workdir })
    }

    /// Construct a repository without verifying that the workdir is
    /// initialized. Used by `init` and tests; everything else should
    /// prefer `open`.
    pub fn unchecked(workdir: Workdir) -> Self {
        Self { workdir }
    }

    pub fn workdir(&self) -> &Workdir {
        &self.workdir
    }

    /// Create the directory layout and write a fresh config. Fails if a
    /// config already exists; missing intermediate directories are
    /// created with `create_dir_all`.
    pub fn init(&self) -> Result<()> {
        if self.workdir.config_path().exists() {
            return Err(Error::WorkdirAlreadyInitialized(
                self.workdir.root().to_path_buf(),
            ));
        }
        for dir in self.workdir.directories() {
            fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        }
        let config_path = self.workdir.config_path();
        let serialized = toml::to_string(&Config::current()).map_err(Error::ConfigSerialize)?;
        fs::write(&config_path, serialized).map_err(|e| Error::io(&config_path, e))?;
        Ok(())
    }

    pub fn read_config(&self) -> Result<Config> {
        let path = self.workdir.config_path();
        let raw = fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
        toml::from_str(&raw).map_err(|source| Error::ConfigParse { path, source })
    }

    /// Persist a brand-new post in its claimed stage directory. Fails if
    /// a file with that slug already exists in any stage.
    pub fn create_post(&self, post: &Post) -> Result<PathBuf> {
        if let Some(existing) = self.find_path(&post.metadata.slug)? {
            return Err(Error::DuplicateSlug(
                post.metadata.slug.clone(),
                existing.display().to_string(),
            ));
        }
        let stage = post.metadata.status;
        let dir = self.workdir.stage_dir(stage);
        fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let path = self.workdir.post_path(stage, &post.metadata.slug);
        let rendered = post.render()?;
        fs::write(&path, rendered).map_err(|e| Error::io(&path, e))?;
        Ok(path)
    }

    /// Load a post by slug. Fails if the post is missing, if the slug
    /// resolves in two stages, or if the directory disagrees with the
    /// frontmatter status.
    pub fn load(&self, slug: &str) -> Result<(PostHandle, Post)> {
        let (stage, path) = self
            .locate(slug)?
            .ok_or_else(|| Error::PostNotFound(slug.to_string()))?;
        let raw = fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
        let post = Post::parse(&path, &raw)?;
        if post.metadata.status != stage {
            return Err(Error::StageMismatch {
                slug: slug.to_string(),
                dir_stage: stage,
                fm_stage: post.metadata.status,
            });
        }
        let handle = PostHandle {
            stage,
            path,
            metadata: post.metadata.clone(),
        };
        Ok((handle, post))
    }

    /// Enumerate every post under the workdir, sorted by stage then slug.
    /// Fails on any frontmatter or stage mismatch — consistent with the
    /// "fail loudly" design constraint.
    pub fn list(&self) -> Result<Vec<PostHandle>> {
        let mut handles = Vec::new();
        for &stage in Stage::ALL {
            let dir = self.workdir.stage_dir(stage);
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(Error::io(&dir, err)),
            };
            for entry in entries {
                let entry = entry.map_err(|e| Error::io(&dir, e))?;
                let path = entry.path();
                if !is_markdown(&path) {
                    continue;
                }
                let raw = fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
                let post = Post::parse(&path, &raw)?;
                if post.metadata.status != stage {
                    return Err(Error::StageMismatch {
                        slug: post.metadata.slug,
                        dir_stage: stage,
                        fm_stage: post.metadata.status,
                    });
                }
                handles.push(PostHandle {
                    stage,
                    path,
                    metadata: post.metadata,
                });
            }
        }
        handles.sort_by(|a, b| {
            (a.stage as u8, &a.metadata.slug).cmp(&(b.stage as u8, &b.metadata.slug))
        });
        Ok(handles)
    }

    /// Move a post into a new stage directory and rewrite its frontmatter
    /// status + `updated_at`. The body is preserved verbatim.
    pub fn relocate(
        &self,
        slug: &str,
        new_stage: Stage,
        now: OffsetDateTime,
    ) -> Result<PostHandle> {
        let (handle, mut post) = self.load(slug)?;
        let old_stage = handle.stage;
        if old_stage == new_stage {
            return Ok(handle);
        }

        post.metadata.status = new_stage;
        post.metadata.updated_at = now;

        let new_dir = self.workdir.stage_dir(new_stage);
        fs::create_dir_all(&new_dir).map_err(|e| Error::io(&new_dir, e))?;
        let new_path = self.workdir.post_path(new_stage, slug);
        let rendered = post.render()?;
        fs::write(&new_path, rendered).map_err(|e| Error::io(&new_path, e))?;
        fs::remove_file(&handle.path).map_err(|e| Error::io(&handle.path, e))?;

        Ok(PostHandle {
            stage: new_stage,
            path: new_path,
            metadata: post.metadata,
        })
    }

    fn locate(&self, slug: &str) -> Result<Option<(Stage, PathBuf)>> {
        let mut found: Option<(Stage, PathBuf)> = None;
        for &stage in Stage::ALL {
            let candidate = self.workdir.post_path(stage, slug);
            if candidate.is_file() {
                if let Some((_, existing_path)) = &found {
                    return Err(Error::DuplicateSlug(
                        slug.to_string(),
                        format!("{} and {}", existing_path.display(), candidate.display(),),
                    ));
                }
                found = Some((stage, candidate));
            }
        }
        Ok(found)
    }

    fn find_path(&self, slug: &str) -> Result<Option<PathBuf>> {
        Ok(self.locate(slug)?.map(|(_, p)| p))
    }
}

fn is_markdown(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("md"))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use time::macros::datetime;

    fn fixture_post(slug: &str, stage: Stage) -> Post {
        Post::new(
            PostMetadata {
                title: format!("Title {slug}"),
                slug: slug.to_string(),
                status: stage,
                created_at: datetime!(2026-05-03 00:00:00 UTC),
                updated_at: datetime!(2026-05-03 00:00:00 UTC),
                tags: vec![],
                todoist_task_id: None,
                history_checked: false,
            },
            "body\n",
        )
    }

    fn fresh_repo() -> (TempDir, Repository) {
        let tmp = TempDir::new().unwrap();
        let repo = Repository::unchecked(Workdir::new(tmp.path()));
        repo.init().unwrap();
        (tmp, repo)
    }

    #[test]
    fn init_creates_every_stage_and_extension_directory() {
        let (tmp, _repo) = fresh_repo();
        for &stage in Stage::ALL {
            assert!(tmp.path().join(stage.dirname()).is_dir(), "{}", stage);
        }
        assert!(tmp.path().join("history/published-posts").is_dir());
        assert!(tmp.path().join("prompts").is_dir());
        assert!(tmp.path().join(".blog-os.toml").is_file());
    }

    #[test]
    fn init_writes_versioned_config() {
        let (_tmp, repo) = fresh_repo();
        let cfg = repo.read_config().unwrap();
        assert_eq!(cfg.version, Config::CURRENT_VERSION);
    }

    #[test]
    fn init_refuses_to_clobber_existing_config() {
        let (_tmp, repo) = fresh_repo();
        let err = repo.init().unwrap_err();
        assert!(matches!(err, Error::WorkdirAlreadyInitialized(_)));
    }

    #[test]
    fn open_requires_an_initialized_workdir() {
        let tmp = TempDir::new().unwrap();
        let err = Repository::open(Workdir::new(tmp.path())).unwrap_err();
        assert!(matches!(err, Error::WorkdirNotInitialized(_)));
    }

    #[test]
    fn create_post_writes_to_stage_directory() {
        let (tmp, repo) = fresh_repo();
        let post = fixture_post("hello-world", Stage::Concept);
        let path = repo.create_post(&post).unwrap();
        assert!(path.starts_with(tmp.path().join("concepts")));
        assert!(path.is_file());
    }

    #[test]
    fn create_post_rejects_duplicate_slug() {
        let (_tmp, repo) = fresh_repo();
        repo.create_post(&fixture_post("dup", Stage::Concept))
            .unwrap();
        let err = repo
            .create_post(&fixture_post("dup", Stage::Concept))
            .unwrap_err();
        assert!(matches!(err, Error::DuplicateSlug(_, _)));
    }

    #[test]
    fn relocate_moves_file_and_updates_metadata() {
        let (tmp, repo) = fresh_repo();
        let post = fixture_post("rust-2026", Stage::Concept);
        repo.create_post(&post).unwrap();
        let later = datetime!(2026-05-04 12:00:00 UTC);
        let handle = repo.relocate("rust-2026", Stage::Ideation, later).unwrap();

        assert_eq!(handle.stage, Stage::Ideation);
        assert!(!tmp.path().join("concepts/rust-2026.md").exists());
        assert!(tmp.path().join("ideation/rust-2026.md").is_file());
        assert_eq!(handle.metadata.status, Stage::Ideation);
        assert_eq!(handle.metadata.updated_at, later);
    }

    #[test]
    fn list_returns_posts_sorted_by_stage_then_slug() {
        let (_tmp, repo) = fresh_repo();
        repo.create_post(&fixture_post("b-second", Stage::Editing))
            .unwrap();
        repo.create_post(&fixture_post("a-first", Stage::Concept))
            .unwrap();
        repo.create_post(&fixture_post("c-third", Stage::Concept))
            .unwrap();
        let handles = repo.list().unwrap();
        let slugs: Vec<&str> = handles.iter().map(|h| h.metadata.slug.as_str()).collect();
        assert_eq!(slugs, vec!["a-first", "c-third", "b-second"]);
    }

    #[test]
    fn load_detects_stage_mismatch_between_dir_and_frontmatter() {
        let (tmp, repo) = fresh_repo();
        // Hand-write a post into the wrong directory.
        let stray = fixture_post("stray", Stage::Editing);
        let path = tmp.path().join("concepts/stray.md");
        fs::write(&path, stray.render().unwrap()).unwrap();

        let err = repo.load("stray").unwrap_err();
        assert!(matches!(err, Error::StageMismatch { .. }));
    }

    #[test]
    fn load_returns_post_not_found_for_missing_slug() {
        let (_tmp, repo) = fresh_repo();
        let err = repo.load("nope").unwrap_err();
        assert!(matches!(err, Error::PostNotFound(_)));
    }

    #[test]
    fn load_detects_duplicate_slug_across_stages() {
        let (tmp, repo) = fresh_repo();
        let p1 = fixture_post("dup", Stage::Concept);
        fs::write(tmp.path().join("concepts/dup.md"), p1.render().unwrap()).unwrap();
        let p2 = fixture_post("dup", Stage::Editing);
        fs::write(tmp.path().join("editing/dup.md"), p2.render().unwrap()).unwrap();

        let err = repo.load("dup").unwrap_err();
        assert!(matches!(err, Error::DuplicateSlug(_, _)));
    }

    #[test]
    fn relocate_to_same_stage_is_a_noop() {
        let (_tmp, repo) = fresh_repo();
        repo.create_post(&fixture_post("same", Stage::Concept))
            .unwrap();
        let handle = repo
            .relocate("same", Stage::Concept, datetime!(2026-05-04 0:00 UTC))
            .unwrap();
        assert_eq!(handle.stage, Stage::Concept);
    }
}
