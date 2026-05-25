use std::process::Command;

use serde_json::Value;
use snafu::{OptionExt, ResultExt, Snafu};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum GhError {
    #[snafu(display("failed to run command: {source}"))]
    Spawn { source: std::io::Error },

    #[snafu(display("{command}: {stderr}"))]
    GhCommand { command: String, stderr: String },

    #[snafu(display("{context}: {source}"))]
    JsonParse {
        context: String,
        source: serde_json::Error,
    },

    #[snafu(display("failed to serialize JSON: {source}"))]
    JsonSerialize { source: serde_json::Error },

    #[snafu(display("{message}"))]
    Schema { message: String },
}

/// Run `gh api <endpoint>` and return parsed JSON.
pub fn api_get(endpoint: &str) -> Result<Value, GhError> {
    let output = Command::new("gh")
        .args(["api", endpoint, "-H", "Accept: application/vnd.github+json"])
        .output()
        .context(SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh api {endpoint}"),
            stderr: stderr.to_string(),
        }
        .fail();
    }

    let body = String::from_utf8_lossy(&output.stdout);
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(&body).context(JsonParseSnafu {
        context: format!("failed to parse JSON from gh api {endpoint}"),
    })
}

/// Fetch a file's raw contents from a repo via the GitHub contents
/// API. Uses `Accept: application/vnd.github.raw` so the response
/// body is the file itself rather than the base64-wrapped JSON the
/// default content type returns. Returns `Ok(None)` on 404 so callers
/// can surface "policy missing" cleanly; other errors propagate.
pub fn fetch_raw_file(repo: &str, path: &str) -> Result<Option<String>, GhError> {
    let endpoint = format!("repos/{repo}/contents/{path}");
    let output = Command::new("gh")
        .args(["api", &endpoint, "-H", "Accept: application/vnd.github.raw"])
        .output()
        .context(SpawnSnafu)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("HTTP 404") || stderr.contains("Not Found") {
            return Ok(None);
        }
        return GhCommandSnafu {
            command: format!("gh api {endpoint}"),
            stderr: stderr.to_string(),
        }
        .fail();
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
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
        .output()
        .context(SpawnSnafu)?;

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

    SchemaSnafu {
        message: format!("could not parse status from gh api {endpoint}"),
    }
    .fail()
}

/// Run `gh api -X PATCH <endpoint>` with a JSON body on stdin.
pub fn api_patch(endpoint: &str, body: &Value) -> Result<Value, GhError> {
    api_write("PATCH", endpoint, body)
}

/// Run `gh api -X POST <endpoint>` with a JSON body on stdin.
pub fn api_post(endpoint: &str, body: &Value) -> Result<Value, GhError> {
    api_write("POST", endpoint, body)
}

/// Run `gh api -X PUT <endpoint>` with a JSON body on stdin.
pub fn api_put(endpoint: &str, body: &Value) -> Result<Value, GhError> {
    api_write("PUT", endpoint, body)
}

fn api_write(method: &str, endpoint: &str, body: &Value) -> Result<Value, GhError> {
    let body_str = serde_json::to_string(body).context(JsonSerializeSnafu)?;

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
        })
        .context(SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh api -X {method} {endpoint}"),
            stderr: stderr.to_string(),
        }
        .fail();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(&stdout).context(JsonParseSnafu {
        context: "failed to parse JSON".to_string(),
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
    let number = first["number"].as_u64().with_context(|| SchemaSnafu {
        message: format!("PR for head {head} missing 'number' field"),
    })?;
    let url = first["html_url"]
        .as_str()
        .with_context(|| SchemaSnafu {
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

/// Find the merged PR that closed the given issue, if any.
///
/// Uses `gh issue view N --json state,closedByPullRequestsReferences`. GitHub
/// only populates `closedByPullRequestsReferences` for PRs that were merged
/// with an autoclose keyword (`Closes #N`, `Fixes #N`, …) — exactly the
/// signal we want for `shaka workspace cleanup` to identify a workspace whose
/// work has landed, even after `repo sync` has deleted the local bookmark.
///
/// Returns `Ok(None)` if the issue is open, has no closing PR reference, or
/// the issue does not exist. Returns the first referenced PR if multiple.
pub fn merged_pr_for_issue(n: u64) -> Result<Option<PrInfo>, GhError> {
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &n.to_string(),
            "--json",
            "state,closedByPullRequestsReferences",
        ])
        .output()
        .context(SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh issue view {n}"),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }

    let body = String::from_utf8_lossy(&output.stdout);
    if body.trim().is_empty() {
        return Ok(None);
    }
    let parsed: Value = serde_json::from_str(&body).context(JsonParseSnafu {
        context: format!("failed to parse JSON from gh issue view {n}"),
    })?;

    if parsed["state"].as_str() != Some("CLOSED") {
        return Ok(None);
    }

    let Some(first) = parsed["closedByPullRequestsReferences"]
        .as_array()
        .and_then(|arr| arr.first())
    else {
        return Ok(None);
    };

    let number = first["number"].as_u64().with_context(|| SchemaSnafu {
        message: format!("closing PR for issue #{n} missing 'number' field"),
    })?;
    let url = first["url"]
        .as_str()
        .with_context(|| SchemaSnafu {
            message: format!("closing PR for issue #{n} missing 'url' field"),
        })?
        .to_string();
    Ok(Some(PrInfo {
        number,
        url,
        state: PrState::Merged,
    }))
}

/// Find a PR (open, closed, or merged) whose body references `Closes #n`.
/// Returns the first match (most recently updated by gh's default sort).
///
/// Relies on the repo convention of `Closes #N` in PR bodies (CLAUDE.md);
/// `Fixes`/`Resolves` aren't matched. For merged PRs that closed the issue
/// you can also use [`merged_pr_for_issue`] — this helper additionally
/// catches *open* PRs, which `closedByPullRequestsReferences` does not.
pub fn pr_for_issue(repo: &str, n: u64) -> Result<Option<PrInfo>, GhError> {
    let query = format!("in:body Closes #{n}");
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            repo,
            "--search",
            &query,
            "--state",
            "all",
            "--limit",
            "1",
            "--json",
            "number,state,url",
        ])
        .output()
        .context(SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh pr list (search Closes #{n})"),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }

    let body = String::from_utf8_lossy(&output.stdout);
    parse_pr_for_issue(&body)
}

pub(crate) fn parse_pr_for_issue(body: &str) -> Result<Option<PrInfo>, GhError> {
    let value: Value = serde_json::from_str(body).context(JsonParseSnafu {
        context: "failed to parse gh pr list JSON".to_string(),
    })?;
    let Some(first) = value.as_array().and_then(|a| a.first()) else {
        return Ok(None);
    };
    let number = first["number"].as_u64().context(SchemaSnafu {
        message: "PR missing 'number' field",
    })?;
    let url = first["url"]
        .as_str()
        .with_context(|| SchemaSnafu {
            message: format!("PR #{number} missing 'url' field"),
        })?
        .to_string();
    // gh pr list reports state as upper-case OPEN/CLOSED/MERGED (unlike the
    // REST API which uses lower-case open/closed plus a separate merged_at).
    let state = match first["state"].as_str() {
        Some("OPEN") => PrState::Open,
        Some("MERGED") => PrState::Merged,
        Some("CLOSED") => PrState::Closed,
        other => {
            return SchemaSnafu {
                message: format!("PR #{number} has unexpected state {other:?}"),
            }
            .fail();
        }
    };
    Ok(Some(PrInfo { number, url, state }))
}

/// Run `gh pr create` and return the PR URL.
///
/// Passes `--repo` so this works inside a sibling jj workspace where
/// `gh`'s implicit walk-up for `.git` would fail. See issue #221.
pub fn pr_create(repo: &str, title: &str, body: &str, head: &str) -> Result<String, GhError> {
    let output = Command::new("gh")
        .args([
            "pr", "create", "--repo", repo, "--title", title, "--body", body, "--head", head,
        ])
        .output()
        .context(SpawnSnafu)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: "gh pr create".to_string(),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(url)
}

/// Enable GitHub auto-merge on a PR with the rebase strategy.
///
/// Calls `gh pr merge --auto --rebase <pr-url>`. The PR will merge as soon
/// as the repo's required checks pass. Idempotent — safe to call on a PR
/// that already has auto-merge enabled.
///
/// Fails if the target repo has `allow_auto_merge: false`. The caller is
/// expected to enforce that via repo policy (`shaka repo audit`).
pub fn pr_merge_auto_rebase(pr_url: &str) -> Result<(), GhError> {
    let output = Command::new("gh")
        .args(["pr", "merge", "--auto", "--rebase", pr_url])
        .output()
        .context(SpawnSnafu)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh pr merge --auto --rebase {pr_url}"),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }
    Ok(())
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
        .output()
        .context(SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh issue view {n}"),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }

    let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(title)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPr {
    pub number: u64,
    pub head_ref: String,
    pub head_sha: String,
    pub url: String,
    pub labels: Vec<String>,
}

/// List open PRs whose base is `base`, with the fields needed for rebasing.
///
/// Shells out to `gh pr list --base <base> --state open --json
/// number,headRefName,headRefOid,url,labels`.
pub fn list_open_prs_against(base: &str) -> Result<Vec<OpenPr>, GhError> {
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--base",
            base,
            "--state",
            "open",
            "--json",
            "number,headRefName,headRefOid,url,labels",
        ])
        .output()
        .context(SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: "gh pr list".to_string(),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }

    let body = String::from_utf8_lossy(&output.stdout);
    parse_open_prs(&body)
}

pub(crate) fn parse_open_prs(body: &str) -> Result<Vec<OpenPr>, GhError> {
    let value: Value = serde_json::from_str(body).context(JsonParseSnafu {
        context: "failed to parse gh pr list JSON".to_string(),
    })?;
    let arr = value.as_array().context(SchemaSnafu {
        message: "gh pr list did not return an array",
    })?;

    arr.iter()
        .map(|v| {
            let number = v["number"].as_u64().context(SchemaSnafu {
                message: "PR missing 'number'",
            })?;
            let head_ref = v["headRefName"]
                .as_str()
                .with_context(|| SchemaSnafu {
                    message: format!("PR #{number} missing 'headRefName'"),
                })?
                .to_string();
            let head_sha = v["headRefOid"]
                .as_str()
                .with_context(|| SchemaSnafu {
                    message: format!("PR #{number} missing 'headRefOid'"),
                })?
                .to_string();
            let url = v["url"]
                .as_str()
                .with_context(|| SchemaSnafu {
                    message: format!("PR #{number} missing 'url'"),
                })?
                .to_string();
            let labels = v["labels"]
                .as_array()
                .map(|labels| {
                    labels
                        .iter()
                        .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            Ok(OpenPr {
                number,
                head_ref,
                head_sha,
                url,
                labels,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListState {
    Open,
    Closed,
    All,
}

impl ListState {
    fn as_arg(self) -> &'static str {
        match self {
            ListState::Open => "open",
            ListState::Closed => "closed",
            ListState::All => "all",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Closed,
}

impl IssueState {
    pub fn as_str(&self) -> &'static str {
        match self {
            IssueState::Open => "OPEN",
            IssueState::Closed => "CLOSED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueSummary {
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub labels: Vec<String>,
    pub url: String,
    pub created_at: String,
    pub updated_at: String,
}

/// List GitHub issues from `repo` matching the given filters.
///
/// Shells out to `gh issue list --json number,title,state,labels,url,createdAt,updatedAt`.
/// `labels` are AND-combined when more than one is supplied.
pub fn list_issues(
    repo: &str,
    state: ListState,
    labels: &[String],
    milestone: Option<&str>,
    limit: u32,
) -> Result<Vec<IssueSummary>, GhError> {
    let limit = limit.to_string();
    let mut args: Vec<String> = vec![
        "issue".into(),
        "list".into(),
        "--repo".into(),
        repo.into(),
        "--state".into(),
        state.as_arg().into(),
        "--limit".into(),
        limit,
        "--json".into(),
        "number,title,state,labels,url,createdAt,updatedAt".into(),
    ];
    for label in labels {
        args.push("--label".into());
        args.push(label.clone());
    }
    if let Some(m) = milestone {
        args.push("--milestone".into());
        args.push(m.to_string());
    }

    let output = Command::new("gh")
        .args(&args)
        .output()
        .context(SpawnSnafu)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: "gh issue list".to_string(),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }

    let body = String::from_utf8_lossy(&output.stdout);
    parse_issue_list(&body)
}

pub(crate) fn parse_issue_list(body: &str) -> Result<Vec<IssueSummary>, GhError> {
    let value: Value = serde_json::from_str(body).context(JsonParseSnafu {
        context: "failed to parse gh issue list JSON".to_string(),
    })?;
    let arr = value.as_array().context(SchemaSnafu {
        message: "gh issue list did not return an array",
    })?;

    arr.iter()
        .map(|v| {
            let number = v["number"].as_u64().context(SchemaSnafu {
                message: "issue missing 'number'",
            })?;
            let title = v["title"]
                .as_str()
                .with_context(|| SchemaSnafu {
                    message: format!("issue #{number} missing 'title'"),
                })?
                .to_string();
            let state = match v["state"].as_str() {
                Some("OPEN") => IssueState::Open,
                Some("CLOSED") => IssueState::Closed,
                other => {
                    return SchemaSnafu {
                        message: format!("issue #{number} has unexpected state {other:?}"),
                    }
                    .fail();
                }
            };
            let labels = v["labels"]
                .as_array()
                .map(|labels| {
                    labels
                        .iter()
                        .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let url = v["url"]
                .as_str()
                .with_context(|| SchemaSnafu {
                    message: format!("issue #{number} missing 'url'"),
                })?
                .to_string();
            let created_at = v["createdAt"].as_str().unwrap_or("").to_string();
            let updated_at = v["updatedAt"].as_str().unwrap_or("").to_string();
            Ok(IssueSummary {
                number,
                title,
                state,
                labels,
                url,
                created_at,
                updated_at,
            })
        })
        .collect()
}

/// Detect owner/repo. Prefers `$GITHUB_REPOSITORY` (set in GitHub Actions),
/// falling back to [`detect_repo`] for local invocations.
pub fn detect_repo_or_env() -> Result<String, GhError> {
    if let Ok(r) = std::env::var("GITHUB_REPOSITORY") {
        if !r.is_empty() {
            return Ok(r);
        }
    }
    detect_repo()
}

/// Detect owner/repo from the jj git remote named "origin".
///
/// Uses `jj git remote list` rather than `git remote get-url` so that this
/// works from inside a non-default jj workspace, which has no `.git` of its
/// own — it only has a `.jj` directory that links back to the shared repo.
pub fn detect_repo() -> Result<String, GhError> {
    let output = Command::new("jj")
        .args(["git", "remote", "list"])
        .output()
        .context(SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: "jj git remote list".to_string(),
            stderr: stderr.trim().to_string(),
        }
        .fail();
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
        .context(SchemaSnafu {
            message: "no jj git remote named 'origin' found",
        })?;

    parse_repo_from_url(&url).with_context(|| SchemaSnafu {
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

    #[test]
    fn parse_open_prs_extracts_fields() {
        let body = r#"[
            {
                "number": 12,
                "headRefName": "feat/x",
                "headRefOid": "abc123",
                "url": "https://github.com/o/r/pull/12",
                "labels": [{"name": "ci"}, {"name": "do-not-rebase"}]
            },
            {
                "number": 13,
                "headRefName": "fix/y",
                "headRefOid": "def456",
                "url": "https://github.com/o/r/pull/13",
                "labels": []
            }
        ]"#;
        let prs = parse_open_prs(body).unwrap();
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].number, 12);
        assert_eq!(prs[0].head_ref, "feat/x");
        assert_eq!(prs[0].head_sha, "abc123");
        assert_eq!(prs[0].labels, vec!["ci", "do-not-rebase"]);
        assert_eq!(prs[1].labels, Vec::<String>::new());
    }

    #[test]
    fn parse_open_prs_empty() {
        assert!(parse_open_prs("[]").unwrap().is_empty());
    }

    #[test]
    fn parse_open_prs_missing_field_errors() {
        let body = r#"[{"number": 1, "headRefName": "x", "url": "u"}]"#;
        assert!(parse_open_prs(body).is_err());
    }

    #[test]
    fn parse_pr_for_issue_open() {
        let body = r#"[{"number": 42, "state": "OPEN", "url": "https://gh.com/o/r/pull/42"}]"#;
        let pr = parse_pr_for_issue(body).unwrap().unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.state, PrState::Open);
        assert_eq!(pr.url, "https://gh.com/o/r/pull/42");
    }

    #[test]
    fn parse_pr_for_issue_merged() {
        let body = r#"[{"number": 7, "state": "MERGED", "url": "u"}]"#;
        assert_eq!(
            parse_pr_for_issue(body).unwrap().unwrap().state,
            PrState::Merged
        );
    }

    #[test]
    fn parse_pr_for_issue_closed() {
        let body = r#"[{"number": 7, "state": "CLOSED", "url": "u"}]"#;
        assert_eq!(
            parse_pr_for_issue(body).unwrap().unwrap().state,
            PrState::Closed
        );
    }

    #[test]
    fn parse_pr_for_issue_empty() {
        assert!(parse_pr_for_issue("[]").unwrap().is_none());
    }

    #[test]
    fn parse_pr_for_issue_unknown_state_errors() {
        let body = r#"[{"number": 1, "state": "WAT", "url": "u"}]"#;
        assert!(parse_pr_for_issue(body).is_err());
    }

    #[test]
    fn parse_issue_list_extracts_fields() {
        let body = r#"[
            {
                "number": 244,
                "title": "feat(shaka): add issue list",
                "state": "OPEN",
                "labels": [{"name": "shaka"}, {"name": "good first issue"}],
                "url": "https://github.com/o/r/issues/244",
                "createdAt": "2026-05-06T00:00:00Z",
                "updatedAt": "2026-05-06T01:00:00Z"
            },
            {
                "number": 209,
                "title": "feat(shaka): add issue audit",
                "state": "CLOSED",
                "labels": [],
                "url": "https://github.com/o/r/issues/209",
                "createdAt": "2026-05-01T00:00:00Z",
                "updatedAt": "2026-05-03T00:00:00Z"
            }
        ]"#;
        let issues = parse_issue_list(body).unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].number, 244);
        assert_eq!(issues[0].state, IssueState::Open);
        assert_eq!(issues[0].labels, vec!["shaka", "good first issue"]);
        assert_eq!(issues[0].url, "https://github.com/o/r/issues/244");
        assert_eq!(issues[1].state, IssueState::Closed);
        assert_eq!(issues[1].labels, Vec::<String>::new());
    }

    #[test]
    fn parse_issue_list_empty() {
        assert!(parse_issue_list("[]").unwrap().is_empty());
    }

    #[test]
    fn parse_issue_list_missing_field_errors() {
        let body = r#"[{"number": 1, "state": "OPEN", "labels": [], "url": "u"}]"#;
        assert!(parse_issue_list(body).is_err());
    }

    #[test]
    fn parse_issue_list_unknown_state_errors() {
        let body = r#"[{
            "number": 1, "title": "t", "state": "WEIRD",
            "labels": [], "url": "u", "createdAt": "", "updatedAt": ""
        }]"#;
        assert!(parse_issue_list(body).is_err());
    }
}
