//! Portable hooks invoked by the Claude Code harness via
//! `settings.json`. Each hook is a subcommand of this binary; the
//! harness pipes tool-call JSON on stdin and treats a non-zero exit
//! as "block the tool call".
//!
//! Today the only hook is `pre-issue-create`, which intercepts
//! `gh issue create` calls and forces a duplicate-search step.
//!
//! This binary intentionally has no dependency on the kolohelios
//! repo or `shaka`. Once installed globally via home-manager (#545)
//! and wired into `~/.claude/settings.json` (#534), the hook fires
//! from any working directory, not just inside a kolohelios
//! checkout. The matching logic is a port of the original
//! implementation in `tools/shaka/src/claude.rs` (#524) — same
//! tests, same semantics, lifted out so the global wiring is
//! possible.

use std::io::Read;
use std::process::Command;

use clap::{Parser, Subcommand};
use serde::Deserialize;

/// Env-var prefix that lets the user opt out of the duplicate-check
/// for a specific `gh issue create` call. Detected by string-matching
/// the front of the bash command, since pre-command env vars don't
/// flow through to this process's environment.
const BYPASS_VAR: &str = "BYPASS_ISSUE_DUP_CHECK";

/// Cap on the number of candidate matches we surface. Keeps the
/// error output skimmable; if there are more, the user can re-run
/// the search manually.
const MAX_CANDIDATES: usize = 5;

/// ANSI escape sequences for skim-friendly output. Inlined rather
/// than pulling a color crate — this is a 100-line binary and the
/// few sequences we use are stable.
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";

#[derive(Parser)]
#[command(
    name = "claude-hooks",
    about = "Portable Claude Code harness hooks (PreToolUse, ...)"
)]
struct Cli {
    #[command(subcommand)]
    command: Hook,
}

#[derive(Subcommand)]
enum Hook {
    /// PreToolUse hook on the `Bash` tool. Intercepts `gh issue create`
    /// invocations and runs a duplicate-search; blocks with a list of
    /// candidate matches if any open issue's title overlaps.
    PreIssueCreate,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Hook::PreIssueCreate => pre_issue_create(),
    }
}

/// Top-level shape of the JSON the harness pipes on stdin for any
/// PreToolUse hook. We only consume `tool_name` and `tool_input.command`
/// for Bash; the rest is ignored.
#[derive(Debug, Deserialize)]
struct HookInput {
    tool_name: String,
    tool_input: ToolInput,
}

#[derive(Debug, Deserialize)]
struct ToolInput {
    #[serde(default)]
    command: String,
}

fn pre_issue_create() {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        // No stdin (or unreadable). Don't block the tool — letting
        // a misconfigured hook silently neuter every Bash call is
        // a worse failure mode than the duplicate it was meant to
        // catch.
        std::process::exit(0);
    }

    let input: HookInput = match serde_json::from_str(&buf) {
        Ok(v) => v,
        Err(_) => std::process::exit(0),
    };

    // Cheap early exits — anything other than a Bash call running
    // `gh issue create` doesn't interest us.
    if input.tool_name != "Bash" || !is_gh_issue_create(&input.tool_input.command) {
        std::process::exit(0);
    }

    if has_bypass_prefix(&input.tool_input.command) {
        eprintln!(
            "{YELLOW}{BOLD}note:{RESET} {BYPASS_VAR}=1 prefix present; skipping duplicate check"
        );
        std::process::exit(0);
    }

    let title = match extract_title(&input.tool_input.command) {
        Some(t) if !t.trim().is_empty() => t,
        _ => {
            eprintln!(
                "{YELLOW}{BOLD}warn:{RESET} could not extract --title from the command; \
                 skipping duplicate check"
            );
            std::process::exit(0);
        }
    };

    let candidates = match search_open_issues(&title) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{YELLOW}{BOLD}warn:{RESET} duplicate-check failed ({e}); \
                 letting the create proceed"
            );
            std::process::exit(0);
        }
    };

    if candidates.is_empty() {
        std::process::exit(0);
    }

    eprintln!(
        "{RED}{BOLD}blocked:{RESET} duplicate-check found {} open issue(s) matching {title:?}:",
        candidates.len()
    );
    for c in candidates.iter().take(MAX_CANDIDATES) {
        eprintln!("  {BOLD}#{}{RESET} {}", c.number, c.title);
    }
    eprintln!();
    eprintln!(
        "{DIM}If this is a genuinely new issue, re-run with \
         `{BYPASS_VAR}=1 gh issue create ...` to bypass the check.{RESET}"
    );
    std::process::exit(1);
}

/// Whether the command line looks like a `gh issue create` invocation.
/// Matches both `gh issue create` and `gh issue create --foo ...`; does
/// NOT match `gh issue close`, comments, etc.
fn is_gh_issue_create(command: &str) -> bool {
    let Some(tokens) = shlex::split(command) else {
        return false;
    };
    // Skip leading env-var assignments (`FOO=bar gh issue create ...`)
    // so the bypass-prefix path still gets recognized as an issue-create
    // for the no-op case.
    let mut iter = tokens.iter().skip_while(|t| t.contains('='));
    matches!(
        (iter.next(), iter.next(), iter.next()),
        (Some(g), Some(i), Some(c)) if g == "gh" && i == "issue" && c == "create"
    )
}

/// True when the command opens with `BYPASS_ISSUE_DUP_CHECK=<value>`.
/// The exact value is unimportant; any non-empty assignment signals
/// intent.
fn has_bypass_prefix(command: &str) -> bool {
    let Some(tokens) = shlex::split(command) else {
        return false;
    };
    let prefix = format!("{BYPASS_VAR}=");
    tokens
        .iter()
        .take_while(|t| t.contains('='))
        .any(|t| t.starts_with(&prefix) && t.len() > prefix.len())
}

/// Extract the `--title` value. Handles `--title VALUE`, `--title=VALUE`,
/// and the short `-t` form; quoting is unwrapped by shlex before we
/// see the tokens.
fn extract_title(command: &str) -> Option<String> {
    let tokens = shlex::split(command)?;
    let mut iter = tokens.into_iter();
    while let Some(tok) = iter.next() {
        if tok == "--title" || tok == "-t" {
            return iter.next();
        }
        if let Some(rest) = tok.strip_prefix("--title=") {
            return Some(rest.to_string());
        }
    }
    None
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Candidate {
    number: u64,
    title: String,
}

fn search_open_issues(title: &str) -> Result<Vec<Candidate>, String> {
    // jj workspaces have no `.git` of their own; gh's git-based repo
    // detection fails there. Read the remote via `jj git remote list`
    // and pass `--repo owner/repo` explicitly so the hook works in
    // any working directory, including non-default jj workspaces.
    let repo = detect_repo()?;
    let output = Command::new("gh")
        .args([
            "issue",
            "list",
            "--repo",
            &repo,
            "--state",
            "open",
            "--search",
            title,
            "--limit",
            &MAX_CANDIDATES.to_string(),
            "--json",
            "number,title",
        ])
        .output()
        .map_err(|e| format!("could not invoke gh: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "gh exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    serde_json::from_slice::<Vec<Candidate>>(&output.stdout)
        .map_err(|e| format!("could not parse gh output: {e}"))
}

/// Detect `owner/repo` from the jj git remote named "origin", with a
/// `GITHUB_REPOSITORY` env-var override. `jj git remote list` works
/// in non-default jj workspaces (which have no `.git` of their own —
/// just a `.jj` pointing at the shared repo) where `git remote` and
/// `gh`'s default detection both fail.
fn detect_repo() -> Result<String, String> {
    if let Ok(r) = std::env::var("GITHUB_REPOSITORY") {
        if !r.is_empty() {
            return Ok(r);
        }
    }

    let output = Command::new("jj")
        .args(["git", "remote", "list"])
        .output()
        .map_err(|e| format!("could not invoke jj: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "jj git remote list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Each line is "<name> <url>"; find the one named "origin".
    let url = stdout
        .lines()
        .find_map(|line| line.strip_prefix("origin "))
        .ok_or_else(|| "no jj remote named 'origin'".to_string())?
        .trim();
    parse_repo_from_url(url).ok_or_else(|| format!("could not parse owner/repo from URL {url:?}"))
}

/// Pull `owner/repo` out of any of the GitHub URL shapes:
/// `https://github.com/owner/repo(.git)`, `git@github.com:owner/repo(.git)`,
/// `ssh://git@github.com/owner/repo(.git)`. Returns `None` on
/// non-GitHub URLs or shapes we don't recognize.
fn parse_repo_from_url(url: &str) -> Option<String> {
    // Normalize to "owner/repo[.git]" by stripping the scheme/host
    // prefix in each supported shape.
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    let trimmed = path.trim_end_matches('/').trim_end_matches(".git");
    let segments: Vec<&str> = trimmed.split('/').collect();
    if segments.len() == 2 && !segments[0].is_empty() && !segments[1].is_empty() {
        Some(format!("{}/{}", segments[0], segments[1]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_gh_issue_create() {
        assert!(is_gh_issue_create("gh issue create --title \"x\""));
        assert!(is_gh_issue_create("gh issue create"));
    }

    #[test]
    fn ignores_other_gh_subcommands() {
        assert!(!is_gh_issue_create("gh issue list"));
        assert!(!is_gh_issue_create("gh issue close 1"));
        assert!(!is_gh_issue_create("gh pr create"));
    }

    #[test]
    fn detects_gh_issue_create_with_env_prefix() {
        assert!(is_gh_issue_create(
            "BYPASS_ISSUE_DUP_CHECK=1 gh issue create --title x"
        ));
        assert!(is_gh_issue_create("FOO=bar BAZ=qux gh issue create"));
    }

    #[test]
    fn ignores_unrelated_commands() {
        assert!(!is_gh_issue_create("echo hi"));
        assert!(!is_gh_issue_create(""));
        assert!(!is_gh_issue_create("ls -la"));
    }

    #[test]
    fn bypass_prefix_present_with_value_1() {
        assert!(has_bypass_prefix(
            "BYPASS_ISSUE_DUP_CHECK=1 gh issue create --title x"
        ));
    }

    #[test]
    fn bypass_prefix_present_with_non_numeric_value() {
        assert!(has_bypass_prefix(
            "BYPASS_ISSUE_DUP_CHECK=true gh issue create --title x"
        ));
        assert!(has_bypass_prefix(
            "BYPASS_ISSUE_DUP_CHECK=yes gh issue create --title x"
        ));
    }

    #[test]
    fn bypass_prefix_absent_when_no_value() {
        assert!(!has_bypass_prefix(
            "BYPASS_ISSUE_DUP_CHECK= gh issue create --title x"
        ));
    }

    #[test]
    fn bypass_prefix_absent_in_normal_command() {
        assert!(!has_bypass_prefix("gh issue create --title x"));
        assert!(!has_bypass_prefix("FOO=bar gh issue create --title x"));
    }

    #[test]
    fn extracts_title_after_space() {
        assert_eq!(
            extract_title("gh issue create --title \"my title\" --body x").as_deref(),
            Some("my title")
        );
    }

    #[test]
    fn extracts_title_with_equals_form() {
        assert_eq!(
            extract_title("gh issue create --title=\"my title\"").as_deref(),
            Some("my title")
        );
    }

    #[test]
    fn extracts_title_short_flag() {
        assert_eq!(
            extract_title("gh issue create -t \"my title\"").as_deref(),
            Some("my title")
        );
    }

    #[test]
    fn extracts_title_single_quoted() {
        assert_eq!(
            extract_title("gh issue create --title 'single quoted'").as_deref(),
            Some("single quoted")
        );
    }

    #[test]
    fn extracts_title_unquoted_single_word() {
        assert_eq!(
            extract_title("gh issue create --title bug").as_deref(),
            Some("bug")
        );
    }

    #[test]
    fn extracts_title_returns_none_when_absent() {
        assert!(extract_title("gh issue create --body x").is_none());
    }

    #[test]
    fn extracts_title_returns_none_when_no_value_after_flag() {
        assert!(extract_title("gh issue create --title").is_none());
    }

    #[test]
    fn parses_https_github_url() {
        assert_eq!(
            parse_repo_from_url("https://github.com/kolohelios/kolohelios.git").as_deref(),
            Some("kolohelios/kolohelios")
        );
        assert_eq!(
            parse_repo_from_url("https://github.com/owner/repo").as_deref(),
            Some("owner/repo")
        );
    }

    #[test]
    fn parses_ssh_github_url() {
        assert_eq!(
            parse_repo_from_url("git@github.com:kolohelios/kolohelios.git").as_deref(),
            Some("kolohelios/kolohelios")
        );
        assert_eq!(
            parse_repo_from_url("ssh://git@github.com/owner/repo").as_deref(),
            Some("owner/repo")
        );
    }

    #[test]
    fn rejects_non_github_url() {
        assert!(parse_repo_from_url("https://gitlab.com/owner/repo").is_none());
        assert!(parse_repo_from_url("not a url").is_none());
        assert!(parse_repo_from_url("").is_none());
    }
}
