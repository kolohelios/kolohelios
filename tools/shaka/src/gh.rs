use std::fmt;
use std::process::Command;

use serde_json::Value;

#[derive(Debug)]
pub struct GhError {
    pub message: String,
}

impl fmt::Display for GhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<std::io::Error> for GhError {
    fn from(e: std::io::Error) -> Self {
        GhError {
            message: format!("failed to run command: {e}"),
        }
    }
}

/// Run `gh api <endpoint>` and return parsed JSON.
pub fn api_get(endpoint: &str) -> Result<Value, GhError> {
    let output = Command::new("gh")
        .args(["api", endpoint, "-H", "Accept: application/vnd.github+json"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError {
            message: format!("gh api {endpoint}: {stderr}"),
        });
    }

    let body = String::from_utf8_lossy(&output.stdout);
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(&body).map_err(|e| GhError {
        message: format!("failed to parse JSON from gh api {endpoint}: {e}"),
    })
}

/// Run `gh api <endpoint>` and return the HTTP status code.
/// Used for endpoints like vulnerability-alerts that signal via status code.
pub fn api_get_status(endpoint: &str) -> Result<i32, GhError> {
    let output = Command::new("gh")
        .args([
            "api",
            endpoint,
            "-H",
            "Accept: application/vnd.github+json",
            "--include",
        ])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // First line is the HTTP status line, e.g. "HTTP/2.0 204 No Content"
    if let Some(status_line) = stdout.lines().next() {
        let parts: Vec<&str> = status_line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(code) = parts[1].parse::<i32>() {
                return Ok(code);
            }
        }
    }

    Err(GhError {
        message: format!("could not parse status from gh api {endpoint}"),
    })
}

/// Run `gh api -X PATCH <endpoint>` with a JSON body on stdin.
pub fn api_patch(endpoint: &str, body: &Value) -> Result<Value, GhError> {
    api_write("PATCH", endpoint, body)
}

/// Run `gh api -X PUT <endpoint>` with a JSON body on stdin.
pub fn api_put(endpoint: &str, body: &Value) -> Result<Value, GhError> {
    api_write("PUT", endpoint, body)
}

fn api_write(method: &str, endpoint: &str, body: &Value) -> Result<Value, GhError> {
    let body_str = serde_json::to_string(body).map_err(|e| GhError {
        message: format!("failed to serialize JSON: {e}"),
    })?;

    let output = Command::new("gh")
        .args([
            "api",
            "-X",
            method,
            endpoint,
            "-H",
            "Accept: application/vnd.github+json",
            "--input",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(body_str.as_bytes())?;
            }
            child.wait_with_output()
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError {
            message: format!("gh api -X {method} {endpoint}: {stderr}"),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(&stdout).map_err(|e| GhError {
        message: format!("failed to parse JSON: {e}"),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrInfo {
    pub number: u64,
    pub url: String,
    pub state: PrState,
}

/// Find the PR whose head branch matches `head`, checking all states (open,
/// closed, merged). Returns `None` if no PR exists. `repo` is in `owner/repo`
/// form.
pub fn pr_for_head(repo: &str, head: &str) -> Result<Option<PrInfo>, GhError> {
    let owner = repo.split('/').next().unwrap_or("");
    // Query all states so we can detect merged PRs.
    let endpoint = format!("/repos/{repo}/pulls?head={owner}:{head}&state=all");
    let result = api_get(&endpoint)?;
    let Some(first) = result.as_array().and_then(|arr| arr.first()) else {
        return Ok(None);
    };
    let number = first["number"].as_u64().ok_or_else(|| GhError {
        message: format!("PR for head {head} missing 'number' field"),
    })?;
    let url = first["html_url"]
        .as_str()
        .ok_or_else(|| GhError {
            message: format!("PR for head {head} missing 'html_url' field"),
        })?
        .to_string();
    // A closed PR with a merge_commit_sha is merged; otherwise it's closed.
    let state = match first["state"].as_str() {
        Some("open") => PrState::Open,
        Some("closed") => {
            if first["merged_at"].is_string() {
                PrState::Merged
            } else {
                PrState::Closed
            }
        }
        _ => PrState::Closed,
    };
    Ok(Some(PrInfo { number, url, state }))
}

/// Run `gh pr create` and return the PR URL.
pub fn pr_create(title: &str, body: &str, head: &str) -> Result<String, GhError> {
    let output = Command::new("gh")
        .args([
            "pr", "create", "--title", title, "--body", body, "--head", head,
        ])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError {
            message: format!("gh pr create: {}", stderr.trim()),
        });
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(url)
}

/// Fetch the title of a GitHub issue by number.
///
/// Shells out to `gh issue view <n> --json title --jq .title`.
/// Returns an error if `gh` is not authenticated or the issue does not exist.
pub fn issue_title(n: u64) -> Result<String, GhError> {
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &n.to_string(),
            "--json",
            "title",
            "--jq",
            ".title",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError {
            message: format!("gh issue view {n}: {}", stderr.trim()),
        });
    }

    let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(title)
}

/// Detect owner/repo from the jj git remote named "origin".
///
/// Uses `jj git remote list` rather than `git remote get-url` so that this
/// works from inside a non-default jj workspace, which has no `.git` of its
/// own — it only has a `.jj` directory that links back to the shared repo.
pub fn detect_repo() -> Result<String, GhError> {
    let output = Command::new("jj")
        .args(["git", "remote", "list"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError {
            message: format!("jj git remote list: {}", stderr.trim()),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Each line is "<name> <url>"; find the one named "origin".
    let url = stdout
        .lines()
        .find_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let name = parts.next()?;
            let url = parts.next()?.trim();
            if name == "origin" {
                Some(url.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| GhError {
            message: "no jj git remote named 'origin' found".into(),
        })?;

    parse_repo_from_url(&url).ok_or_else(|| GhError {
        message: format!("could not parse owner/repo from remote URL: {url}"),
    })
}

fn parse_repo_from_url(url: &str) -> Option<String> {
    let path = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest
    } else if url.contains("github.com/") {
        url.split("github.com/").nth(1)?
    } else {
        return None;
    };

    let path = path.strip_suffix(".git").unwrap_or(path);
    let parts: Vec<&str> = path.splitn(3, '/').collect();
    if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_https_url() {
        assert_eq!(
            parse_repo_from_url("https://github.com/owner/repo.git"),
            Some("owner/repo".into())
        );
    }

    #[test]
    fn parse_ssh_url() {
        assert_eq!(
            parse_repo_from_url("git@github.com:owner/repo.git"),
            Some("owner/repo".into())
        );
    }

    #[test]
    fn parse_https_no_dotgit() {
        assert_eq!(
            parse_repo_from_url("https://github.com/owner/repo"),
            Some("owner/repo".into())
        );
    }
}
