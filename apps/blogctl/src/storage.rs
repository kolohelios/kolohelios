use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::post::{Post, PostMetadata};
use crate::stage::Stage;

const CONFIG_FILE: &str = ".blog-os.toml";
const README_FILE: &str = "README.md";

/// Default narrative theme. Used by `PostMetadata`'s serde default and by
/// `Config::current()` to seed `defaults.theme`.
pub const DEFAULT_THEME: &str = "standard";

/// Themes pre-declared by `init`. Keeps the validation list non-empty
/// out of the box so `blogctl new --theme parable` works on a fresh
/// workdir without the user editing the config first.
const STARTER_THEMES: &[&str] = &["standard", "parable"];

/// Canonical workdir README, baked in at build time. `init` writes this
/// file when one is not already present; `readme regenerate` overwrites
/// whatever is currently there.
pub const README_TEMPLATE: &str = include_str!("../templates/workdir-readme.md");

/// On-disk config persisted at `<workdir>/.blog-os.toml`. Versioned so we
/// can evolve the schema as features land (OpenRouter, Todoist, …).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub themes: BTreeMap<String, ThemeConfig>,
}

/// Workdir-wide defaults. Currently just the theme `blogctl new` selects
/// when `--theme` is not given; will grow as additional defaults land.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Defaults {
    #[serde(default = "default_theme_name")]
    pub theme: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            theme: DEFAULT_THEME.to_string(),
        }
    }
}

fn default_theme_name() -> String {
    DEFAULT_THEME.to_string()
}

/// Per-theme configuration. The `prompts` map is reserved for the
/// run-stage resolver (#233) — it stays empty in this slice but the
/// schema slot is here so the file format doesn't churn later.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ThemeConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prompts: BTreeMap<String, String>,
}

impl Config {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn current() -> Self {
        let themes = STARTER_THEMES
            .iter()
            .map(|name| (name.to_string(), ThemeConfig::default()))
            .collect();
        Self {
            version: Self::CURRENT_VERSION,
            defaults: Defaults::default(),
            themes,
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

    pub fn readme_path(&self) -> PathBuf {
        self.root.join(README_FILE)
    }

    pub fn stage_dir(&self, stage: Stage) -> PathBuf {
        self.root.join(stage.dirname())
    }

    pub fn post_path(&self, stage: Stage, slug: &str) -> PathBuf {
        self.stage_dir(stage).join(format!("{slug}.md"))
    }

    /// All directories that should exist after `init`. Only the stage
    /// directories — prompt files live at the workdir root next to
    /// `.blog-os.toml` and `README.md`.
    pub fn directories(&self) -> Vec<PathBuf> {
        Stage::ALL.iter().map(|s| self.stage_dir(*s)).collect()
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
        self.write_readme(false)?;
        Ok(())
    }

    pub fn read_config(&self) -> Result<Config> {
        let path = self.workdir.config_path();
        let raw = fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
        toml::from_str(&raw).map_err(|source| Error::ConfigParse { path, source })
    }

    /// Write the canonical README into the workdir. Returns `true` if
    /// the file was (over)written, `false` if a README already existed
    /// and `force` was false (init's path — preserves a user's
    /// pre-existing README).
    pub fn write_readme(&self, force: bool) -> Result<bool> {
        let path = self.workdir.readme_path();
        if path.exists() && !force {
            return Ok(false);
        }
        fs::write(&path, README_TEMPLATE).map_err(|e| Error::io(&path, e))?;
        Ok(true)
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
                kind: crate::kind::Kind::Post,
                theme: DEFAULT_THEME.to_string(),
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
    fn init_creates_every_stage_directory_and_config() {
        let (tmp, _repo) = fresh_repo();
        for &stage in Stage::ALL {
            assert!(tmp.path().join(stage.dirname()).is_dir(), "{}", stage);
        }
        assert!(tmp.path().join(".blog-os.toml").is_file());
    }

    #[test]
    fn init_does_not_create_prompts_or_history_subdirs() {
        let (tmp, _repo) = fresh_repo();
        assert!(!tmp.path().join("prompts").exists());
        assert!(!tmp.path().join("history").exists());
    }

    #[test]
    fn init_writes_versioned_config() {
        let (_tmp, repo) = fresh_repo();
        let cfg = repo.read_config().unwrap();
        assert_eq!(cfg.version, Config::CURRENT_VERSION);
    }

    #[test]
    fn init_seeds_default_theme_and_starter_themes() {
        let (_tmp, repo) = fresh_repo();
        let cfg = repo.read_config().unwrap();
        assert_eq!(cfg.defaults.theme, DEFAULT_THEME);
        assert!(cfg.themes.contains_key("standard"));
        assert!(cfg.themes.contains_key("parable"));
    }

    #[test]
    fn config_with_only_version_back_fills_defaults_and_themes() {
        let tmp = TempDir::new().unwrap();
        // Simulate a workdir from before the themes field landed: just
        // `version = 1` in the config file.
        let repo = Repository::unchecked(Workdir::new(tmp.path()));
        for dir in repo.workdir().directories() {
            fs::create_dir_all(&dir).unwrap();
        }
        fs::write(repo.workdir().config_path(), "version = 1\n").unwrap();
        let cfg = repo.read_config().unwrap();
        assert_eq!(cfg.defaults.theme, DEFAULT_THEME);
        assert!(cfg.themes.is_empty());
    }

    #[test]
    fn init_writes_readme_with_setup_directives() {
        let (tmp, _repo) = fresh_repo();
        let readme =
            fs::read_to_string(tmp.path().join("README.md")).expect("README.md present after init");
        assert!(readme.contains("jj git init --colocate"));
        assert!(readme.contains("--allow-new"));
        assert!(readme.contains("--allow-backwards"));
        assert!(readme.contains("blogctl readme regenerate"));
    }

    #[test]
    fn init_preserves_a_pre_existing_readme() {
        let tmp = TempDir::new().unwrap();
        let custom = "my hand-rolled README\n";
        fs::write(tmp.path().join("README.md"), custom).unwrap();
        let repo = Repository::unchecked(Workdir::new(tmp.path()));
        repo.init().unwrap();
        let readme = fs::read_to_string(tmp.path().join("README.md")).unwrap();
        assert_eq!(readme, custom, "init must not clobber a user's README");
    }

    #[test]
    fn write_readme_force_overwrites_existing() {
        let (tmp, repo) = fresh_repo();
        fs::write(tmp.path().join("README.md"), "stale").unwrap();
        let written = repo.write_readme(true).unwrap();
        assert!(written);
        let readme = fs::read_to_string(tmp.path().join("README.md")).unwrap();
        assert!(readme.contains("jj git init --colocate"));
    }

    #[test]
    fn write_readme_without_force_skips_existing() {
        let (tmp, repo) = fresh_repo();
        fs::write(tmp.path().join("README.md"), "user wrote this").unwrap();
        let written = repo.write_readme(false).unwrap();
        assert!(!written, "expected write_readme(false) to skip");
        let readme = fs::read_to_string(tmp.path().join("README.md")).unwrap();
        assert_eq!(readme, "user wrote this");
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
