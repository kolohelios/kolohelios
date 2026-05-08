use std::process::Command;

use serde_json::Value;

use crate::gh;
use crate::term::{BOLD, GREEN, RED, RESET};

// Every open issue must carry at least one of these labels. New scope
// labels must be added here AND created in the repo (`gh label create`).
const SCOPE_LABELS: &[&str] = &[
    "ci",
    "claude",
    "infra/devbox",
    "meta",
    "nix",
    "pollen-alert",
    "portfolio",
    "shaka",
];

pub fn run(repo_arg: Option<String>) {
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

    println!("{BOLD}Auditing {repo}{RESET}");
    println!("Scope labels: {}\n", SCOPE_LABELS.join(", "));

    let issues = match list_open_issues(&repo) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    let mut violators: Vec<(u64, String)> = Vec::new();
    for issue in &issues {
        let labels: Vec<&str> = issue["labels"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|l| l["name"].as_str()).collect())
            .unwrap_or_default();
        let has_scope = labels.iter().any(|l| SCOPE_LABELS.contains(l));
        if !has_scope {
            let n = issue["number"].as_u64().unwrap_or(0);
            let title = issue["title"].as_str().unwrap_or("").to_string();
            violators.push((n, title));
        }
    }

    if violators.is_empty() {
        println!(
            "{GREEN}{BOLD}All {} open issue(s) carry a scope label{RESET}",
            issues.len()
        );
        return;
    }

    println!("{BOLD}Open issues missing a scope label:{RESET}");
    for (n, title) in &violators {
        println!("  {RED}#{n}{RESET}  {title}");
    }
    println!(
        "\n{RED}{BOLD}{} issue(s) need attention{RESET}",
        violators.len()
    );
    std::process::exit(1);
}

// `gh issue list` rather than the REST API directly, since the REST API
// mixes PRs into the issues endpoint while the gh CLI filters them out.
fn list_open_issues(repo: &str) -> Result<Vec<Value>, gh::GhError> {
    use snafu::ResultExt;

    let output = Command::new("gh")
        .args([
            "issue",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--limit",
            "1000",
            "--json",
            "number,title,labels",
        ])
        .output()
        .context(gh::SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return gh::GhCommandSnafu {
            command: "gh issue list".to_string(),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&body).context(gh::JsonParseSnafu {
        context: "failed to parse JSON from gh issue list".to_string(),
    })?;

    Ok(parsed.as_array().cloned().unwrap_or_default())
}
