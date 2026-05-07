use serde_json::{json, Value};

use crate::gh::{self, IssueState, IssueSummary, ListState};
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

pub fn run(
    repo_arg: Option<String>,
    state: ListState,
    labels: Vec<String>,
    milestone: Option<String>,
    limit: u32,
    json_out: bool,
) {
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

    let issues = match gh::list_issues(&repo, state, &labels, milestone.as_deref(), limit) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    if json_out {
        let payload = build_payload(&repo, state, &labels, milestone.as_deref(), &issues);
        match serde_json::to_string_pretty(&payload) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} serialize JSON: {e}");
                std::process::exit(1);
            }
        }
    } else {
        print!("{}", render_tree(&repo, state, &issues));
    }
}

fn render_tree(repo: &str, state: ListState, issues: &[IssueSummary]) -> String {
    let mut out = String::new();
    out.push_str(&format!("{BOLD}shaka issue list{RESET}\n"));
    out.push_str(&format!(
        "{DIM}├── repo: {repo}  state: {}  count: {}{RESET}\n",
        state_arg_str(state),
        issues.len()
    ));

    if issues.is_empty() {
        out.push_str(&format!("{DIM}└── (no issues){RESET}\n"));
        return out;
    }

    out.push_str(&format!("{DIM}│{RESET}\n"));
    let n_width = issues
        .iter()
        .map(|i| digit_count(i.number))
        .max()
        .unwrap_or(1);
    for issue in issues {
        let state_color = match issue.state {
            IssueState::Open => GREEN,
            IssueState::Closed => YELLOW,
        };
        let labels_str = if issue.labels.is_empty() {
            "(none)".to_string()
        } else {
            issue.labels.join(", ")
        };
        out.push_str(&format!(
            "├── {BOLD}#{:<width$}{RESET}  {state_color}{}{RESET}  {}\n",
            issue.number,
            issue.state.as_str(),
            issue.title,
            width = n_width,
        ));
        out.push_str(&format!("{DIM}│   labels: {labels_str}{RESET}\n"));
    }
    out.push_str(&format!("{DIM}└── {} issue(s){RESET}\n", issues.len()));
    out
}

fn build_payload(
    repo: &str,
    state: ListState,
    labels: &[String],
    milestone: Option<&str>,
    issues: &[IssueSummary],
) -> Value {
    let issues_value: Vec<Value> = issues
        .iter()
        .map(|i| {
            json!({
                "number": i.number,
                "title": i.title,
                "state": i.state.as_str(),
                "labels": i.labels,
                "url": i.url,
                "createdAt": i.created_at,
                "updatedAt": i.updated_at,
            })
        })
        .collect();
    json!({
        "repo": repo,
        "filters": {
            "state": state_arg_str(state),
            "labels": labels,
            "milestone": milestone,
        },
        "count": issues.len(),
        "issues": issues_value,
    })
}

fn state_arg_str(state: ListState) -> &'static str {
    match state {
        ListState::Open => "open",
        ListState::Closed => "closed",
        ListState::All => "all",
    }
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

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' && chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn issue(number: u64, state: IssueState, labels: &[&str], title: &str) -> IssueSummary {
        IssueSummary {
            number,
            title: title.to_string(),
            state,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            url: format!("https://github.com/o/r/issues/{number}"),
            created_at: "2026-05-06T00:00:00Z".to_string(),
            updated_at: "2026-05-06T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn renders_empty_list_with_summary_branch() {
        let out = strip_ansi(&render_tree("o/r", ListState::Open, &[]));
        assert!(out.contains("shaka issue list"));
        assert!(out.contains("├── repo: o/r  state: open  count: 0"));
        assert!(out.contains("└── (no issues)"));
    }

    #[test]
    fn renders_issues_with_aligned_numbers_and_labels() {
        let issues = vec![
            issue(15, IssueState::Open, &["shaka"], "umbrella"),
            issue(244, IssueState::Open, &["shaka"], "list slice"),
            issue(209, IssueState::Closed, &[], "audit slice"),
        ];
        let out = strip_ansi(&render_tree("o/r", ListState::All, &issues));
        assert!(out.contains("├── repo: o/r  state: all  count: 3"));
        // numbers right-padded to width 3 (longest is "244")
        assert!(out.contains("├── #15   OPEN  umbrella"));
        assert!(out.contains("├── #244  OPEN  list slice"));
        assert!(out.contains("├── #209  CLOSED  audit slice"));
        assert!(out.contains("│   labels: shaka"));
        assert!(out.contains("│   labels: (none)"));
        assert!(out.contains("└── 3 issue(s)"));
    }

    #[test]
    fn payload_shape_is_normalized() {
        let issues = vec![issue(1, IssueState::Open, &["shaka"], "t")];
        let payload = build_payload(
            "o/r",
            ListState::Open,
            &["shaka".to_string()],
            Some("v1"),
            &issues,
        );
        assert_eq!(payload["repo"], "o/r");
        assert_eq!(payload["filters"]["state"], "open");
        assert_eq!(payload["filters"]["labels"], json!(["shaka"]));
        assert_eq!(payload["filters"]["milestone"], "v1");
        assert_eq!(payload["count"], 1);
        assert_eq!(payload["issues"][0]["number"], 1);
        assert_eq!(payload["issues"][0]["state"], "OPEN");
        assert_eq!(payload["issues"][0]["labels"], json!(["shaka"]));
        assert_eq!(
            payload["issues"][0]["url"],
            "https://github.com/o/r/issues/1"
        );
    }

    #[test]
    fn payload_milestone_null_when_absent() {
        let payload = build_payload("o/r", ListState::Open, &[], None, &[]);
        assert!(payload["filters"]["milestone"].is_null());
        assert_eq!(payload["filters"]["labels"], json!([]));
        assert_eq!(payload["count"], 0);
    }

    #[test]
    fn digit_count_sanity() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(9), 1);
        assert_eq!(digit_count(10), 2);
        assert_eq!(digit_count(999), 3);
        assert_eq!(digit_count(1000), 4);
    }
}
