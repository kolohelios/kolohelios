use std::process::Command;

use serde_json::{json, Value};

use crate::gh;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";

pub fn run(n: u64, no_fetch: bool, json_out: bool) {
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

    let issue = match fetch_issue(&repo, n) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    if json_out {
        let payload = json!({
            "fetch": fetch_output,
            "issue": issue,
        });
        match serde_json::to_string_pretty(&payload) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} serialize JSON: {e}");
                std::process::exit(1);
            }
        }
    } else {
        print!("{}", render_tree(n, fetch_output.as_deref(), &issue));
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

fn render_tree(n: u64, fetch: Option<&str>, issue: &Value) -> String {
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

        let out = strip_ansi(&render_tree(216, Some(""), &issue));
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

        let out = strip_ansi(&render_tree(1, None, &issue));
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

        let out = strip_ansi(&render_tree(5, Some(""), &issue));
        assert!(out.contains("└── comments (2)"));
        assert!(out.contains("    ├── alice — 2026-04-01"));
        assert!(out.contains("    │   first"));
        assert!(out.contains("    └── bob — 2026-04-02"));
        assert!(out.contains("        second"));
        assert!(out.contains("        line"));
    }
}
