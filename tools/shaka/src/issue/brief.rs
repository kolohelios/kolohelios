use std::process::Command;

use serde_json::{json, Value};

use crate::gh::{self, PrInfo, PrState};
use crate::workspace::{self, WorkspaceRef};

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";

pub fn run(input: &str, no_fetch: bool, json_out: bool) {
    let numbers = match parse_numbers(input) {
        Ok(ns) => ns,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    let fetch_output = if no_fetch {
        None
    } else {
        match jj_git_fetch() {
            Ok(out) => Some(out),
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} jj git fetch: {e}");
                std::process::exit(1);
            }
        }
    };

    let repo = match gh::detect_repo() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    // Fetch the per-issue facts up-front so a failure on any number
    // aborts before partial output. With three numbers and a typo on
    // the third, we'd rather not have printed two real briefs first.
    let mut briefs: Vec<BriefBundle> = Vec::with_capacity(numbers.len());
    for &n in &numbers {
        let issue = match fetch_issue(&repo, n) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} {e}");
                std::process::exit(1);
            }
        };
        let workspace = workspace::find_for_issue(n);
        let pr = match gh::pr_for_issue(&repo, n) {
            Ok(p) => p,
            Err(e) => {
                // PR lookup is best-effort metadata, not load-bearing.
                // A search failure (rate limit, transient API hiccup)
                // shouldn't fail the whole brief — warn and continue
                // with `pr: null`.
                eprintln!("{DIM}warn:{RESET} could not query PR for issue #{n}: {e}");
                None
            }
        };
        briefs.push(BriefBundle {
            number: n,
            issue,
            workspace,
            pr,
        });
    }

    if json_out {
        let bundles: Vec<Value> = briefs
            .iter()
            .map(|b| {
                build_payload(
                    fetch_output.as_deref(),
                    &b.issue,
                    b.workspace.as_ref(),
                    b.pr.as_ref(),
                )
            })
            .collect();
        let payload = json!({ "briefs": bundles });
        match serde_json::to_string_pretty(&payload) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} serialize JSON: {e}");
                std::process::exit(1);
            }
        }
    } else {
        for (i, b) in briefs.iter().enumerate() {
            if i > 0 {
                println!();
            }
            print!(
                "{}",
                render_tree(
                    b.number,
                    fetch_output.as_deref(),
                    &b.issue,
                    b.workspace.as_ref(),
                    b.pr.as_ref()
                )
            );
        }
    }
}

struct BriefBundle {
    number: u64,
    issue: Value,
    workspace: Option<WorkspaceRef>,
    pr: Option<PrInfo>,
}

/// Pure: parse a comma-delimited list of issue numbers. Accepts
/// surrounding whitespace per element (`"345, 427"`); rejects empty
/// elements (`"345,,427"`) and non-numeric tokens.
pub(crate) fn parse_numbers(input: &str) -> Result<Vec<u64>, String> {
    if input.trim().is_empty() {
        return Err("no issue numbers given".into());
    }
    let mut out: Vec<u64> = Vec::new();
    for raw in input.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            return Err(format!("empty element in issue number list: {input:?}"));
        }
        let n: u64 = token
            .parse()
            .map_err(|_| format!("not a positive integer: {token:?}"))?;
        out.push(n);
    }
    Ok(out)
}

fn build_payload(
    fetch: Option<&str>,
    issue: &Value,
    workspace: Option<&WorkspaceRef>,
    pr: Option<&PrInfo>,
) -> Value {
    let workspace_value = workspace
        .map(|w| {
            json!({
                "name": w.name,
                "path": w.path.to_string_lossy(),
            })
        })
        .unwrap_or(Value::Null);
    let pr_value = pr
        .map(|p| {
            json!({
                "number": p.number,
                "url": p.url,
                "state": pr_state_str(&p.state),
            })
        })
        .unwrap_or(Value::Null);
    json!({
        "fetch": fetch,
        "issue": issue,
        "workspace": workspace_value,
        "pr": pr_value,
    })
}

fn pr_state_str(s: &PrState) -> &'static str {
    match s {
        PrState::Open => "OPEN",
        PrState::Closed => "CLOSED",
        PrState::Merged => "MERGED",
    }
}

fn jj_git_fetch() -> Result<String, String> {
    let output = Command::new("jj")
        .args(["git", "fetch"])
        .output()
        .map_err(|e| format!("failed to run jj git fetch: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr.trim().to_string());
    }

    // jj writes the bookmark-update report to stderr; concatenate so we
    // capture it whether jj or future versions choose stdout instead.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(format!("{stdout}{stderr}").trim().to_string())
}

// Pass `--repo` so this works inside a sibling jj workspace where gh's
// implicit walk-up for `.git` would fail. Same fix as pr_create (#221).
fn fetch_issue(repo: &str, n: u64) -> Result<Value, String> {
    let fields = "number,title,state,labels,author,body,url,createdAt,comments";
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &n.to_string(),
            "--repo",
            repo,
            "--json",
            fields,
        ])
        .output()
        .map_err(|e| format!("failed to run gh: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh issue view {n}: {}", stderr.trim()));
    }

    let body = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&body)
        .map_err(|e| format!("failed to parse JSON from gh issue view {n}: {e}"))
}

fn render_tree(
    n: u64,
    fetch: Option<&str>,
    issue: &Value,
    workspace: Option<&WorkspaceRef>,
    pr: Option<&PrInfo>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("{BOLD}shaka issue brief #{n}{RESET}\n"));

    match fetch {
        Some("") => {
            out.push_str(&format!("{DIM}├── fetching from origin{RESET}\n"));
            out.push_str(&format!("{DIM}│   nothing changed{RESET}\n"));
        }
        Some(text) => {
            out.push_str(&format!("{DIM}├── fetching from origin{RESET}\n"));
            for line in text.lines() {
                out.push_str(&format!("{DIM}│   {line}{RESET}\n"));
            }
        }
        None => {
            out.push_str(&format!("{DIM}├── fetching from origin (skipped){RESET}\n"));
        }
    }

    let title = issue["title"].as_str().unwrap_or("");
    let state = issue["state"].as_str().unwrap_or("");
    let labels: Vec<&str> = issue["labels"]
        .as_array()
        .map(|a| a.iter().filter_map(|l| l["name"].as_str()).collect())
        .unwrap_or_default();
    let author = issue["author"]["login"].as_str().unwrap_or("");
    let url = issue["url"].as_str().unwrap_or("");
    let body = issue["body"].as_str().unwrap_or("");
    let empty: Vec<Value> = Vec::new();
    let comments = issue["comments"].as_array().unwrap_or(&empty);

    let state_color = match state {
        "OPEN" => GREEN,
        "CLOSED" => YELLOW,
        _ => RED,
    };
    let labels_str = if labels.is_empty() {
        "(none)".to_string()
    } else {
        labels.join(", ")
    };

    out.push_str(&format!("├── {BOLD}#{n}{RESET} {title}\n"));
    out.push_str(&format!(
        "│   state: {state_color}{state}{RESET}  labels: {labels_str}  author: {author}\n"
    ));
    if !url.is_empty() {
        out.push_str(&format!("│   {DIM}{url}{RESET}\n"));
    }
    if let Some(ws) = workspace {
        out.push_str(&format!(
            "│   workspace: {BOLD}{}{RESET}  {DIM}{}{RESET}\n",
            ws.name,
            ws.path.display()
        ));
    }
    if let Some(pr) = pr {
        let (pr_color, pr_label) = match pr.state {
            PrState::Open => (GREEN, "open"),
            PrState::Merged => (YELLOW, "merged"),
            PrState::Closed => (RED, "closed"),
        };
        out.push_str(&format!(
            "│   pr: {BOLD}#{}{RESET} ({pr_color}{pr_label}{RESET})  {DIM}{}{RESET}\n",
            pr.number, pr.url
        ));
    }
    if !body.trim().is_empty() {
        out.push_str("│\n");
        for line in body.lines() {
            if line.is_empty() {
                out.push_str("│\n");
            } else {
                out.push_str(&format!("│   {line}\n"));
            }
        }
    }

    let count = comments.len();
    out.push_str(&format!("└── comments ({count})\n"));
    for (i, comment) in comments.iter().enumerate() {
        let last = i + 1 == count;
        let head_prefix = if last {
            "    └──"
        } else {
            "    ├──"
        };
        let cont_prefix = if last { "       " } else { "    │  " };
        let cauthor = comment["author"]["login"].as_str().unwrap_or("");
        let created = comment["createdAt"].as_str().unwrap_or("");
        let cbody = comment["body"].as_str().unwrap_or("");
        let date = created.split('T').next().unwrap_or(created);
        out.push_str(&format!("{head_prefix} {BOLD}{cauthor}{RESET} — {date}\n"));
        for line in cbody.lines() {
            if line.is_empty() {
                out.push_str(&format!("{cont_prefix}\n"));
            } else {
                out.push_str(&format!("{cont_prefix} {line}\n"));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_numbers_single() {
        assert_eq!(parse_numbers("586").unwrap(), vec![586]);
    }

    #[test]
    fn parse_numbers_multiple() {
        assert_eq!(parse_numbers("345,427,483").unwrap(), vec![345, 427, 483]);
    }

    #[test]
    fn parse_numbers_tolerates_whitespace() {
        assert_eq!(
            parse_numbers(" 345 , 427, 483 ").unwrap(),
            vec![345, 427, 483]
        );
    }

    #[test]
    fn parse_numbers_rejects_empty_input() {
        let err = parse_numbers("").unwrap_err();
        assert!(err.contains("no issue numbers"), "got: {err}");
    }

    #[test]
    fn parse_numbers_rejects_empty_element() {
        // `586,,427` would otherwise silently drop the empty token —
        // surface it so the user notices the stray comma.
        let err = parse_numbers("586,,427").unwrap_err();
        assert!(err.contains("empty element"), "got: {err}");
    }

    #[test]
    fn parse_numbers_rejects_trailing_comma() {
        let err = parse_numbers("586,").unwrap_err();
        assert!(err.contains("empty element"), "got: {err}");
    }

    #[test]
    fn parse_numbers_rejects_non_numeric() {
        let err = parse_numbers("586,notnum,427").unwrap_err();
        assert!(err.contains("not a positive integer"), "got: {err}");
        assert!(err.contains("notnum"), "got: {err}");
    }

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

    #[test]
    fn renders_issue_with_body_and_no_comments() {
        let issue = json!({
            "number": 216,
            "title": "feat(shaka): issue brief",
            "state": "OPEN",
            "labels": [{"name": "shaka"}],
            "author": {"login": "kolohelios"},
            "body": "First line.\n\nSecond paragraph.",
            "url": "https://github.com/kolohelios/kolohelios/issues/216",
            "createdAt": "2026-05-01T12:00:00Z",
            "comments": [],
        });

        let out = strip_ansi(&render_tree(216, Some(""), &issue, None, None));
        assert!(out.contains("shaka issue brief #216"));
        assert!(out.contains("├── fetching from origin"));
        assert!(out.contains("│   nothing changed"));
        assert!(out.contains("├── #216 feat(shaka): issue brief"));
        assert!(out.contains("state: OPEN"));
        assert!(out.contains("labels: shaka"));
        assert!(out.contains("author: kolohelios"));
        assert!(out.contains("│   First line."));
        assert!(out.contains("│   Second paragraph."));
        assert!(out.contains("└── comments (0)"));
        assert!(!out.contains("workspace:"));
        assert!(!out.contains("pr:"));
    }

    #[test]
    fn renders_skipped_fetch() {
        let issue = json!({
            "number": 1,
            "title": "x",
            "state": "OPEN",
            "labels": [],
            "author": {"login": "u"},
            "body": "",
            "url": "",
            "createdAt": "",
            "comments": [],
        });

        let out = strip_ansi(&render_tree(1, None, &issue, None, None));
        assert!(out.contains("├── fetching from origin (skipped)"));
        assert!(out.contains("labels: (none)"));
    }

    #[test]
    fn renders_comments_with_tree_continuation() {
        let issue = json!({
            "number": 5,
            "title": "t",
            "state": "OPEN",
            "labels": [],
            "author": {"login": "u"},
            "body": "",
            "url": "",
            "createdAt": "",
            "comments": [
                {"author": {"login": "alice"}, "createdAt": "2026-04-01T00:00:00Z", "body": "first"},
                {"author": {"login": "bob"}, "createdAt": "2026-04-02T00:00:00Z", "body": "second\nline"},
            ],
        });

        let out = strip_ansi(&render_tree(5, Some(""), &issue, None, None));
        assert!(out.contains("└── comments (2)"));
        assert!(out.contains("    ├── alice — 2026-04-01"));
        assert!(out.contains("    │   first"));
        assert!(out.contains("    └── bob — 2026-04-02"));
        assert!(out.contains("        second"));
        assert!(out.contains("        line"));
    }

    fn minimal_issue() -> Value {
        json!({
            "number": 231,
            "title": "t",
            "state": "OPEN",
            "labels": [],
            "author": {"login": "u"},
            "body": "",
            "url": "",
            "createdAt": "",
            "comments": [],
        })
    }

    #[test]
    fn renders_workspace_and_pr_when_present() {
        let issue = minimal_issue();
        let ws = WorkspaceRef {
            name: "i231".to_string(),
            path: std::path::PathBuf::from("/Users/me/code/kolohelios-i231"),
        };
        let pr = PrInfo {
            number: 234,
            url: "https://github.com/o/r/pull/234".to_string(),
            state: PrState::Open,
        };
        let out = strip_ansi(&render_tree(231, Some(""), &issue, Some(&ws), Some(&pr)));
        assert!(out.contains("│   workspace: i231  /Users/me/code/kolohelios-i231"));
        assert!(out.contains("│   pr: #234 (open)  https://github.com/o/r/pull/234"));
    }

    #[test]
    fn renders_pr_states_with_labels() {
        let issue = minimal_issue();
        for (state, label) in [
            (PrState::Open, "open"),
            (PrState::Merged, "merged"),
            (PrState::Closed, "closed"),
        ] {
            let pr = PrInfo {
                number: 1,
                url: "u".to_string(),
                state,
            };
            let out = strip_ansi(&render_tree(1, Some(""), &issue, None, Some(&pr)));
            assert!(
                out.contains(&format!("pr: #1 ({label})")),
                "missing label {label} in: {out}"
            );
        }
    }

    #[test]
    fn payload_includes_nullable_workspace_and_pr() {
        let issue = minimal_issue();
        let payload = build_payload(Some(""), &issue, None, None);
        assert!(payload.get("workspace").unwrap().is_null());
        assert!(payload.get("pr").unwrap().is_null());
        assert_eq!(payload["fetch"], json!(""));
        assert_eq!(payload["issue"]["number"], 231);
    }

    #[test]
    fn payload_populates_workspace_and_pr() {
        let issue = minimal_issue();
        let ws = WorkspaceRef {
            name: "i231".to_string(),
            path: std::path::PathBuf::from("/tmp/kolohelios-i231"),
        };
        let pr = PrInfo {
            number: 234,
            url: "https://github.com/o/r/pull/234".to_string(),
            state: PrState::Merged,
        };
        let payload = build_payload(None, &issue, Some(&ws), Some(&pr));
        assert_eq!(payload["workspace"]["name"], "i231");
        assert_eq!(payload["workspace"]["path"], "/tmp/kolohelios-i231");
        assert_eq!(payload["pr"]["number"], 234);
        assert_eq!(payload["pr"]["state"], "MERGED");
        assert_eq!(payload["pr"]["url"], "https://github.com/o/r/pull/234");
    }
}
