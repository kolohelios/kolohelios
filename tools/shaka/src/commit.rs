use std::collections::BTreeSet;
use std::process::Command;

use clap::Subcommand;

const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

const TYPES: &[&str] = &[
    "feat", "fix", "docs", "chore", "refactor", "test", "style", "perf", "ci", "build",
];
const SLOTS: &[&str] = &["apps", "infra", "packages", "services", "tools"];
const TITLE_MAX: usize = 70;
const BODY_LINE_MAX: usize = 80;

#[derive(Subcommand)]
pub enum CommitCommand {
    /// Lint commit descriptions against conventional commit conventions
    Lint {
        /// Revset to lint (jj syntax). Defaults to the working copy commit.
        #[arg(short = 'r', long, default_value = "@")]
        revset: String,
    },
}

pub fn run(cmd: CommitCommand) {
    match cmd {
        CommitCommand::Lint { revset } => lint(&revset),
    }
}

fn lint(revset: &str) {
    let commits = match collect_commits(revset) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    if commits.is_empty() {
        println!("no commits found in revset {revset:?}");
        return;
    }

    let mut total_errors = 0usize;
    let mut total_warnings = 0usize;
    let mut clean = 0usize;

    for commit in &commits {
        let findings = lint_commit(commit);
        let errors = findings.iter().filter(|f| matches!(f.severity, Severity::Error)).count();
        let warnings = findings.iter().filter(|f| matches!(f.severity, Severity::Warn)).count();

        if findings.is_empty() {
            clean += 1;
        } else {
            print_commit_findings(commit, &findings);
        }

        total_errors += errors;
        total_warnings += warnings;
    }

    println!();
    if total_errors > 0 {
        eprintln!(
            "{RED}{BOLD}commit lint failed{RESET} ({} commit(s), {total_errors} error(s), {total_warnings} warning(s))",
            commits.len()
        );
        std::process::exit(1);
    }
    let warn_str = if total_warnings > 0 {
        format!(" ({total_warnings} warning(s))")
    } else {
        String::new()
    };
    println!(
        "{GREEN}{BOLD}commit lint passed{RESET} ({clean}/{} clean){warn_str}",
        commits.len()
    );
}

fn print_commit_findings(commit: &Commit, findings: &[Finding]) {
    let title = commit
        .description
        .lines()
        .next()
        .unwrap_or("(empty)")
        .to_string();
    println!("\n{BOLD}{} {DIM}{}{RESET}", commit.short_id(), title);
    for f in findings {
        let label = match f.severity {
            Severity::Error => format!("{RED}{BOLD}error{RESET}"),
            Severity::Warn => format!("{YELLOW}{BOLD}warn{RESET} "),
        };
        println!("  {label}  {}", f.message);
    }
}

#[derive(Debug)]
struct Commit {
    change_id: String,
    description: String,
    files: Vec<String>,
}

impl Commit {
    fn short_id(&self) -> &str {
        &self.change_id[..self.change_id.len().min(8)]
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Severity {
    Error,
    Warn,
}

#[derive(Debug)]
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
        return Err(String::from_utf8_lossy(&ids_output.stderr).trim().to_string());
    }

    let mut commits = Vec::new();
    for line in String::from_utf8_lossy(&ids_output.stdout).lines() {
        let id = line.trim();
        if id.is_empty() {
            continue;
        }
        let description = jj_template(id, "description")?;
        let files = jj_files(id)?;
        commits.push(Commit {
            change_id: id.to_string(),
            description,
            files,
        });
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

fn lint_commit(commit: &Commit) -> Vec<Finding> {
    let mut findings = Vec::new();
    let desc = commit.description.trim_end();

    if desc.is_empty() || desc == "(no description set)" {
        findings.push(Finding {
            severity: Severity::Error,
            message: "empty description".into(),
        });
        return findings;
    }

    let mut lines = desc.lines();
    let title = lines.next().unwrap_or("");

    if !title_matches_format(title) {
        findings.push(Finding {
            severity: Severity::Error,
            message: format!(
                "title does not match `<type>(<scope>): <subject>` (type ∈ {{{}}})",
                TYPES.join(", ")
            ),
        });
    }

    if title.chars().count() > TITLE_MAX {
        findings.push(Finding {
            severity: Severity::Error,
            message: format!(
                "title is {} chars (max {TITLE_MAX})",
                title.chars().count()
            ),
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
                    message: format!(
                        "body line {} is {len} chars (max {BODY_LINE_MAX})",
                        i + 2
                    ),
                });
            }
        }
    }

    if let Some(f) = check_atomicity(&commit.files) {
        findings.push(f);
    }

    findings
}

fn title_matches_format(title: &str) -> bool {
    let Some(colon_idx) = title.find(": ") else {
        return false;
    };
    let prefix = &title[..colon_idx];
    let subject = &title[colon_idx + 2..];

    if subject.is_empty() {
        return false;
    }

    let (type_part, scope_part) = match prefix.find('(') {
        Some(open) => {
            if !prefix.ends_with(')') {
                return false;
            }
            let close = prefix.len() - 1;
            (&prefix[..open], Some(&prefix[open + 1..close]))
        }
        None => (prefix, None),
    };

    if !TYPES.contains(&type_part) {
        return false;
    }

    if let Some(scope) = scope_part {
        if scope.is_empty() {
            return false;
        }
        if !scope
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/')
        {
            return false;
        }
    }

    true
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
        let commit = Commit {
            change_id: "abc12345".into(),
            description: desc.into(),
            files: vec![],
        };
        lint_commit(&commit)
    }

    #[test]
    fn title_format_matches_simple_type() {
        assert!(title_matches_format("feat: add a thing"));
        assert!(title_matches_format("fix: handle edge case"));
    }

    #[test]
    fn title_format_matches_type_with_scope() {
        assert!(title_matches_format("feat(shaka): add command"));
        assert!(title_matches_format("ci(infra/devbox): scope job"));
    }

    #[test]
    fn title_format_rejects_unknown_type() {
        assert!(!title_matches_format("foo: do thing"));
        assert!(!title_matches_format("FEAT: capital"));
    }

    #[test]
    fn title_format_rejects_missing_subject() {
        assert!(!title_matches_format("feat: "));
        assert!(!title_matches_format("feat:"));
    }

    #[test]
    fn title_format_rejects_missing_colon_space() {
        assert!(!title_matches_format("feat:add thing"));
    }

    #[test]
    fn title_format_rejects_empty_or_unclosed_scope() {
        assert!(!title_matches_format("feat(): bad"));
        assert!(!title_matches_format("feat(scope: bad"));
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
}
