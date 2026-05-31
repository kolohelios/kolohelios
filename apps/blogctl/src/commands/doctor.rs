//! Read-only audit of a workdir's health. `audit()` returns the full
//! list of `Finding`s; `run()` prints them and returns
//! `Error::WorkdirUnhealthy` when the list is non-empty so the binary
//! exits non-zero. `fix` (next slice) calls `audit()` directly.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::stage::Stage;
use crate::storage::{AuditedPost, Config, ConfigStatus, Repository, StrayEntry, Workdir};
use crate::sync::{Jj, Status};

/// Threshold for flagging unpushed commits as stale. Past this age,
/// the auto-push is probably failing silently (network gone bad,
/// remote ref renamed, auth expired). `doctor` surfaces it; the
/// user is expected to investigate (no auto-fix).
pub const STALE_UNPUSHED_THRESHOLD_HOURS: i64 = 24;

#[derive(Debug)]
pub enum Finding {
    /// The `jj` binary is not on `$PATH`. blogctl writes assume a
    /// VC-tracked workdir; without `jj`, every change accumulates
    /// silently with no commit trail.
    JjNotInstalled,
    /// The workdir exists but is not a `jj` repository. Same hazard
    /// as `JjNotInstalled` — writes have no commit trail until the
    /// user runs `jj git init --colocate` (or equivalent).
    NotAJjRepo {
        workdir: PathBuf,
    },
    ConfigMissing,
    ConfigUnparsable {
        error: Error,
    },
    StageDirMissing {
        stage: Stage,
    },
    FrontmatterParseError {
        path: PathBuf,
        error: Error,
    },
    StageMismatch {
        path: PathBuf,
        dir_stage: Stage,
        fm_stage: Stage,
    },
    FilenameSlugMismatch {
        path: PathBuf,
        slug: String,
    },
    DuplicateSlug {
        slug: String,
        paths: Vec<PathBuf>,
    },
    UnknownTheme {
        path: PathBuf,
        theme: String,
        known: Vec<String>,
    },
    /// `defaults.theme` names a theme that has no `[themes.<theme>]`
    /// entry. Pre-themes workdirs (config has only `version = 1`)
    /// trip this on every write — the read path back-fills the field
    /// but the write path rejects every theme value.
    DefaultThemeMissing {
        theme: String,
    },
    TimestampInversion {
        path: PathBuf,
        created_at: OffsetDateTime,
        updated_at: OffsetDateTime,
    },
    StrayEntry {
        dir_stage: Stage,
        path: PathBuf,
    },
    /// Local commits on `<bookmark>` have not been pushed to
    /// `<bookmark>@<remote>` for `oldest_age_hours`+, suggesting
    /// auto-push has been silently failing.
    UnpushedCommitsStale {
        count: usize,
        oldest_age_hours: i64,
        bookmark: String,
        remote: String,
    },
    /// A classification value on `path` is not in the workdir's
    /// declared taxonomy (`[classifications.<dimension>]`).
    InvalidClassification {
        path: PathBuf,
        dimension: String,
        value: String,
        allowed: Vec<String>,
    },
    /// The post's `tags` list overruns the workdir's declared cap
    /// (`[tags] max = N`). Workdirs without a cap declared never
    /// trip this.
    TagsExceedMax {
        path: PathBuf,
        count: usize,
        max: u32,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JjNotInstalled => write!(
                f,
                "jj is not installed: blogctl uses `jj` to commit every write — install jj and re-run"
            ),
            Self::NotAJjRepo { workdir } => write!(
                f,
                "not a jj repo: {} — run `jj git init --colocate` in the workdir to enable commit tracking",
                workdir.display()
            ),
            Self::ConfigMissing => write!(f, "config missing: .blog-os.toml does not exist"),
            Self::ConfigUnparsable { error } => write!(f, "config unparsable: {error}"),
            Self::StageDirMissing { stage } => {
                write!(f, "stage directory missing: {}/", stage.dirname())
            },
            Self::FrontmatterParseError { path, error } => {
                write!(f, "frontmatter unreadable in {}: {error}", path.display())
            },
            Self::StageMismatch {
                path,
                dir_stage,
                fm_stage,
            } => write!(
                f,
                "stage mismatch in {}: directory says {dir_stage}, frontmatter says {fm_stage}",
                path.display()
            ),
            Self::FilenameSlugMismatch { path, slug } => write!(
                f,
                "filename does not match slug in {}: frontmatter slug is {slug:?}",
                path.display()
            ),
            Self::DuplicateSlug { slug, paths } => {
                let joined = paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "duplicate slug {slug:?}: {joined}")
            },
            Self::UnknownTheme { path, theme, known } => write!(
                f,
                "unknown theme {theme:?} in {}: known themes are {}",
                path.display(),
                known.join(", ")
            ),
            Self::DefaultThemeMissing { theme } => write!(
                f,
                "default theme {theme:?} has no [themes.{theme}] entry in .blog-os.toml: write commands will reject every theme value until it's seeded",
            ),
            Self::TimestampInversion {
                path,
                created_at,
                updated_at,
            } => write!(
                f,
                "timestamp inversion in {}: updated_at {updated_at} predates created_at {created_at}",
                path.display()
            ),
            Self::StrayEntry { dir_stage, path } => write!(
                f,
                "stray entry in {}/: {}",
                dir_stage.dirname(),
                path.display()
            ),
            Self::UnpushedCommitsStale {
                count,
                oldest_age_hours,
                bookmark,
                remote,
            } => write!(
                f,
                "{count} unpushed commit(s) on {bookmark}; oldest is {oldest_age_hours}h old (auto-push to {remote} may be failing silently)",
            ),
            Self::InvalidClassification {
                path,
                dimension,
                value,
                allowed,
            } => write!(
                f,
                "invalid classification in {}: {dimension}={value:?} is not in the taxonomy (allowed: {})",
                path.display(),
                allowed.join(", "),
            ),
            Self::TagsExceedMax { path, count, max } => write!(
                f,
                "too many tags in {}: {count} tags exceeds max {max}",
                path.display(),
            ),
        }
    }
}

/// Walk the workdir and collect every health finding. Never short-
/// circuits — the caller decides whether to print, repair, or both.
///
/// Returns `Err` only for unrecoverable IO (e.g. a stage directory
/// that exists but can't be read). Every recoverable problem comes
/// back as a `Finding`.
pub fn audit(workdir: &Workdir) -> Result<Vec<Finding>> {
    let repo = Repository::unchecked(workdir.clone());
    let mut findings = Vec::new();

    let cfg = match repo.audit_config() {
        ConfigStatus::Ok(cfg) => Some(cfg),
        ConfigStatus::Missing => {
            findings.push(Finding::ConfigMissing);
            None
        }
        ConfigStatus::Unparsable(error) => {
            findings.push(Finding::ConfigUnparsable { error });
            None
        }
    };

    for stage in repo.missing_stage_dirs() {
        findings.push(Finding::StageDirMissing { stage });
    }

    if let Some(cfg) = &cfg {
        if !cfg.themes.contains_key(&cfg.defaults.theme) {
            findings.push(Finding::DefaultThemeMissing {
                theme: cfg.defaults.theme.clone(),
            });
        }
    }

    // Per-post checks. `slug_index` collects parsed slugs so we can
    // surface duplicates after the loop.
    let mut slug_index: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for post in repo.audit_posts()? {
        match post {
            AuditedPost::ParseFailed { path, error, .. } => {
                findings.push(Finding::FrontmatterParseError { path, error });
            }
            AuditedPost::Parsed {
                dir_stage,
                path,
                post,
            } => {
                let meta = post.metadata;
                if meta.status != dir_stage {
                    findings.push(Finding::StageMismatch {
                        path: path.clone(),
                        dir_stage,
                        fm_stage: meta.status,
                    });
                }
                if filename_stem(&path) != Some(meta.slug.as_str()) {
                    findings.push(Finding::FilenameSlugMismatch {
                        path: path.clone(),
                        slug: meta.slug.clone(),
                    });
                }
                if meta.updated_at < meta.created_at {
                    findings.push(Finding::TimestampInversion {
                        path: path.clone(),
                        created_at: meta.created_at,
                        updated_at: meta.updated_at,
                    });
                }
                if let Some(cfg) = &cfg {
                    if !theme_known(cfg, &meta.theme) {
                        findings.push(Finding::UnknownTheme {
                            path: path.clone(),
                            theme: meta.theme.clone(),
                            known: cfg.themes.keys().cloned().collect(),
                        });
                    }
                    // Run taxonomy validation per dimension and emit
                    // one finding per violation — doctor enumerates,
                    // unlike Repository::load which fail-fasts.
                    let taxonomy = crate::taxonomy::Taxonomy::new(cfg.classifications.clone());
                    for v in meta.classifications.violations(&taxonomy) {
                        findings.push(Finding::InvalidClassification {
                            path: path.clone(),
                            dimension: v.dimension,
                            value: v.value,
                            allowed: v.allowed,
                        });
                    }
                    // Tag cap. Workdirs that haven't declared
                    // `[tags] max = N` skip silently — the field
                    // stays `None` and there's nothing to enforce.
                    if let Some(max) = cfg.tags.max {
                        if meta.tags.len() > max as usize {
                            findings.push(Finding::TagsExceedMax {
                                path: path.clone(),
                                count: meta.tags.len(),
                                max,
                            });
                        }
                    }
                }
                slug_index.entry(meta.slug).or_default().push(path);
            }
        }
    }
    for (slug, paths) in slug_index {
        if paths.len() > 1 {
            findings.push(Finding::DuplicateSlug { slug, paths });
        }
    }

    for StrayEntry { dir_stage, path } in repo.stray_entries()? {
        findings.push(Finding::StrayEntry { dir_stage, path });
    }

    Ok(findings)
}

/// Sync-state audit. Separate from `audit()` so the filesystem pass
/// stays pure. Precondition: `jj.status()` returns `Status::Ok` —
/// `full_audit` guarantees this gate before calling, so we don't
/// re-query.
pub fn sync_audit(workdir: &Workdir, jj: &dyn Jj, now: OffsetDateTime) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    let cfg = match Repository::unchecked(workdir.clone()).audit_config() {
        ConfigStatus::Ok(cfg) => cfg,
        // The config layer's own findings (Missing / Unparsable) are
        // surfaced by `audit()` already; we can't read sync settings
        // without a config, so nothing more to do here.
        _ => return Ok(findings),
    };
    if let Some(summary) =
        jj.unpushed_summary(workdir.root(), &cfg.sync.bookmark, &cfg.sync.remote, now)?
    {
        if summary.oldest_age_hours >= STALE_UNPUSHED_THRESHOLD_HOURS {
            findings.push(Finding::UnpushedCommitsStale {
                count: summary.count,
                oldest_age_hours: summary.oldest_age_hours,
                bookmark: cfg.sync.bookmark,
                remote: cfg.sync.remote,
            });
        }
    }
    Ok(findings)
}

/// Full audit: jj precondition + filesystem audit + sync audit.
/// Short-circuits to a single `JjNotInstalled` or `NotAJjRepo`
/// finding when `.jj` is unavailable — those are hard preconditions
/// for blogctl, so further per-post checks would only add noise.
pub fn full_audit(workdir: &Workdir, jj: &dyn Jj, now: OffsetDateTime) -> Result<Vec<Finding>> {
    match jj.status(workdir.root())? {
        Status::Ok => {
            let mut findings = audit(workdir)?;
            findings.extend(sync_audit(workdir, jj, now)?);
            Ok(findings)
        }
        Status::JjNotInstalled => Ok(vec![Finding::JjNotInstalled]),
        Status::NotAJjRepo => Ok(vec![Finding::NotAJjRepo {
            workdir: workdir.root().to_path_buf(),
        }]),
    }
}

pub fn run(jj: &dyn Jj, workdir: PathBuf) -> Result<()> {
    let wd = Workdir::new(&workdir);
    let findings = full_audit(&wd, jj, OffsetDateTime::now_utc())?;
    if findings.is_empty() {
        println!("workdir healthy: {}", workdir.display());
        return Ok(());
    }
    for finding in &findings {
        println!("{finding}");
    }
    Err(Error::WorkdirUnhealthy {
        findings: findings.len(),
    })
}

fn theme_known(cfg: &Config, theme: &str) -> bool {
    cfg.themes.contains_key(theme)
}

fn filename_stem(path: &Path) -> Option<&str> {
    path.file_stem().and_then(|s| s.to_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::TempDir;
    use time::macros::datetime;

    use crate::kind::Kind;
    use crate::post::{Post, PostMetadata};
    use crate::storage::DEFAULT_THEME;

    fn fixture_post(slug: &str, stage: Stage) -> Post {
        Post::new(
            PostMetadata {
                title: format!("Title {slug}"),
                slug: slug.to_string(),
                kind: Kind::Post,
                theme: DEFAULT_THEME.to_string(),
                status: stage,
                created_at: datetime!(2026-05-03 00:00:00 UTC),
                updated_at: datetime!(2026-05-03 00:00:00 UTC),
                tags: vec![],
                todoist_task_id: None,
                history_checked: false,
                targets: vec![],
                classifications: Default::default(),
                ai: None,
            },
            "body\n",
        )
    }

    fn fresh_workdir() -> (TempDir, Workdir) {
        let tmp = TempDir::new().unwrap();
        let workdir = Workdir::new(tmp.path());
        Repository::unchecked(workdir.clone()).init().unwrap();
        (tmp, workdir)
    }

    #[test]
    fn audit_returns_empty_for_freshly_initialized_workdir() {
        let (_tmp, workdir) = fresh_workdir();
        let findings = audit(&workdir).unwrap();
        assert!(findings.is_empty(), "got findings: {findings:?}");
    }

    #[test]
    fn audit_flags_missing_config() {
        let tmp = TempDir::new().unwrap();
        for &stage in Stage::ALL {
            fs::create_dir_all(tmp.path().join(stage.dirname())).unwrap();
        }
        let findings = audit(&Workdir::new(tmp.path())).unwrap();
        assert!(findings.iter().any(|f| matches!(f, Finding::ConfigMissing)));
    }

    #[test]
    fn audit_flags_unparsable_config() {
        let tmp = TempDir::new().unwrap();
        for &stage in Stage::ALL {
            fs::create_dir_all(tmp.path().join(stage.dirname())).unwrap();
        }
        fs::write(tmp.path().join(".blog-os.toml"), "this = is not = valid").unwrap();
        let findings = audit(&Workdir::new(tmp.path())).unwrap();
        assert!(findings
            .iter()
            .any(|f| matches!(f, Finding::ConfigUnparsable { .. })));
    }

    #[test]
    fn audit_flags_each_missing_stage_directory() {
        let tmp = TempDir::new().unwrap();
        // Only create concepts/ and the config; everything else is
        // missing.
        fs::create_dir_all(tmp.path().join("concepts")).unwrap();
        fs::write(tmp.path().join(".blog-os.toml"), "version = 1\n").unwrap();
        let findings = audit(&Workdir::new(tmp.path())).unwrap();
        let missing: Vec<Stage> = findings
            .iter()
            .filter_map(|f| match f {
                Finding::StageDirMissing { stage } => Some(*stage),
                _ => None,
            })
            .collect();
        assert_eq!(missing.len(), 5);
        assert!(!missing.contains(&Stage::Concept));
    }

    #[test]
    fn audit_flags_stage_mismatch() {
        let (tmp, workdir) = fresh_workdir();
        let mismatched = fixture_post("oops", Stage::Editing);
        fs::write(
            tmp.path().join("concepts/oops.md"),
            mismatched.render().unwrap(),
        )
        .unwrap();
        let findings = audit(&workdir).unwrap();
        assert!(findings.iter().any(|f| matches!(
            f,
            Finding::StageMismatch {
                dir_stage: Stage::Concept,
                fm_stage: Stage::Editing,
                ..
            }
        )));
    }

    #[test]
    fn audit_flags_filename_slug_mismatch() {
        let (tmp, workdir) = fresh_workdir();
        let post = fixture_post("real-slug", Stage::Concept);
        // Write the post under a wrong filename — slug stays "real-slug"
        // but the file is named otherwise.md.
        fs::write(
            tmp.path().join("concepts/otherwise.md"),
            post.render().unwrap(),
        )
        .unwrap();
        let findings = audit(&workdir).unwrap();
        assert!(findings
            .iter()
            .any(|f| matches!(f, Finding::FilenameSlugMismatch { .. })));
    }

    #[test]
    fn audit_flags_frontmatter_parse_errors() {
        let (tmp, workdir) = fresh_workdir();
        // Missing closing delimiter.
        fs::write(tmp.path().join("concepts/broken.md"), "---\ntitle: X\n").unwrap();
        let findings = audit(&workdir).unwrap();
        assert!(findings
            .iter()
            .any(|f| matches!(f, Finding::FrontmatterParseError { .. })));
    }

    #[test]
    fn audit_flags_duplicate_slug_across_stages() {
        let (tmp, workdir) = fresh_workdir();
        let p1 = fixture_post("dup", Stage::Concept);
        fs::write(tmp.path().join("concepts/dup.md"), p1.render().unwrap()).unwrap();
        let p2 = fixture_post("dup", Stage::Editing);
        // Frontmatter says editing, file lives in editing — both valid in
        // isolation, but they share a slug.
        fs::write(tmp.path().join("editing/dup.md"), p2.render().unwrap()).unwrap();

        let findings = audit(&workdir).unwrap();
        let dups: Vec<_> = findings
            .iter()
            .filter(|f| matches!(f, Finding::DuplicateSlug { .. }))
            .collect();
        assert_eq!(dups.len(), 1);
    }

    #[test]
    fn audit_flags_unknown_theme() {
        let (tmp, workdir) = fresh_workdir();
        let mut post = fixture_post("themed", Stage::Concept);
        post.metadata.theme = "wat".to_string();
        fs::write(
            tmp.path().join("concepts/themed.md"),
            post.render().unwrap(),
        )
        .unwrap();
        let findings = audit(&workdir).unwrap();
        assert!(findings.iter().any(|f| matches!(
            f,
            Finding::UnknownTheme { theme, .. } if theme == "wat"
        )));
    }

    #[test]
    fn audit_skips_theme_check_when_config_unparsable() {
        // Without a parseable config we have no list of valid themes,
        // so the theme check would be noise. The config error itself
        // is the actionable finding.
        let tmp = TempDir::new().unwrap();
        for &stage in Stage::ALL {
            fs::create_dir_all(tmp.path().join(stage.dirname())).unwrap();
        }
        fs::write(tmp.path().join(".blog-os.toml"), "garbage = = =").unwrap();
        let mut post = fixture_post("p", Stage::Concept);
        post.metadata.theme = "anything".into();
        fs::write(tmp.path().join("concepts/p.md"), post.render().unwrap()).unwrap();

        let findings = audit(&Workdir::new(tmp.path())).unwrap();
        assert!(!findings
            .iter()
            .any(|f| matches!(f, Finding::UnknownTheme { .. })));
        assert!(findings
            .iter()
            .any(|f| matches!(f, Finding::ConfigUnparsable { .. })));
    }

    #[test]
    fn audit_flags_default_theme_missing_when_themes_table_absent() {
        // Pre-themes workdir: only `version = 1`. Config parses fine
        // (back-filled defaults), but `defaults.theme` ("standard")
        // has no `[themes.standard]` entry, so every write would be
        // rejected.
        let tmp = TempDir::new().unwrap();
        for &stage in Stage::ALL {
            fs::create_dir_all(tmp.path().join(stage.dirname())).unwrap();
        }
        fs::write(tmp.path().join(".blog-os.toml"), "version = 1\n").unwrap();
        let findings = audit(&Workdir::new(tmp.path())).unwrap();
        assert!(
            findings.iter().any(|f| matches!(
                f,
                Finding::DefaultThemeMissing { theme } if theme == DEFAULT_THEME
            )),
            "got: {findings:?}"
        );
    }

    #[test]
    fn audit_flags_default_theme_missing_uses_actual_default_name() {
        // Non-standard `defaults.theme` with no matching `[themes.*]`
        // entry — the finding must name "parable", not "standard".
        let tmp = TempDir::new().unwrap();
        for &stage in Stage::ALL {
            fs::create_dir_all(tmp.path().join(stage.dirname())).unwrap();
        }
        fs::write(
            tmp.path().join(".blog-os.toml"),
            "version = 1\n[defaults]\ntheme = \"parable\"\n",
        )
        .unwrap();
        let findings = audit(&Workdir::new(tmp.path())).unwrap();
        assert!(
            findings.iter().any(|f| matches!(
                f,
                Finding::DefaultThemeMissing { theme } if theme == "parable"
            )),
            "got: {findings:?}"
        );
    }

    #[test]
    fn audit_silent_on_default_theme_when_table_present() {
        // Freshly initialized workdir seeds `[themes.standard]` —
        // the finding must not fire.
        let (_tmp, workdir) = fresh_workdir();
        let findings = audit(&workdir).unwrap();
        assert!(
            !findings
                .iter()
                .any(|f| matches!(f, Finding::DefaultThemeMissing { .. })),
            "got: {findings:?}"
        );
    }

    #[test]
    fn audit_flags_timestamp_inversion() {
        let (tmp, workdir) = fresh_workdir();
        let mut post = fixture_post("inverted", Stage::Concept);
        post.metadata.created_at = datetime!(2026-05-05 00:00:00 UTC);
        post.metadata.updated_at = datetime!(2026-05-03 00:00:00 UTC);
        fs::write(
            tmp.path().join("concepts/inverted.md"),
            post.render().unwrap(),
        )
        .unwrap();
        let findings = audit(&workdir).unwrap();
        assert!(findings
            .iter()
            .any(|f| matches!(f, Finding::TimestampInversion { .. })));
    }

    #[test]
    fn audit_flags_stray_entries() {
        let (tmp, workdir) = fresh_workdir();
        fs::write(tmp.path().join("concepts/.DS_Store"), "noise").unwrap();
        let findings = audit(&workdir).unwrap();
        assert!(findings
            .iter()
            .any(|f| matches!(f, Finding::StrayEntry { .. })));
    }

    #[test]
    fn audit_flags_invalid_classifications_per_dimension() {
        let (tmp, workdir) = fresh_workdir();
        let mut post = fixture_post("multi-bad", Stage::Concept);
        // Two bad single-valued dims + one bad motifs element.
        post.metadata.classifications.format = Some("not-a-format".into());
        post.metadata.classifications.tone = Some("brisk".into());
        post.metadata
            .classifications
            .motifs
            .push("ambiguity".into()); // good
        post.metadata
            .classifications
            .motifs
            .push("made-up-motif".into()); // bad
        fs::write(
            tmp.path().join("concepts/multi-bad.md"),
            post.render().unwrap(),
        )
        .unwrap();

        let findings = audit(&workdir).unwrap();
        let invalid: Vec<&Finding> = findings
            .iter()
            .filter(|f| matches!(f, Finding::InvalidClassification { .. }))
            .collect();
        assert_eq!(
            invalid.len(),
            3,
            "expected one finding per bad value: {findings:?}"
        );
    }

    #[test]
    fn audit_flags_tags_exceeding_max() {
        // Declare `[tags] max = 2` and write a post with 3 tags —
        // doctor surfaces one TagsExceedMax finding for that post.
        let (tmp, workdir) = fresh_workdir();
        fs::write(
            tmp.path().join(".blog-os.toml"),
            "version = 1\n[themes.standard]\n[tags]\nmax = 2\n",
        )
        .unwrap();

        let mut post = fixture_post("over-tagged", Stage::Concept);
        post.metadata.tags = vec!["a".into(), "b".into(), "c".into()];
        fs::write(
            tmp.path().join("concepts/over-tagged.md"),
            post.render().unwrap(),
        )
        .unwrap();

        let findings = audit(&workdir).unwrap();
        let tags_findings: Vec<&Finding> = findings
            .iter()
            .filter(|f| matches!(f, Finding::TagsExceedMax { .. }))
            .collect();
        assert_eq!(
            tags_findings.len(),
            1,
            "expected one TagsExceedMax finding: {findings:?}",
        );
        if let Finding::TagsExceedMax { count, max, .. } = tags_findings[0] {
            assert_eq!(*count, 3);
            assert_eq!(*max, 2);
        }
    }

    #[test]
    fn audit_silent_on_tags_when_no_max_declared() {
        // No `[tags]` block in config → doctor is permissive even on
        // posts with arbitrarily many tags.
        let (tmp, workdir) = fresh_workdir();
        fs::write(
            tmp.path().join(".blog-os.toml"),
            "version = 1\n[themes.standard]\n",
        )
        .unwrap();

        let mut post = fixture_post("loose-tagged", Stage::Concept);
        post.metadata.tags = (0..50).map(|i| format!("tag-{i}")).collect();
        fs::write(
            tmp.path().join("concepts/loose-tagged.md"),
            post.render().unwrap(),
        )
        .unwrap();

        let findings = audit(&workdir).unwrap();
        assert!(
            !findings
                .iter()
                .any(|f| matches!(f, Finding::TagsExceedMax { .. })),
            "expected no TagsExceedMax findings without `[tags] max`: {findings:?}",
        );
    }

    #[test]
    fn audit_silent_on_classifications_when_taxonomy_is_empty() {
        // Wipe the taxonomy in the config; doctor should accept any
        // classification value (permissive on undeclared dimensions).
        let (tmp, workdir) = fresh_workdir();
        // Erase `[classifications.*]` by writing a minimal config.
        fs::write(
            tmp.path().join(".blog-os.toml"),
            "version = 1\n[themes.standard]\n",
        )
        .unwrap();

        let mut post = fixture_post("anything", Stage::Concept);
        post.metadata.classifications.format = Some("whatever-i-want".into());
        fs::write(
            tmp.path().join("concepts/anything.md"),
            post.render().unwrap(),
        )
        .unwrap();

        let findings = audit(&workdir).unwrap();
        assert!(!findings
            .iter()
            .any(|f| matches!(f, Finding::InvalidClassification { .. })));
    }

    #[test]
    fn invalid_classification_finding_lists_allowed_values() {
        let f = Finding::InvalidClassification {
            path: PathBuf::from("concepts/typo.md"),
            dimension: "format".into(),
            value: "thesys".into(),
            allowed: vec!["thesis".into(), "essay".into()],
        };
        let rendered = format!("{f}");
        assert!(rendered.contains("thesys"));
        assert!(rendered.contains("thesis"));
        assert!(rendered.contains("essay"));
        assert!(rendered.contains("format"));
    }

    #[test]
    fn run_returns_workdir_unhealthy_on_findings() {
        let (tmp, _workdir) = fresh_workdir();
        fs::write(tmp.path().join("concepts/.DS_Store"), "x").unwrap();
        // `Status::Ok` keeps the filesystem audit path live so the
        // stray-entry finding actually surfaces.
        let jj = crate::sync::FakeJj::new().with_status(crate::sync::Status::Ok);
        let err = run(&jj, tmp.path().to_path_buf()).unwrap_err();
        assert!(matches!(err, Error::WorkdirUnhealthy { findings: n } if n >= 1));
    }

    #[test]
    fn run_returns_ok_on_clean_workdir() {
        let (tmp, _workdir) = fresh_workdir();
        let jj = crate::sync::FakeJj::new().with_status(crate::sync::Status::Ok);
        run(&jj, tmp.path().to_path_buf()).unwrap();
    }

    #[test]
    fn full_audit_short_circuits_on_jj_not_installed() {
        let (_tmp, workdir) = fresh_workdir();
        let jj = crate::sync::FakeJj::new().with_status(crate::sync::Status::JjNotInstalled);
        let findings = full_audit(&workdir, &jj, OffsetDateTime::now_utc()).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0], Finding::JjNotInstalled));
    }

    #[test]
    fn full_audit_short_circuits_on_not_a_jj_repo() {
        let (_tmp, workdir) = fresh_workdir();
        // Plant a filesystem-level finding that would normally surface
        // through `audit()` — short-circuit must suppress it.
        fs::write(_tmp.path().join("concepts/.DS_Store"), "x").unwrap();
        let jj = crate::sync::FakeJj::new().with_status(crate::sync::Status::NotAJjRepo);
        let findings = full_audit(&workdir, &jj, OffsetDateTime::now_utc()).unwrap();
        assert_eq!(findings.len(), 1, "got: {findings:?}");
        assert!(matches!(findings[0], Finding::NotAJjRepo { .. }));
    }

    #[test]
    fn run_reports_not_a_jj_repo() {
        let (tmp, _workdir) = fresh_workdir();
        let jj = crate::sync::FakeJj::new().with_status(crate::sync::Status::NotAJjRepo);
        let err = run(&jj, tmp.path().to_path_buf()).unwrap_err();
        assert!(matches!(err, Error::WorkdirUnhealthy { findings: 1 }));
    }

    #[test]
    fn sync_audit_returns_empty_when_no_unpushed_commits() {
        let (_tmp, workdir) = fresh_workdir();
        let jj = crate::sync::FakeJj::new().with_unpushed_summary(None);
        let findings = sync_audit(&workdir, &jj, OffsetDateTime::now_utc()).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn sync_audit_returns_empty_when_unpushed_below_threshold() {
        let (_tmp, workdir) = fresh_workdir();
        let jj =
            crate::sync::FakeJj::new().with_unpushed_summary(Some(crate::sync::UnpushedSummary {
                count: 2,
                oldest_age_hours: STALE_UNPUSHED_THRESHOLD_HOURS - 1,
            }));
        let findings = sync_audit(&workdir, &jj, OffsetDateTime::now_utc()).unwrap();
        assert!(
            findings.is_empty(),
            "below-threshold age must not produce a finding"
        );
    }

    #[test]
    fn sync_audit_surfaces_unpushed_stale_at_threshold() {
        let (_tmp, workdir) = fresh_workdir();
        let jj =
            crate::sync::FakeJj::new().with_unpushed_summary(Some(crate::sync::UnpushedSummary {
                count: 3,
                oldest_age_hours: STALE_UNPUSHED_THRESHOLD_HOURS,
            }));
        let findings = sync_audit(&workdir, &jj, OffsetDateTime::now_utc()).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(matches!(
            findings[0],
            Finding::UnpushedCommitsStale {
                count: 3,
                oldest_age_hours: h,
                ..
            } if h == STALE_UNPUSHED_THRESHOLD_HOURS
        ));
    }

    #[test]
    fn unpushed_stale_finding_displays_useful_message() {
        let f = Finding::UnpushedCommitsStale {
            count: 4,
            oldest_age_hours: 72,
            bookmark: "main".into(),
            remote: "origin".into(),
        };
        let rendered = format!("{f}");
        assert!(rendered.contains("4 unpushed commit"));
        assert!(rendered.contains("72h"));
        assert!(rendered.contains("main"));
        assert!(rendered.contains("origin"));
    }
}
