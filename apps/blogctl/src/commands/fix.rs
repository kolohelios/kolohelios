//! Apply auto-correctable repairs surfaced by `doctor`. The
//! classifier in `plan` decides per-finding whether to apply or skip
//! (with a reason); `apply` executes the applies in an order that
//! avoids in-flight conflicts (e.g. stage rewrite before filename
//! rename, so the rename works against the post's final shape).
//!
//! Exits non-zero whenever any finding remains after the run — `fix`
//! followed by `doctor` is therefore a useful idempotency check.

use std::fmt;
use std::fs;
use std::path::PathBuf;

use time::OffsetDateTime;

use crate::commands::doctor::{self, Finding};
use crate::error::{Error, Result};
use crate::post::Post;
use crate::stage::Stage;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};

/// Per-finding plan entry — either we'll attempt a repair or we'll
/// skip with a reason. The order in `plan()` doubles as the apply
/// order: directory creation precedes content rewrites, content
/// rewrites precede file renames.
#[derive(Debug)]
pub enum Repair {
    CreateStageDir(Stage),
    RewriteStage {
        path: PathBuf,
        new_status: Stage,
    },
    BumpUpdatedAt {
        path: PathBuf,
    },
    RenameToSlug {
        path: PathBuf,
        slug: String,
    },
    Skip {
        finding: Finding,
        reason: SkipReason,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum SkipReason {
    DuplicateSlug,
    FrontmatterUnreadable,
    UnknownTheme,
    StrayEntry,
    ConfigMissing,
    ConfigUnparsable,
    UnpushedCommits,
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::DuplicateSlug => "manual: choose which file keeps the slug",
            Self::FrontmatterUnreadable => "manual: frontmatter must be repaired by hand",
            Self::UnknownTheme => "manual: typo or missing [themes.*] entry",
            Self::StrayEntry => "manual: review the file and remove or relocate it",
            Self::ConfigMissing => "manual: run `blogctl init` or restore .blog-os.toml",
            Self::ConfigUnparsable => "manual: edit .blog-os.toml by hand",
            Self::UnpushedCommits => {
                "manual: investigate auto-push (network? auth? wrong bookmark name?)"
            }
        };
        f.write_str(s)
    }
}

/// Classify every finding. Auto-correctable findings become specific
/// `Repair` variants in apply order; the rest become `Skip` with a
/// reason. Order matters at apply time: directory creation precedes
/// content rewrites, content rewrites precede renames.
pub fn plan(findings: Vec<Finding>) -> Vec<Repair> {
    let mut creates = Vec::new();
    let mut rewrites = Vec::new();
    let mut bumps = Vec::new();
    let mut renames = Vec::new();
    let mut skips = Vec::new();

    for finding in findings {
        match finding {
            Finding::StageDirMissing { stage } => creates.push(Repair::CreateStageDir(stage)),
            Finding::StageMismatch {
                path, dir_stage, ..
            } => rewrites.push(Repair::RewriteStage {
                path,
                new_status: dir_stage,
            }),
            Finding::TimestampInversion { path, .. } => bumps.push(Repair::BumpUpdatedAt { path }),
            Finding::FilenameSlugMismatch { path, slug } => {
                renames.push(Repair::RenameToSlug { path, slug })
            }
            f @ Finding::DuplicateSlug { .. } => skips.push(Repair::Skip {
                finding: f,
                reason: SkipReason::DuplicateSlug,
            }),
            f @ Finding::FrontmatterParseError { .. } => skips.push(Repair::Skip {
                finding: f,
                reason: SkipReason::FrontmatterUnreadable,
            }),
            f @ Finding::UnknownTheme { .. } => skips.push(Repair::Skip {
                finding: f,
                reason: SkipReason::UnknownTheme,
            }),
            f @ Finding::StrayEntry { .. } => skips.push(Repair::Skip {
                finding: f,
                reason: SkipReason::StrayEntry,
            }),
            f @ Finding::ConfigMissing => skips.push(Repair::Skip {
                finding: f,
                reason: SkipReason::ConfigMissing,
            }),
            f @ Finding::ConfigUnparsable { .. } => skips.push(Repair::Skip {
                finding: f,
                reason: SkipReason::ConfigUnparsable,
            }),
            f @ Finding::UnpushedCommitsStale { .. } => skips.push(Repair::Skip {
                finding: f,
                reason: SkipReason::UnpushedCommits,
            }),
        }
    }

    let mut out = Vec::with_capacity(
        creates.len() + rewrites.len() + bumps.len() + renames.len() + skips.len(),
    );
    out.extend(creates);
    out.extend(rewrites);
    out.extend(bumps);
    out.extend(renames);
    out.extend(skips);
    out
}

/// Outcome line for a single planned `Repair`. `Failed` carries the
/// finding-equivalent diagnosis so the post-run summary can re-count
/// it as a remaining problem.
#[derive(Debug)]
pub enum Outcome {
    Fixed(String),
    Skipped(String, SkipReason),
    Failed(String, Error),
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(diag) => write!(f, "fixed: {diag}"),
            Self::Skipped(diag, reason) => write!(f, "skipped: {diag} ({reason})"),
            Self::Failed(diag, err) => write!(f, "failed: {diag} ({err})"),
        }
    }
}

/// Apply (or simulate) every planned repair against the workdir.
/// `dry_run = true` performs no IO but produces the same diagnostic
/// stream so users can preview the fix.
pub fn apply(
    workdir: &Workdir,
    plan: Vec<Repair>,
    now: OffsetDateTime,
    dry_run: bool,
) -> Vec<Outcome> {
    plan.into_iter()
        .map(|r| apply_one(workdir, r, now, dry_run))
        .collect()
}

fn apply_one(workdir: &Workdir, repair: Repair, now: OffsetDateTime, dry_run: bool) -> Outcome {
    match repair {
        Repair::CreateStageDir(stage) => {
            let dir = workdir.stage_dir(stage);
            let diag = format!("create stage directory {}/", stage.dirname());
            if dry_run {
                return Outcome::Fixed(diag);
            }
            match fs::create_dir_all(&dir) {
                Ok(()) => Outcome::Fixed(diag),
                Err(err) => Outcome::Failed(diag, Error::io(&dir, err)),
            }
        }
        Repair::RewriteStage { path, new_status } => {
            let diag = format!(
                "rewrite frontmatter status to {} in {}",
                new_status,
                path.display()
            );
            if dry_run {
                return Outcome::Fixed(diag);
            }
            match rewrite_metadata(&path, |post| {
                post.metadata.status = new_status;
                post.metadata.updated_at = now;
            }) {
                Ok(()) => Outcome::Fixed(diag),
                Err(e) => Outcome::Failed(diag, e),
            }
        }
        Repair::BumpUpdatedAt { path } => {
            let diag = format!("bump updated_at in {}", path.display());
            if dry_run {
                return Outcome::Fixed(diag);
            }
            match rewrite_metadata(&path, |post| {
                post.metadata.updated_at = now;
            }) {
                Ok(()) => Outcome::Fixed(diag),
                Err(e) => Outcome::Failed(diag, e),
            }
        }
        Repair::RenameToSlug { path, slug } => {
            let parent = match path.parent() {
                Some(p) => p,
                None => {
                    return Outcome::Failed(
                        format!("rename {} to {slug}.md", path.display()),
                        Error::io(&path, std::io::Error::other("path has no parent")),
                    )
                }
            };
            let new_path = parent.join(format!("{slug}.md"));
            let diag = format!("rename {} to {}", path.display(), new_path.display());
            if new_path.exists() {
                return Outcome::Failed(
                    diag,
                    Error::io(
                        &new_path,
                        std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "target file already exists",
                        ),
                    ),
                );
            }
            if dry_run {
                return Outcome::Fixed(diag);
            }
            match fs::rename(&path, &new_path) {
                Ok(()) => Outcome::Fixed(diag),
                Err(err) => Outcome::Failed(diag, Error::io(&new_path, err)),
            }
        }
        Repair::Skip { finding, reason } => Outcome::Skipped(finding.to_string(), reason),
    }
}

fn rewrite_metadata<F>(path: &PathBuf, mutate: F) -> Result<()>
where
    F: FnOnce(&mut Post),
{
    let raw = fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    let mut post = Post::parse(path, &raw)?;
    mutate(&mut post);
    let rendered = post.render()?;
    fs::write(path, rendered).map_err(|e| Error::io(path, e))?;
    Ok(())
}

pub fn run(jj: &dyn Jj, workdir: PathBuf, dry_run: bool, no_sync: bool) -> Result<()> {
    let wd = Workdir::new(&workdir);
    let findings = doctor::audit(&wd)?;
    if findings.is_empty() {
        println!("workdir healthy: {}", workdir.display());
        return Ok(());
    }

    let plan = plan(findings);
    // Count repairs that will actually touch the workdir — skips
    // never write anything, so a plan of all-Skip needs no sync wrap.
    let attempted = plan
        .iter()
        .filter(|r| !matches!(r, Repair::Skip { .. }))
        .count();

    if dry_run || attempted == 0 {
        // Dry-run or skip-only plan: no file changes happen, so no
        // sync. Print outcomes and return.
        let outcomes = apply(&wd, plan, OffsetDateTime::now_utc(), dry_run);
        for outcome in &outcomes {
            println!("{outcome}");
        }
        let remaining = outcomes
            .iter()
            .filter(|o| !matches!(o, Outcome::Fixed(_)))
            .count();
        return if remaining == 0 {
            Ok(())
        } else {
            Err(Error::WorkdirUnhealthy(remaining))
        };
    }

    let repo = Repository::open(wd.clone())?;
    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, no_sync);
    let message = format!("chore: fix workdir ({attempted} findings)");

    let outcomes = sync::commit_and_push(jj, &workdir, &opts, &message, || {
        Ok(apply(&wd, plan, OffsetDateTime::now_utc(), false))
    })?;
    for outcome in &outcomes {
        println!("{outcome}");
    }
    let remaining = outcomes
        .iter()
        .filter(|o| !matches!(o, Outcome::Fixed(_)))
        .count();
    if remaining == 0 {
        Ok(())
    } else {
        Err(Error::WorkdirUnhealthy(remaining))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::TempDir;
    use time::macros::datetime;

    use crate::kind::Kind;
    use crate::post::PostMetadata;
    use crate::storage::{Repository, DEFAULT_THEME};
    use crate::sync::FakeJj;

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

    fn now() -> OffsetDateTime {
        datetime!(2026-05-09 12:00:00 UTC)
    }

    #[test]
    fn apply_creates_missing_stage_directories() {
        let tmp = TempDir::new().unwrap();
        let workdir = Workdir::new(tmp.path());
        // Skip init so all stage dirs are missing, but plant a config so
        // we don't trip the config-missing skip path.
        fs::write(tmp.path().join(".blog-os.toml"), "version = 1\n").unwrap();

        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), false);

        for &stage in Stage::ALL {
            assert!(tmp.path().join(stage.dirname()).is_dir(), "{}", stage);
        }
        assert!(outcomes.iter().all(|o| matches!(o, Outcome::Fixed(_))));
    }

    #[test]
    fn apply_rewrites_stage_to_match_directory() {
        let (tmp, workdir) = fresh_workdir();
        let mismatched = fixture_post("oops", Stage::Editing);
        fs::write(
            tmp.path().join("concepts/oops.md"),
            mismatched.render().unwrap(),
        )
        .unwrap();

        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), false);
        assert!(outcomes.iter().all(|o| matches!(o, Outcome::Fixed(_))));

        // After fix, doctor should see no findings.
        let after = doctor::audit(&workdir).unwrap();
        assert!(after.is_empty(), "remaining findings: {after:?}");
    }

    #[test]
    fn apply_renames_file_to_match_slug() {
        let (tmp, workdir) = fresh_workdir();
        let post = fixture_post("real-slug", Stage::Concept);
        fs::write(
            tmp.path().join("concepts/wrong-name.md"),
            post.render().unwrap(),
        )
        .unwrap();

        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), false);
        assert!(outcomes.iter().all(|o| matches!(o, Outcome::Fixed(_))));
        assert!(tmp.path().join("concepts/real-slug.md").is_file());
        assert!(!tmp.path().join("concepts/wrong-name.md").exists());
    }

    #[test]
    fn apply_bumps_updated_at_on_inversion() {
        let (tmp, workdir) = fresh_workdir();
        let mut post = fixture_post("inverted", Stage::Concept);
        post.metadata.created_at = datetime!(2026-05-05 00:00:00 UTC);
        post.metadata.updated_at = datetime!(2026-05-03 00:00:00 UTC);
        fs::write(
            tmp.path().join("concepts/inverted.md"),
            post.render().unwrap(),
        )
        .unwrap();

        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), false);
        assert!(outcomes.iter().all(|o| matches!(o, Outcome::Fixed(_))));

        let raw = fs::read_to_string(tmp.path().join("concepts/inverted.md")).unwrap();
        assert!(raw.contains("updated_at: 2026-05-09T12:00:00Z"));
    }

    #[test]
    fn apply_combined_stage_and_filename_repairs_in_correct_order() {
        // Both stage mismatch and filename mismatch on the same file.
        // Stage rewrite must run first, otherwise the rename moves the
        // file out from under the rewrite. The plan() helper enforces
        // this ordering.
        let (tmp, workdir) = fresh_workdir();
        let mut post = fixture_post("right-slug", Stage::Editing);
        post.metadata.status = Stage::Editing; // explicit
        let path = tmp.path().join("concepts/wrong-name.md");
        fs::write(&path, post.render().unwrap()).unwrap();

        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), false);
        assert!(
            outcomes.iter().all(|o| matches!(o, Outcome::Fixed(_))),
            "got: {outcomes:?}"
        );

        let final_path = tmp.path().join("concepts/right-slug.md");
        assert!(final_path.is_file());
        let raw = fs::read_to_string(final_path).unwrap();
        assert!(raw.contains("status: concept"));
        assert!(!tmp.path().join("concepts/wrong-name.md").exists());
    }

    #[test]
    fn apply_skips_duplicate_slug() {
        let (tmp, workdir) = fresh_workdir();
        let p1 = fixture_post("dup", Stage::Concept);
        let p2 = fixture_post("dup", Stage::Editing);
        fs::write(tmp.path().join("concepts/dup.md"), p1.render().unwrap()).unwrap();
        fs::write(tmp.path().join("editing/dup.md"), p2.render().unwrap()).unwrap();

        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), false);
        assert!(outcomes
            .iter()
            .any(|o| matches!(o, Outcome::Skipped(_, SkipReason::DuplicateSlug))));
    }

    #[test]
    fn apply_skips_frontmatter_parse_errors() {
        let (tmp, workdir) = fresh_workdir();
        fs::write(tmp.path().join("concepts/bad.md"), "---\nbroken").unwrap();
        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), false);
        assert!(outcomes
            .iter()
            .any(|o| matches!(o, Outcome::Skipped(_, SkipReason::FrontmatterUnreadable))));
    }

    #[test]
    fn apply_skips_unknown_theme() {
        let (tmp, workdir) = fresh_workdir();
        let mut post = fixture_post("themed", Stage::Concept);
        post.metadata.theme = "nope".into();
        fs::write(
            tmp.path().join("concepts/themed.md"),
            post.render().unwrap(),
        )
        .unwrap();
        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), false);
        assert!(outcomes
            .iter()
            .any(|o| matches!(o, Outcome::Skipped(_, SkipReason::UnknownTheme))));
    }

    #[test]
    fn apply_skips_stray_entries() {
        let (tmp, workdir) = fresh_workdir();
        fs::write(tmp.path().join("concepts/.DS_Store"), "x").unwrap();
        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), false);
        assert!(outcomes
            .iter()
            .any(|o| matches!(o, Outcome::Skipped(_, SkipReason::StrayEntry))));
        // Confirm the stray file was left in place.
        assert!(tmp.path().join("concepts/.DS_Store").is_file());
    }

    #[test]
    fn dry_run_makes_no_changes() {
        let (tmp, workdir) = fresh_workdir();
        let mismatched = fixture_post("oops", Stage::Editing);
        let path = tmp.path().join("concepts/oops.md");
        fs::write(&path, mismatched.render().unwrap()).unwrap();
        let raw_before = fs::read_to_string(&path).unwrap();

        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), true);
        assert!(outcomes.iter().any(|o| matches!(o, Outcome::Fixed(_))));

        let raw_after = fs::read_to_string(&path).unwrap();
        assert_eq!(raw_before, raw_after, "dry-run must not modify files");
    }

    #[test]
    fn rename_refuses_to_clobber_existing_file() {
        // Plant two files: concepts/a.md (slug=a) and concepts/b.md
        // (slug=b). Then rewrite a.md's frontmatter so its slug claims
        // to be "b" — fix should refuse the rename rather than clobber.
        let (tmp, workdir) = fresh_workdir();
        let a = fixture_post("a", Stage::Concept);
        let b = fixture_post("b", Stage::Concept);
        fs::write(tmp.path().join("concepts/a.md"), a.render().unwrap()).unwrap();
        fs::write(tmp.path().join("concepts/b.md"), b.render().unwrap()).unwrap();

        // Write a file at concepts/a.md whose frontmatter says slug=b.
        let mut wrong = fixture_post("b", Stage::Concept);
        wrong.metadata.title = "claims b".into();
        fs::write(tmp.path().join("concepts/a.md"), wrong.render().unwrap()).unwrap();

        let findings = doctor::audit(&workdir).unwrap();
        let outcomes = apply(&workdir, plan(findings), now(), false);
        // The rename for concepts/a.md -> concepts/b.md must fail.
        assert!(
            outcomes.iter().any(|o| matches!(o, Outcome::Failed(_, _))),
            "expected a Failed outcome, got: {outcomes:?}"
        );
        // Both files still exist.
        assert!(tmp.path().join("concepts/a.md").is_file());
        assert!(tmp.path().join("concepts/b.md").is_file());
    }

    #[test]
    fn run_returns_ok_when_every_repair_applies() {
        let (tmp, workdir) = fresh_workdir();
        let mismatched = fixture_post("oops", Stage::Editing);
        fs::write(
            tmp.path().join("concepts/oops.md"),
            mismatched.render().unwrap(),
        )
        .unwrap();

        // Disable sync via --no-sync (workdir isn't a jj repo).
        let jj = FakeJj::new();
        run(&jj, tmp.path().to_path_buf(), false, true).unwrap();
        // doctor should now be clean.
        assert!(doctor::audit(&workdir).unwrap().is_empty());
    }

    #[test]
    fn run_returns_unhealthy_when_findings_remain() {
        let (tmp, _workdir) = fresh_workdir();
        // A skipped-only finding: stray entry.
        fs::write(tmp.path().join("concepts/.DS_Store"), "x").unwrap();
        let jj = FakeJj::new();
        let err = run(&jj, tmp.path().to_path_buf(), false, true).unwrap_err();
        assert!(matches!(err, Error::WorkdirUnhealthy(_)));
    }
}
