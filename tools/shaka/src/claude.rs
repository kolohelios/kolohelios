//! Hooks invoked by the Claude Code harness via
//! `.claude/settings.json`. The harness pipes tool-call JSON on stdin
//! and treats a non-zero exit as "block the tool call"; stderr is
//! surfaced back to Claude.
//!
//! Today the only hook is `pre-issue-create`, which intercepts
//! `gh issue create` calls and forces a duplicate-search step. The
//! rule used to be a memory note ("search before filing"); memory
//! turned out to be too easy to ignore — see #515 for the
//! motivating case.
//!
//! New hooks land as new `HookCommand` variants. Each variant is one
//! `match` arm + one `fn` here; matching against `tool_name` and
//! `tool_input.command` happens inside the hook fn so each hook can
//! exit 0 cheaply for tool calls it doesn't care about.

use std::io::Read;
use std::process::Command;

use clap::Subcommand;
use serde::Deserialize;

use crate::gh;
use crate::term::{BOLD, DIM, RED, RESET, YELLOW};

/// Env-var prefix that lets the user opt out of the duplicate-check
/// for a specific `gh issue create` call. Detected by string-matching
/// the front of the bash command, since pre-command env vars don't
/// flow through to the hook's process environment.
const BYPASS_VAR: &str = "BYPASS_ISSUE_DUP_CHECK";

/// Cap on the number of candidate matches we surface. Keeps the
/// error output skimmable; if there are more, the user can re-run
/// the search manually.
const MAX_CANDIDATES: usize = 5;

#[derive(Subcommand)]
pub enum ClaudeCommand {
    /// Hooks invoked by the Claude Code harness via `.claude/settings.json`.
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
}

#[derive(Subcommand)]
pub enum HookCommand {
    /// PreToolUse hook on the `Bash` tool. Intercepts `gh issue create`
    /// invocations and runs a duplicate-search; blocks with a list of
    /// candidate matches if any open issue's title overlaps.
    PreIssueCreate,
}

pub fn run(cmd: ClaudeCommand) {
    match cmd {
        ClaudeCommand::Hook { command } => match command {
            HookCommand::PreIssueCreate => pre_issue_create(),
        },
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
            // Couldn't find a title — don't block, but tell the user
            // why the hook isn't helping. The `gh issue create` call
            // proceeds; the only loss is the search step.
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
            // gh failed (network, auth, rate limit). Don't block the
            // create — the user may be filing a perfectly valid new
            // issue and a transient gh failure shouldn't gate that.
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

/// True when the command opens with `BYPASS_ISSUE_DUP_CHECK=1` (or
/// `=true`, or any non-empty value). The intent is "explicitly opt
/// out for this call"; the exact value is unimportant.
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

/// Extract the `--title` value. Handles both `--title VALUE` and
/// `--title=VALUE` shapes; quoting is unwrapped by shlex before we
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
    // detection fails there. shaka's `detect_repo_or_env` reads the
    // remote via `jj git remote list`, which works in any workspace.
    let repo = gh::detect_repo_or_env().map_err(|e| format!("could not detect repo: {e}"))?;
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
        // The hook should still recognize the underlying command
        // shape even with a leading env var; downstream the bypass
        // check decides whether to proceed.
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
        // We don't constrain the value — any non-empty assignment
        // signals intent.
        assert!(has_bypass_prefix(
            "BYPASS_ISSUE_DUP_CHECK=true gh issue create --title x"
        ));
        assert!(has_bypass_prefix(
            "BYPASS_ISSUE_DUP_CHECK=yes gh issue create --title x"
        ));
    }

    #[test]
    fn bypass_prefix_absent_when_no_value() {
        // Bare `BYPASS_ISSUE_DUP_CHECK=` (empty value) shouldn't
        // bypass — `=` with nothing on the right is almost certainly
        // a typo, not intent.
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
        // -t is gh's short form for --title; the hook should pick
        // both up (Claude has been observed using --title; users
        // sometimes prefer -t).
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
        // `--title` at the very end of the command — no value to
        // grab.
        assert!(extract_title("gh issue create --title").is_none());
    }
}
