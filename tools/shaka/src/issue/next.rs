use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::gh::{self, IssueSummary, ListState};
use crate::term::{BOLD, DIM, GREEN, RED, RESET};
use crate::workspace;

/// List open issues that are *not* already in flight: no assignee, no
/// open PR that would close them, and no local `i<N>` workspace.
pub fn run(repo_arg: Option<String>, limit: u32, json_out: bool) {
    let repo = match repo_arg {
        Some(r) => r,
        None => match gh::detect_repo() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} {e}");
                eprintln!("Hint: pass --repo owner/repo explicitly");
                std::process::exit(1);
            }
        },
    };

    let issues = match gh::list_issues(&repo, ListState::Open, &[], None, None, limit) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };
    let open_pr_refs = match gh::open_pr_issue_refs(&repo) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };
    // Local-only; degrades to an empty set rather than failing.
    let workspace_issues = workspace::active_issue_numbers();

    let available: Vec<&IssueSummary> = issues
        .iter()
        .filter(|i| is_available(i, &open_pr_refs, &workspace_issues))
        .collect();

    if json_out {
        let payload = build_payload(&repo, &available);
        match serde_json::to_string_pretty(&payload) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} serialize JSON: {e}");
                std::process::exit(1);
            }
        }
    } else {
        print!("{}", render_tree(&repo, &available));
    }
}

/// An issue is "available" when nothing signals it's already being
/// worked: unassigned, no open PR closing it, no local workspace.
fn is_available(
    issue: &IssueSummary,
    open_pr_refs: &BTreeSet<u64>,
    workspace_issues: &BTreeSet<u64>,
) -> bool {
    issue.assignees.is_empty()
        && !open_pr_refs.contains(&issue.number)
        && !workspace_issues.contains(&issue.number)
}

fn render_tree(repo: &str, issues: &[&IssueSummary]) -> String {
    let mut out = String::new();
    out.push_str(&format!("{BOLD}shaka issue next{RESET}\n"));
    out.push_str(&format!(
        "{DIM}├── repo: {repo}  available: {}  (no assignee, no open PR, no workspace){RESET}\n",
        issues.len()
    ));

    if issues.is_empty() {
        out.push_str(&format!("{DIM}└── (nothing available){RESET}\n"));
        return out;
    }

    out.push_str(&format!("{DIM}│{RESET}\n"));
    let n_width = issues
        .iter()
        .map(|i| digit_count(i.number))
        .max()
        .unwrap_or(1);
    for issue in issues {
        let labels_str = if issue.labels.is_empty() {
            "(none)".to_string()
        } else {
            issue.labels.join(", ")
        };
        out.push_str(&format!(
            "├── {BOLD}#{:<width$}{RESET}  {GREEN}OPEN{RESET}  {}\n",
            issue.number,
            issue.title,
            width = n_width,
        ));
        out.push_str(&format!("{DIM}│   labels: {labels_str}{RESET}\n"));
    }
    out.push_str(&format!("{DIM}└── {} available{RESET}\n", issues.len()));
    out
}

fn build_payload(repo: &str, issues: &[&IssueSummary]) -> Value {
    let issues_value: Vec<Value> = issues
        .iter()
        .map(|i| {
            json!({
                "number": i.number,
                "title": i.title,
                "labels": i.labels,
                "url": i.url,
                "createdAt": i.created_at,
                "updatedAt": i.updated_at,
            })
        })
        .collect();
    json!({
        "repo": repo,
        "count": issues.len(),
        "issues": issues_value,
    })
}

fn digit_count(n: u64) -> usize {
    if n == 0 {
        1
    } else {
        let mut c = 0;
        let mut x = n;
        while x > 0 {
            c += 1;
            x /= 10;
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gh::IssueState;

    fn issue(number: u64, assignees: &[&str]) -> IssueSummary {
        IssueSummary {
            number,
            title: format!("issue {number}"),
            state: IssueState::Open,
            labels: vec!["shaka".to_string()],
            url: format!("https://github.com/o/r/issues/{number}"),
            created_at: String::new(),
            updated_at: String::new(),
            assignees: assignees.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn set(nums: &[u64]) -> BTreeSet<u64> {
        nums.iter().copied().collect()
    }

    #[test]
    fn available_when_no_signals() {
        let i = issue(100, &[]);
        assert!(is_available(&i, &set(&[]), &set(&[])));
    }

    #[test]
    fn unavailable_when_assigned() {
        let i = issue(100, &["jedwards"]);
        assert!(!is_available(&i, &set(&[]), &set(&[])));
    }

    #[test]
    fn unavailable_when_open_pr_closes_it() {
        let i = issue(100, &[]);
        assert!(!is_available(&i, &set(&[100]), &set(&[])));
    }

    #[test]
    fn unavailable_when_workspace_exists() {
        let i = issue(100, &[]);
        assert!(!is_available(&i, &set(&[]), &set(&[100])));
    }

    #[test]
    fn payload_lists_only_available_fields() {
        let a = issue(7, &[]);
        let b = issue(9, &[]);
        let refs: Vec<&IssueSummary> = vec![&a, &b];
        let payload = build_payload("o/r", &refs);
        assert_eq!(payload["repo"], "o/r");
        assert_eq!(payload["count"], 2);
        assert_eq!(payload["issues"][0]["number"], 7);
        assert_eq!(payload["issues"][1]["number"], 9);
        // Available issues are unassigned by construction, so the payload
        // doesn't carry an assignees field.
        assert!(payload["issues"][0].get("assignees").is_none());
    }

    #[test]
    fn render_tree_empty_has_nothing_available_branch() {
        let out = render_tree("o/r", &[]);
        assert!(out.contains("shaka issue next"));
        assert!(out.contains("available: 0"));
        assert!(out.contains("(nothing available)"));
    }
}
