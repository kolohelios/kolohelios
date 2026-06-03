use std::collections::BTreeSet;
use std::process::Command;

use clap::Subcommand;
use serde::Serialize;

use crate::jj;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};
use crate::workspace;

pub mod issue_ref;
pub mod title;

use title::TitleError;

const SLOTS: &[&str] = &["apps", "infra", "packages", "services", "tools"];
pub(crate) const TITLE_MAX: usize = 70;
const BODY_LINE_MAX: usize = 80;

#[derive(Subcommand)]
pub enum CommitCommand {
    /// Lint commit descriptions against conventional commit conventions
    Lint {
        /// Revset to lint (jj syntax). Defaults to the working copy commit.
        #[arg(short = 'r', long, default_value = "@")]
        revset: String,
        /// Suppress the "issue-tied branch missing Closes #N" check. Use
        /// for branches that legitimately don't close an issue (chores,
        /// docs sweeps, exploratory branches).
        #[arg(long)]
        allow_no_issue_link: bool,
        /// Emit a machine-readable JSON report instead of prose
        #[arg(long)]
        json: bool,
    },
}

pub fn run(cmd: CommitCommand) {
    match cmd {
        CommitCommand::Lint {
            revset,
            allow_no_issue_link,
            json,
        } => lint(&revset, allow_no_issue_link, json),
    }
}

/// Machine-readable `commit lint` result, emitted under `--json`.
#[derive(Serialize)]
struct LintReport {
    commits: Vec<CommitReport>,
    total_errors: usize,
    total_warnings: usize,
    clean: usize,
    success: bool,
}

#[derive(Serialize)]
struct CommitReport {
    change_id: String,
    title: String,
    findings: Vec<Finding>,
}

fn lint(revset: &str, allow_no_issue_link: bool, json: bool) {
    let commits = match collect_commits(revset) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    if commits.is_empty() {
        let report = LintReport {
            commits: Vec::new(),
            total_errors: 0,
            total_warnings: 0,
            clean: 0,
            success: true,
        };
        crate::output::emit(json, &report, |_| {
            println!("no commits found in revset {revset:?}");
        });
        return;
    }

    let ctx = LintContext {
        allow_no_issue_link,
        tied_issues: gather_tied_issues(revset),
    };

    let mut commit_reports: Vec<CommitReport> = Vec::new();
    let mut total_errors = 0usize;
    let mut total_warnings = 0usize;
    let mut clean = 0usize;

    for commit in &commits {
        let findings = lint_commit(commit, &ctx);
        total_errors += findings
            .iter()
            .filter(|f| matches!(f.severity, Severity::Error))
            .count();
        total_warnings += findings
            .iter()
            .filter(|f| matches!(f.severity, Severity::Warn))
            .count();
        if findings.is_empty() {
            clean += 1;
        }

        let title = commit
            .description
            .lines()
            .next()
            .unwrap_or("(empty)")
            .to_string();
        commit_reports.push(CommitReport {
            change_id: commit.change_id.clone(),
            title,
            findings,
        });
    }

    let report = LintReport {
        commits: commit_reports,
        total_errors,
        total_warnings,
        clean,
        success: total_errors == 0,
    };

    crate::output::emit(json, &report, render_human);

    if !report.success {
        std::process::exit(1);
    }
}

fn render_human(report: &LintReport) {
    for c in &report.commits {
        if c.findings.is_empty() {
            continue;
        }
        let short = &c.change_id[..c.change_id.len().min(8)];
        println!("\n{BOLD}{short} {DIM}{}{RESET}", c.title);
        for f in &c.findings {
            let label = match f.severity {
                Severity::Error => format!("{RED}{BOLD}error{RESET}"),
                Severity::Warn => format!("{YELLOW}{BOLD}warn{RESET} "),
            };
            println!("  {label}  {}", f.message);
        }
    }

    let n = report.commits.len();
    println!();
    if report.total_errors > 0 {
        eprintln!(
            "{RED}{BOLD}commit lint failed{RESET} ({n} commit(s), {} error(s), {} warning(s))",
            report.total_errors, report.total_warnings
        );
        return;
    }
    let warn_str = if report.total_warnings > 0 {
        format!(" ({} warning(s))", report.total_warnings)
    } else {
        String::new()
    };
    println!(
        "{GREEN}{BOLD}commit lint passed{RESET} ({}/{n} clean){warn_str}",
        report.clean
    );
}

#[derive(Debug)]
struct Commit {
    change_id: String,
    description: String,
    files: Vec<String>,
    /// Parent count for the commit. Used to identify merge commits
    /// (≥ 2 parents); a regular commit has exactly one parent (or
    /// zero for the root, which doesn't reach `commit lint`).
    parent_count: usize,
    /// `true` for the tip of the linted revset — the commit closest
    /// to the PR head. The `Closes #N` enforcement only applies to
    /// the tip; intermediate commits often don't close the issue.
    is_tip: bool,
}

/// Context shared across every commit in the linted revset.
struct LintContext {
    allow_no_issue_link: bool,
    /// Issue numbers the branch appears to be tied to, gathered from
    /// bookmark names matching the `iN` convention and from the
    /// workspace's persisted `--issue` link. Empty when no heuristic
    /// matched — in that case the issue-link check is skipped.
    tied_issues: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Severity {
    Error,
    Warn,
}

#[derive(Debug, Serialize)]
struct Finding {
    severity: Severity,
    message: String,
}

fn collect_commits(revset: &str) -> Result<Vec<Commit>, String> {
    let ids_output = Command::new("jj")
        .args([
            "log",
            "-r",
            revset,
            "--no-graph",
            "-T",
            r#"change_id ++ "\n""#,
        ])
        .output()
        .map_err(|e| format!("failed to run jj: {e}"))?;

    if !ids_output.status.success() {
        return Err(String::from_utf8_lossy(&ids_output.stderr)
            .trim()
            .to_string());
    }

    let at_change_id = jj_template("@", "change_id")?.trim().to_string();
    let mut commits = Vec::new();
    for line in String::from_utf8_lossy(&ids_output.stdout).lines() {
        let id = line.trim();
        if id.is_empty() {
            continue;
        }
        let description = jj_template(id, "description")?;
        let files = jj_files(id)?;
        let parent_count = jj_template(id, "parents.len()")?
            .trim()
            .parse::<usize>()
            .map_err(|e| format!("could not parse parent count for {id}: {e}"))?;
        let is_tip = id == at_change_id;
        commits.push(Commit {
            change_id: id.to_string(),
            description,
            files,
            parent_count,
            is_tip,
        });
    }
    // If `@` isn't in the revset (e.g. `-r main..main@origin`), fall back
    // to marking the first reported commit — `jj log` defaults to
    // newest-first, so the head of the requested set is at the top.
    if !commits.iter().any(|c| c.is_tip) {
        if let Some(first) = commits.first_mut() {
            first.is_tip = true;
        }
    }
    Ok(commits)
}

fn jj_template(rev: &str, template: &str) -> Result<String, String> {
    let out = Command::new("jj")
        .args(["log", "-r", rev, "--no-graph", "-T", template])
        .output()
        .map_err(|e| format!("failed to run jj: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn jj_files(rev: &str) -> Result<Vec<String>, String> {
    let out = Command::new("jj")
        .args(["diff", "-r", rev, "--name-only"])
        .output()
        .map_err(|e| format!("failed to run jj: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

/// Collect issue numbers the linted branch appears to be tied to.
/// Two heuristics, OR-combined:
///
/// 1. Any bookmark on a commit in the revset has an `iN` segment
///    (matches `iN-`, `iN/`, `-iN`, or bare `iN`). This is the
///    "branch name carries the issue" pattern the issue body cites.
/// 2. The workspace this command runs in has a persisted `--issue` link
///    (`shaka workspace new --issue N` writes one). This catches our
///    convention of descriptive bookmark names (`feat/shaka-foo`) that
///    don't encode the issue in the name.
///
/// PR-body cross-reference (third heuristic mentioned in #146) is
/// deferred — only relevant post-push, when the user has already lost
/// the chance for the lint to catch the missing `Closes #N`.
fn gather_tied_issues(revset: &str) -> Vec<u64> {
    let mut out: BTreeSet<u64> = BTreeSet::new();
    if let Ok(names) = jj::bookmarks_on(revset) {
        for n in issue_numbers_from_bookmarks(&names) {
            out.insert(n);
        }
    }
    if let Some(n) = workspace::current_issue() {
        out.insert(n);
    }
    out.into_iter().collect()
}

/// Pure helper: extract issue numbers from bookmark names that match
/// the `iN` convention. Recognized shapes within a bookmark's
/// `-`/`/`-separated segments: `iN`, `iN-...`, `...-iN`, `iN/...`.
/// Order-preserving by first occurrence; deduplicated across the slice.
pub(crate) fn issue_numbers_from_bookmarks(names: &[String]) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    let mut seen: BTreeSet<u64> = BTreeSet::new();
    for name in names {
        for segment in name.split(['/', '-']) {
            if let Some(rest) = segment.strip_prefix('i') {
                if let Ok(n) = rest.parse::<u64>() {
                    if seen.insert(n) {
                        out.push(n);
                    }
                }
            }
        }
    }
    out
}

/// Check that the tip commit's body has a GitHub-autoclose reference
/// to one of `tied_issues`. Returns `None` when the rule passes (or
/// doesn't apply); `Some(Finding)` when the tip is missing the link.
fn check_issue_link(commit: &Commit, tied_issues: &[u64]) -> Option<Finding> {
    if !commit.is_tip || tied_issues.is_empty() {
        return None;
    }
    let body = commit
        .description
        .split_once('\n')
        .map(|(_, rest)| rest)
        .unwrap_or("");
    let body_refs = issue_ref::extract_autoclose_refs(body);
    if tied_issues.iter().any(|n| body_refs.contains(n)) {
        return None;
    }
    let expected: Vec<String> = tied_issues.iter().map(|n| format!("#{n}")).collect();
    Some(Finding {
        severity: Severity::Error,
        message: format!(
            "tip commit lacks `Closes #N` for tied issue(s) {} — \
             add `Closes #<N>` to the body, or pass `--allow-no-issue-link` \
             if this branch legitimately doesn't close an issue",
            expected.join(", ")
        ),
    })
}

fn lint_commit(commit: &Commit, ctx: &LintContext) -> Vec<Finding> {
    let mut findings = Vec::new();
    let desc = commit.description.trim_end();

    // Merge commits (≥ 2 parents) carry titles like
    // `Merge branch 'main' into <branch>` which fail the conventional
    // commit regex. Short-circuit with a specific message that points
    // at the most common source (GitHub's "Update branch" UI button)
    // and the recovery path, so the user isn't left guessing what a
    // generic title-format error means.
    if commit.parent_count >= 2 {
        findings.push(Finding {
            severity: Severity::Error,
            message: format!(
                "merge commit ({} parents); usually from GitHub's \"Update branch\" \
                 button. `auto-rebase-prs.yaml` handles main-movement on the next \
                 push:main; to recover now, rebase locally with:\n    \
                 jj rebase --branch <bookmark> -d main@origin\n    \
                 jj git push --bookmark <bookmark>",
                commit.parent_count
            ),
        });
        return findings;
    }

    if desc.is_empty() || desc == "(no description set)" {
        findings.push(Finding {
            severity: Severity::Error,
            message: "empty description".into(),
        });
        return findings;
    }

    let mut lines = desc.lines();
    let title = lines.next().unwrap_or("");

    if let Err(e) = title::validate(title) {
        findings.push(Finding {
            severity: Severity::Error,
            message: title_finding_message(&e),
        });
    }

    if title.chars().count() > TITLE_MAX {
        findings.push(Finding {
            severity: Severity::Error,
            message: format!("title is {} chars (max {TITLE_MAX})", title.chars().count()),
        });
    }

    let body_lines: Vec<&str> = lines.collect();
    if !body_lines.is_empty() {
        if !body_lines[0].is_empty() {
            findings.push(Finding {
                severity: Severity::Error,
                message: "title and body must be separated by a blank line".into(),
            });
        }
        for (i, line) in body_lines.iter().enumerate() {
            let len = line.chars().count();
            if len > BODY_LINE_MAX {
                findings.push(Finding {
                    severity: Severity::Warn,
                    message: format!("body line {} is {len} chars (max {BODY_LINE_MAX})", i + 2),
                });
            }
        }
    }

    if let Some(f) = check_atomicity(&commit.files) {
        findings.push(f);
    }

    if !ctx.allow_no_issue_link {
        if let Some(f) = check_issue_link(commit, &ctx.tied_issues) {
            findings.push(f);
        }
    }

    findings
}

/// Render a `TitleError` into the human-facing string `commit lint`
/// surfaces. The grammar errors get a uniform `does not match` prefix
/// (since the structural reason is usually less interesting than
/// "title format is wrong"); type / scope errors carry their detail
/// from the validator directly.
fn title_finding_message(err: &TitleError) -> String {
    match err {
        TitleError::MissingSeparator | TitleError::EmptySubject => format!(
            "title does not match `<type>(<scope>): <subject>` (type ∈ {{{}}})",
            title::TYPES.join(", ")
        ),
        TitleError::UnknownType { .. } | TitleError::InvalidScope { .. } => err.to_string(),
    }
}

fn check_atomicity(files: &[String]) -> Option<Finding> {
    let projects: BTreeSet<String> = files.iter().filter_map(|f| project_of(f)).collect();

    if projects.len() > 1 {
        let names: Vec<String> = projects.into_iter().collect();
        return Some(Finding {
            severity: Severity::Warn,
            message: format!("commit spans multiple projects: {}", names.join(", ")),
        });
    }
    None
}

fn project_of(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    let slot = parts.next()?;
    if !SLOTS.contains(&slot) {
        return None;
    }
    let project = parts.next()?;
    if project.is_empty() {
        return None;
    }
    Some(format!("{slot}/{project}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(msg: &str) -> Finding {
        Finding {
            severity: Severity::Error,
            message: msg.into(),
        }
    }

    fn lint_desc(desc: &str) -> Vec<Finding> {
        lint_with_parents(desc, 1)
    }

    fn lint_with_parents(desc: &str, parent_count: usize) -> Vec<Finding> {
        let commit = Commit {
            change_id: "abc12345".into(),
            description: desc.into(),
            files: vec![],
            parent_count,
            is_tip: true,
        };
        lint_commit(&commit, &noop_ctx())
    }

    fn noop_ctx() -> LintContext {
        LintContext {
            allow_no_issue_link: false,
            tied_issues: vec![],
        }
    }

    // Grammar-level coverage lives in `commit::title::tests`. The lint
    // tests below only assert that lint_commit composes the validator
    // correctly with the other checks (length, body wrap, atomicity).

    #[test]
    fn lint_flags_unknown_type_with_specific_message() {
        let findings = lint_desc("frob: do thing");
        assert!(
            findings.iter().any(|f| f.message.contains("frob")),
            "expected message to name the bad type, got: {findings:?}"
        );
    }

    #[test]
    fn lint_flags_grammar_error_with_format_hint() {
        // MissingSeparator / EmptySubject collapse to the uniform
        // "does not match `<type>(<scope>): <subject>`" message so the
        // hint stays the same regardless of which structural rule
        // failed first.
        let findings = lint_desc("feat add thing");
        assert!(
            findings
                .iter()
                .any(|f| f.message.contains("does not match")),
            "got: {findings:?}"
        );
    }

    #[test]
    fn lint_flags_empty_description() {
        let findings = lint_desc("");
        assert!(findings
            .iter()
            .any(|f| matches!(f.severity, Severity::Error) && f.message.contains("empty")));
    }

    #[test]
    fn lint_flags_jj_placeholder_description() {
        let findings = lint_desc("(no description set)");
        assert!(findings
            .iter()
            .any(|f| matches!(f.severity, Severity::Error) && f.message.contains("empty")));
    }

    #[test]
    fn lint_passes_simple_valid_title() {
        assert!(lint_desc("feat(shaka): add the thing").is_empty());
    }

    #[test]
    fn lint_flags_two_parent_merge_commit() {
        let findings = lint_with_parents("Merge branch 'main' into feat/whatever", 2);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, Severity::Error));
        assert!(
            findings[0].message.starts_with("merge commit"),
            "message: {}",
            findings[0].message,
        );
        assert!(findings[0].message.contains("Update branch"));
        assert!(findings[0].message.contains("jj rebase"));
    }

    #[test]
    fn lint_skips_title_format_check_for_merge_commits() {
        // The merge-commit message would also fail the conventional-
        // commit regex; the specific finding should fire alone so the
        // output isn't noisy.
        let findings = lint_with_parents("Merge branch 'main' into feat/whatever", 2);
        assert!(
            !findings
                .iter()
                .any(|f| f.message.contains("title does not match")),
            "expected only the merge-commit finding, got: {findings:?}",
        );
    }

    #[test]
    fn lint_flags_title_too_long() {
        let too_long = format!("feat: {}", "x".repeat(70));
        let findings = lint_desc(&too_long);
        assert!(findings
            .iter()
            .any(|f| f.message.contains("title is") && f.message.contains("max 70")));
    }

    #[test]
    fn lint_flags_title_at_boundary() {
        let exact_70 = format!("feat: {}", "x".repeat(64));
        assert_eq!(exact_70.chars().count(), 70);
        assert!(lint_desc(&exact_70).is_empty());
    }

    #[test]
    fn lint_flags_missing_blank_line_before_body() {
        let findings = lint_desc("feat: thing\nbody right after title");
        assert!(findings.iter().any(|f| {
            matches!(f.severity, Severity::Error) && f.message.contains("blank line")
        }));
    }

    #[test]
    fn lint_warns_on_long_body_line() {
        let body = "x".repeat(90);
        let desc = format!("feat: thing\n\n{body}");
        let findings = lint_desc(&desc);
        assert!(findings.iter().any(|f| {
            matches!(f.severity, Severity::Warn)
                && f.message.contains("body line")
                && f.message.contains("max 80")
        }));
    }

    #[test]
    fn lint_passes_well_wrapped_body() {
        let desc = "feat: thing\n\nThis is a body line under 80 chars.\nAnother line, also fine.";
        assert!(lint_desc(desc).is_empty());
    }

    #[test]
    fn project_of_extracts_slot_project_pair() {
        assert_eq!(
            project_of("tools/shaka/src/main.rs"),
            Some("tools/shaka".into())
        );
        assert_eq!(
            project_of("infra/devbox/terraform/main.tf"),
            Some("infra/devbox".into())
        );
    }

    #[test]
    fn project_of_returns_none_for_root_files() {
        assert_eq!(project_of("flake.nix"), None);
        assert_eq!(project_of("README.md"), None);
        assert_eq!(project_of(".github/workflows/main.yaml"), None);
    }

    #[test]
    fn project_of_returns_none_for_unknown_slot() {
        assert_eq!(project_of("docs/intro/index.md"), None);
        assert_eq!(project_of("projects/legacy/whatever"), None);
    }

    #[test]
    fn check_atomicity_silent_for_single_project() {
        let files: Vec<String> = vec![
            "tools/shaka/src/main.rs".into(),
            "tools/shaka/Cargo.toml".into(),
        ];
        assert!(check_atomicity(&files).is_none());
    }

    #[test]
    fn check_atomicity_silent_for_root_only_files() {
        let files: Vec<String> = vec!["flake.nix".into(), "README.md".into()];
        assert!(check_atomicity(&files).is_none());
    }

    #[test]
    fn check_atomicity_warns_on_cross_project_changes() {
        let files: Vec<String> = vec![
            "tools/shaka/src/main.rs".into(),
            "infra/devbox/main.tf".into(),
        ];
        let finding = check_atomicity(&files).unwrap();
        assert_eq!(finding.severity, Severity::Warn);
        assert!(finding.message.contains("infra/devbox"));
        assert!(finding.message.contains("tools/shaka"));
    }

    #[test]
    fn _ignore_unused_warning() {
        // ensure the err helper compiles even if unused; used to gate findings construction
        let _ = err("ignored");
    }

    // -- issue-link rule -----------------------------------------------

    fn commit_at_tip(desc: &str) -> Commit {
        Commit {
            change_id: "abc12345".into(),
            description: desc.into(),
            files: vec![],
            parent_count: 1,
            is_tip: true,
        }
    }

    #[test]
    fn issue_numbers_from_bookmarks_extracts_i_segments() {
        let names = vec![
            "feat/i42-foo".to_string(),
            "fix/some-thing".to_string(),
            "i101".to_string(),
            "feat/shaka/i200-bar".to_string(),
        ];
        let nums = issue_numbers_from_bookmarks(&names);
        assert_eq!(nums, vec![42, 101, 200]);
    }

    #[test]
    fn issue_numbers_from_bookmarks_dedupes() {
        let names = vec!["feat/i42".to_string(), "feat/i42-foo".to_string()];
        assert_eq!(issue_numbers_from_bookmarks(&names), vec![42]);
    }

    #[test]
    fn issue_numbers_from_bookmarks_ignores_non_issue_segments() {
        // `int` starts with `i` but isn't `i<digits>`.
        let names = vec!["feat/int-types".to_string()];
        assert!(issue_numbers_from_bookmarks(&names).is_empty());
    }

    #[test]
    fn issue_link_passes_when_body_has_closes_for_tied_issue() {
        let commit = commit_at_tip("feat(shaka): add thing\n\nBody text.\n\nCloses #42");
        assert!(check_issue_link(&commit, &[42]).is_none());
    }

    #[test]
    fn issue_link_passes_with_alternate_autoclose_keywords() {
        for body in ["Fixes #42", "Resolves #42", "fixes #42"] {
            let commit = commit_at_tip(&format!("feat: x\n\n{body}"));
            assert!(
                check_issue_link(&commit, &[42]).is_none(),
                "expected pass for body {body:?}"
            );
        }
    }

    #[test]
    fn issue_link_fails_when_body_lacks_autoclose_for_tied_issue() {
        let commit = commit_at_tip("feat(shaka): add thing\n\nBody text.");
        let f = check_issue_link(&commit, &[42]).unwrap();
        assert!(matches!(f.severity, Severity::Error));
        assert!(f.message.contains("#42"), "got: {}", f.message);
        assert!(f.message.contains("Closes"), "got: {}", f.message);
        assert!(
            f.message.contains("--allow-no-issue-link"),
            "should mention the escape hatch: {}",
            f.message
        );
    }

    #[test]
    fn issue_link_fails_when_body_only_has_refs_keyword() {
        // `Refs #N` isn't an autoclose keyword — doesn't satisfy the rule.
        let commit = commit_at_tip("feat: x\n\nRefs #42");
        let f = check_issue_link(&commit, &[42]).unwrap();
        assert!(matches!(f.severity, Severity::Error));
    }

    #[test]
    fn issue_link_skipped_when_no_tied_issues() {
        let commit = commit_at_tip("feat: x\n\nNo closes here.");
        assert!(check_issue_link(&commit, &[]).is_none());
    }

    #[test]
    fn issue_link_skipped_for_non_tip_commits() {
        let mut commit = commit_at_tip("feat: intermediate\n\nNo closes.");
        commit.is_tip = false;
        assert!(check_issue_link(&commit, &[42]).is_none());
    }

    #[test]
    fn issue_link_passes_when_one_of_multiple_tied_issues_referenced() {
        // Branch tied to both #42 and #99 (e.g. bookmark `feat/i42-foo`
        // plus workspace link to #99). Closing either satisfies the rule.
        let commit = commit_at_tip("feat: x\n\nCloses #99");
        assert!(check_issue_link(&commit, &[42, 99]).is_none());
    }

    #[test]
    fn allow_no_issue_link_suppresses_check_via_context() {
        let commit = commit_at_tip("feat: x\n\nNo closes here.");
        let ctx = LintContext {
            allow_no_issue_link: true,
            tied_issues: vec![42],
        };
        let findings = lint_commit(&commit, &ctx);
        assert!(
            !findings.iter().any(|f| f.message.contains("Closes #")),
            "expected suppression, got: {findings:?}"
        );
    }

    #[test]
    fn lint_report_json_shape_is_stable() {
        let report = LintReport {
            commits: vec![
                CommitReport {
                    change_id: "abcdef1234567890".to_string(),
                    title: "feat(x): clean one".to_string(),
                    findings: vec![],
                },
                CommitReport {
                    change_id: "0011223344556677".to_string(),
                    title: "bad title".to_string(),
                    findings: vec![
                        Finding {
                            severity: Severity::Error,
                            message: "title must follow <type>(<scope>): <subject>".to_string(),
                        },
                        Finding {
                            severity: Severity::Warn,
                            message: "commit spans multiple projects".to_string(),
                        },
                    ],
                },
            ],
            total_errors: 1,
            total_warnings: 1,
            clean: 1,
            success: false,
        };
        let v: serde_json::Value = serde_json::to_value(&report).unwrap();

        assert_eq!(v["success"], false);
        assert_eq!(v["total_errors"], 1);
        assert_eq!(v["total_warnings"], 1);
        assert_eq!(v["clean"], 1);
        assert_eq!(v["commits"][0]["change_id"], "abcdef1234567890");
        assert_eq!(v["commits"][0]["findings"].as_array().unwrap().len(), 0);
        assert_eq!(v["commits"][1]["findings"][0]["severity"], "error");
        assert_eq!(v["commits"][1]["findings"][1]["severity"], "warn");
    }
}
