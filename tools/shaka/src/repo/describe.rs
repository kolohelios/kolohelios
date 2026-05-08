use serde::Serialize;

use crate::jj::{self, CommitMessage};
use crate::term::{BOLD, RED, RESET};

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Description {
    pub title: String,
    pub body: String,
}

/// Synthesize a PR title and body from the commits in `main@origin..@`.
///
/// Title is the tip commit's title verbatim. Body concatenates each
/// commit's "why" — the body up to (but not including) any trailer
/// footer like `Closes #N` or `Co-authored-by:` — joined with blank-line
/// separators in chronological order. Returns `None` if `commits` is
/// empty.
pub fn build(commits: &[CommitMessage]) -> Option<Description> {
    let tip = commits.last()?;
    let title = tip.title.clone();
    let body = synthesize_body(commits);
    Some(Description { title, body })
}

fn synthesize_body(commits: &[CommitMessage]) -> String {
    let parts: Vec<String> = commits
        .iter()
        .map(|c| extract_why(&c.body))
        .filter(|s| !s.is_empty())
        .collect();
    parts.join("\n\n")
}

fn extract_why(body: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for line in body.lines() {
        if is_footer_line(line) {
            break;
        }
        kept.push(line);
    }
    while matches!(kept.last(), Some(l) if l.trim().is_empty()) {
        kept.pop();
    }
    kept.join("\n")
}

fn is_footer_line(line: &str) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    const ISSUE_PREFIXES: &[&str] = &[
        "closes #",
        "close #",
        "fixes #",
        "fix #",
        "resolves #",
        "resolve #",
        "refs #",
    ];
    if ISSUE_PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return true;
    }
    const TRAILER_PREFIXES: &[&str] = &[
        "co-authored-by:",
        "signed-off-by:",
        "reviewed-by:",
        "acked-by:",
        "tested-by:",
        "reported-by:",
        "suggested-by:",
        "refs:",
    ];
    TRAILER_PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// Synthesize a description from the current branch (`main@origin..@`).
///
/// Falls back to `@`'s description alone when `main@origin` doesn't
/// resolve or the range walk yields nothing — e.g. a freshly-initialised
/// repo without an origin, or a workspace whose `@` doesn't yet have a
/// content diff. Errors only if `@` itself has no description to read.
pub fn for_current_branch() -> Result<Description, String> {
    if let Ok(commits) = jj::commits_between("main@origin", "@") {
        if !commits.is_empty() {
            return Ok(build(&commits).expect("non-empty commits yield a description"));
        }
    }
    let description = jj::current_description().map_err(|e| e.to_string())?;
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return Err("no commits to describe and @ has no description".to_string());
    }
    Ok(from_message(trimmed))
}

/// Build a `Description` from a single commit message, applying the
/// same footer-stripping logic as the multi-commit path. Used as the
/// fallback when `main@origin` doesn't resolve.
fn from_message(message: &str) -> Description {
    let cm = match message.split_once('\n') {
        Some((title, rest)) => CommitMessage {
            title: title.trim().to_string(),
            body: rest.trim_matches('\n').to_string(),
        },
        None => CommitMessage {
            title: message.trim().to_string(),
            body: String::new(),
        },
    };
    Description {
        title: cm.title.clone(),
        body: extract_why(&cm.body),
    }
}

pub fn run(json: bool) {
    let description = match for_current_branch() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    if json {
        match serde_json::to_string_pretty(&description) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} failed to serialize description: {e}");
                std::process::exit(1);
            }
        }
    } else {
        println!("{}", description.title);
        if !description.body.is_empty() {
            println!();
            println!("{}", description.body);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cm(title: &str, body: &str) -> CommitMessage {
        CommitMessage {
            title: title.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn build_single_commit_uses_title_and_body() {
        let commits = vec![cm("feat(shaka): thing", "why this thing\nspans lines")];
        let got = build(&commits).unwrap();
        assert_eq!(got.title, "feat(shaka): thing");
        assert_eq!(got.body, "why this thing\nspans lines");
    }

    #[test]
    fn build_uses_tip_title() {
        let commits = vec![
            cm("feat: a", "why a"),
            cm("test: b", "why b"),
            cm("refactor: c", "why c"),
        ];
        let got = build(&commits).unwrap();
        assert_eq!(got.title, "refactor: c");
        assert_eq!(got.body, "why a\n\nwhy b\n\nwhy c");
    }

    #[test]
    fn build_empty_returns_none() {
        assert!(build(&[]).is_none());
    }

    #[test]
    fn build_omits_commits_with_empty_body() {
        let commits = vec![
            cm("feat: a", "why a"),
            cm("docs: b", ""),
            cm("fix: c", "why c"),
        ];
        let got = build(&commits).unwrap();
        assert_eq!(got.body, "why a\n\nwhy c");
    }

    #[test]
    fn extract_why_strips_closes_footer() {
        let body = "First paragraph.\n\nSecond paragraph.\n\nCloses #67";
        assert_eq!(extract_why(body), "First paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn extract_why_strips_multiple_trailer_lines() {
        let body = "Why.\n\nCloses #1\nCo-authored-by: someone <a@b.c>\nSigned-off-by: x";
        assert_eq!(extract_why(body), "Why.");
    }

    #[test]
    fn extract_why_handles_no_footer() {
        let body = "Just a why\nspread over\nthree lines";
        assert_eq!(extract_why(body), body);
    }

    #[test]
    fn extract_why_empty_body() {
        assert_eq!(extract_why(""), "");
    }

    #[test]
    fn extract_why_keeps_blank_lines_between_paragraphs() {
        let body = "Para 1.\n\nPara 2.";
        assert_eq!(extract_why(body), "Para 1.\n\nPara 2.");
    }

    #[test]
    fn footer_detection_case_insensitive() {
        assert!(is_footer_line("CLOSES #5"));
        assert!(is_footer_line("Closes #5"));
        assert!(is_footer_line("co-authored-by: someone"));
    }

    #[test]
    fn footer_detection_rejects_prose() {
        assert!(!is_footer_line("This closes the loop somehow."));
        assert!(!is_footer_line("issue #5 was the trigger"));
    }

    #[test]
    fn from_message_title_only() {
        let got = from_message("feat: solo");
        assert_eq!(got.title, "feat: solo");
        assert_eq!(got.body, "");
    }

    #[test]
    fn from_message_strips_footers() {
        let got = from_message("feat: x\n\nthe why\n\nCloses #1");
        assert_eq!(got.title, "feat: x");
        assert_eq!(got.body, "the why");
    }
}
