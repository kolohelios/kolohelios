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

/// Detect owner/repo from the git remote origin URL.
pub fn detect_repo() -> Result<String, GhError> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()?;

    if !output.status.success() {
        return Err(GhError {
            message: "no git remote 'origin' found".into(),
        });
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
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
