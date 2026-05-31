use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

use crate::gh::{self, OpenPr};
use crate::term::{BOLD, GREEN, RED, RESET, YELLOW};

const STATUS_CONTEXT: &str = "auto-rebase";
const OPT_OUT_LABEL: &str = "do-not-rebase";
const STATUS_DESCRIPTION_LIMIT: usize = 140;
const GATE_CHECK_NAME: &str = "Gate";

pub fn run(dry_run: bool) {
    let repo = match gh::detect_repo_or_env() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    let prs = match gh::list_open_prs_against("main") {
        Ok(prs) => prs,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} listing open PRs: {e}");
            std::process::exit(1);
        }
    };

    if prs.is_empty() {
        println!("no open PRs against main");
        return;
    }

    println!(
        "{BOLD}{} open PR{} against main{RESET}",
        prs.len(),
        if prs.len() == 1 { "" } else { "s" }
    );

    let target_url = github_run_url();
    let mut had_conflict = false;

    for pr in prs {
        if pr.labels.iter().any(|l| l == OPT_OUT_LABEL) {
            println!(
                "  {YELLOW}skip{RESET} #{} (label '{OPT_OUT_LABEL}')",
                pr.number
            );
            continue;
        }
        match rebase_one(&repo, &pr, target_url.as_deref(), dry_run) {
            Ok(Outcome::UpToDate) => {
                println!("  {GREEN}up-to-date{RESET} #{}", pr.number);
            }
            Ok(Outcome::Rebased { new_sha }) => {
                println!(
                    "  {GREEN}rebased{RESET} #{} (now {})",
                    pr.number,
                    short_sha(&new_sha)
                );
            }
            Ok(Outcome::NoOverlap) => {
                println!(
                    "  {GREEN}rebased{RESET} #{} (no file overlap, skipped CI)",
                    pr.number
                );
            }
            Ok(Outcome::WouldRebase) => {
                println!("  {YELLOW}would rebase{RESET} #{}", pr.number);
            }
            Ok(Outcome::ConcurrentPush) => {
                println!(
                    "  {YELLOW}deferred{RESET} #{} (head moved during rebase)",
                    pr.number
                );
            }
            Ok(Outcome::Conflict { files }) => {
                had_conflict = true;
                println!(
                    "  {RED}conflict{RESET} #{} — {}",
                    pr.number,
                    summarize_files(&files)
                );
            }
            Err(e) => {
                eprintln!("{RED}{BOLD}error{RESET} #{}: {e}", pr.number);
                std::process::exit(1);
            }
        }
    }

    if had_conflict {
        std::process::exit(1);
    }
}

#[derive(Debug)]
enum Outcome {
    UpToDate,
    Rebased { new_sha: String },
    NoOverlap,
    WouldRebase,
    ConcurrentPush,
    Conflict { files: Vec<String> },
}

fn rebase_one(
    repo: &str,
    pr: &OpenPr,
    target_url: Option<&str>,
    dry_run: bool,
) -> Result<Outcome, String> {
    git(&["fetch", "origin", &pr.head_ref, "main"])?;

    let merge_base = git_capture(&["merge-base", "origin/main", &pr.head_sha])?
        .trim()
        .to_string();
    let main_sha = git_capture(&["rev-parse", "origin/main"])?
        .trim()
        .to_string();
    if merge_base == main_sha {
        return Ok(Outcome::UpToDate);
    }

    if dry_run {
        return Ok(Outcome::WouldRebase);
    }

    let main_projects = changed_projects(&merge_base, &main_sha)?;
    let pr_projects = changed_projects(&merge_base, &pr.head_sha)?;
    let has_overlap = main_projects.iter().any(|p| pr_projects.contains(p));
    let gate_passed = !has_overlap && gate_check_passed(repo, &pr.head_sha);

    let worktree = PathBuf::from(format!("/tmp/auto-rebase-{}", pr.number));
    cleanup_worktree(&worktree);
    git(&[
        "worktree",
        "add",
        "--detach",
        worktree
            .to_str()
            .ok_or_else(|| "non-utf8 worktree path".to_string())?,
        &pr.head_sha,
    ])?;

    let result = run_rebase(repo, pr, &worktree, target_url, gate_passed);
    cleanup_worktree(&worktree);
    result
}

fn run_rebase(
    repo: &str,
    pr: &OpenPr,
    worktree: &Path,
    target_url: Option<&str>,
    skip_push: bool,
) -> Result<Outcome, String> {
    let rebase = Command::new("git")
        .current_dir(worktree)
        .args(["rebase", "origin/main"])
        .output()
        .map_err(|e| format!("git rebase: {e}"))?;

    if !rebase.status.success() {
        let files = unmerged_files(worktree);
        let _ = Command::new("git")
            .current_dir(worktree)
            .args(["rebase", "--abort"])
            .output();
        let description = format!("Rebase failed — {}", summarize_files(&files));
        post_status(repo, &pr.head_sha, "failure", target_url, &description)?;
        return Ok(Outcome::Conflict { files });
    }

    if skip_push {
        let main_short = git_capture(&["rev-parse", "--short", "origin/main"])?
            .trim()
            .to_string();
        post_status(
            repo,
            &pr.head_sha,
            "success",
            target_url,
            &format!("Rebased onto main@{main_short} (no file overlap, CI skipped)"),
        )?;
        return Ok(Outcome::NoOverlap);
    }

    let new_sha = git_capture_in(worktree, &["rev-parse", "HEAD"])?
        .trim()
        .to_string();

    let push = Command::new("git")
        .current_dir(worktree)
        .args([
            "push",
            &format!("--force-with-lease={}:{}", pr.head_ref, pr.head_sha),
            "origin",
            &format!("HEAD:refs/heads/{}", pr.head_ref),
        ])
        .output()
        .map_err(|e| format!("git push: {e}"))?;

    if !push.status.success() {
        let stderr = String::from_utf8_lossy(&push.stderr);
        if is_lease_rejection(&stderr) {
            return Ok(Outcome::ConcurrentPush);
        }
        return Err(format!("git push: {}", stderr.trim()));
    }

    let main_short = git_capture(&["rev-parse", "--short", "origin/main"])?
        .trim()
        .to_string();
    post_status(
        repo,
        &new_sha,
        "success",
        target_url,
        &format!("Rebased onto main@{main_short}"),
    )?;
    Ok(Outcome::Rebased { new_sha })
}

fn gate_check_passed(repo: &str, sha: &str) -> bool {
    let endpoint = format!("/repos/{repo}/commits/{sha}/check-runs?check_name={GATE_CHECK_NAME}");
    let Ok(response) = gh::api_get(&endpoint) else {
        return false;
    };
    response["check_runs"]
        .as_array()
        .and_then(|runs| runs.first())
        .and_then(|run| run["conclusion"].as_str())
        .is_some_and(|c| c == "success")
}

fn changed_projects(base: &str, head: &str) -> Result<HashSet<String>, String> {
    let output = git_capture(&["diff", "--name-only", base, head])?;
    Ok(output.lines().map(project_prefix).collect())
}

fn project_prefix(path: &str) -> String {
    let parts: Vec<&str> = path.splitn(3, '/').collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        path.to_string()
    }
}

fn post_status(
    repo: &str,
    sha: &str,
    state: &str,
    target_url: Option<&str>,
    description: &str,
) -> Result<(), String> {
    let mut body = json!({
        "state": state,
        "context": STATUS_CONTEXT,
        "description": truncate(description, STATUS_DESCRIPTION_LIMIT),
    });
    if let Some(url) = target_url {
        body["target_url"] = json!(url);
    }
    let endpoint = format!("/repos/{repo}/statuses/{sha}");
    gh::api_post(&endpoint, &body).map_err(|e| e.to_string())?;
    Ok(())
}

fn github_run_url() -> Option<String> {
    let server = std::env::var("GITHUB_SERVER_URL")
        .ok()
        .filter(|s| !s.is_empty())?;
    let repo = std::env::var("GITHUB_REPOSITORY")
        .ok()
        .filter(|s| !s.is_empty())?;
    let run_id = std::env::var("GITHUB_RUN_ID")
        .ok()
        .filter(|s| !s.is_empty())?;
    Some(format!("{server}/{repo}/actions/runs/{run_id}"))
}

fn cleanup_worktree(path: &Path) {
    if let Some(s) = path.to_str() {
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force", s])
            .output();
    }
}

fn unmerged_files(worktree: &Path) -> Vec<String> {
    let Ok(output) = Command::new("git")
        .current_dir(worktree)
        .args(["status", "--porcelain"])
        .output()
    else {
        return vec![];
    };
    parse_unmerged(&String::from_utf8_lossy(&output.stdout))
}

fn git(args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.first().unwrap_or(&"")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            stderr.trim()
        ));
    }
    Ok(())
}

fn git_capture(args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.first().unwrap_or(&"")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_capture_in(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.first().unwrap_or(&"")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_unmerged(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let xy = &line[..2];
            if xy.contains('U') || xy == "AA" || xy == "DD" {
                line.get(3..).map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn summarize_files(files: &[String]) -> String {
    if files.is_empty() {
        return "see workflow log for details".into();
    }
    let shown: Vec<&str> = files.iter().take(3).map(|s| s.as_str()).collect();
    if files.len() <= 3 {
        format!("conflicts in: {}", shown.join(", "))
    } else {
        format!(
            "conflicts in: {} (and {} more)",
            shown.join(", "),
            files.len() - 3
        )
    }
}

fn is_lease_rejection(stderr: &str) -> bool {
    stderr.contains("stale info") || stderr.contains("[rejected]")
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmerged_parses_uu_lines() {
        let stdout = "UU src/lib.rs\nUU README.md\n M tools/foo.rs\n?? tmp.txt\n";
        let files = parse_unmerged(stdout);
        assert_eq!(files, vec!["src/lib.rs", "README.md"]);
    }

    #[test]
    fn unmerged_parses_aa_dd() {
        let stdout = "AA both-added.rs\nDD both-deleted.rs\nAU added-by-us.rs\n";
        let files = parse_unmerged(stdout);
        assert_eq!(
            files,
            vec!["both-added.rs", "both-deleted.rs", "added-by-us.rs"]
        );
    }

    #[test]
    fn unmerged_skips_unmodified() {
        assert!(parse_unmerged(" M staged.rs\n M other.rs\n").is_empty());
    }

    #[test]
    fn summarize_empty() {
        assert_eq!(summarize_files(&[]), "see workflow log for details");
    }

    #[test]
    fn summarize_few() {
        let files = vec!["a.rs".into(), "b.rs".into()];
        assert_eq!(summarize_files(&files), "conflicts in: a.rs, b.rs");
    }

    #[test]
    fn summarize_exactly_three() {
        let files = vec!["a.rs".into(), "b.rs".into(), "c.rs".into()];
        assert_eq!(summarize_files(&files), "conflicts in: a.rs, b.rs, c.rs");
    }

    #[test]
    fn summarize_many() {
        let files = vec![
            "a.rs".into(),
            "b.rs".into(),
            "c.rs".into(),
            "d.rs".into(),
            "e.rs".into(),
        ];
        assert_eq!(
            summarize_files(&files),
            "conflicts in: a.rs, b.rs, c.rs (and 2 more)"
        );
    }

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn truncate_exact_length_unchanged() {
        let s = "x".repeat(10);
        assert_eq!(truncate(&s, 10), s);
    }

    #[test]
    fn truncate_long_replaces_with_ellipsis() {
        let s = "x".repeat(200);
        let out = truncate(&s, 140);
        assert_eq!(out.chars().count(), 140);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn lease_rejection_stale_info() {
        assert!(is_lease_rejection(
            " ! [rejected]        feat/x -> feat/x (stale info)"
        ));
    }

    #[test]
    fn lease_rejection_plain_rejected() {
        assert!(is_lease_rejection(
            " ! [rejected]        feat/x -> feat/x (non-fast-forward)"
        ));
    }

    #[test]
    fn lease_rejection_unrelated_error_false() {
        assert!(!is_lease_rejection("fatal: unable to access remote"));
    }

    #[test]
    fn short_sha_truncates() {
        assert_eq!(short_sha("abcdef0123456789"), "abcdef0");
    }

    #[test]
    fn project_prefix_two_components() {
        assert_eq!(project_prefix("tools/shaka/src/main.rs"), "tools/shaka");
    }

    #[test]
    fn project_prefix_deep_path() {
        assert_eq!(
            project_prefix("apps/blogctl/src/commands/doctor.rs"),
            "apps/blogctl"
        );
    }

    #[test]
    fn project_prefix_exact_two() {
        assert_eq!(project_prefix("infra/home"), "infra/home");
    }

    #[test]
    fn project_prefix_root_file() {
        assert_eq!(project_prefix("CLAUDE.md"), "CLAUDE.md");
    }

    #[test]
    fn project_prefix_dotgithub() {
        assert_eq!(
            project_prefix(".github/workflows/main.yaml"),
            ".github/workflows"
        );
    }
}
