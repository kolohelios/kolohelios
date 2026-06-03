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

/// Run `gh api graphql -f query=<query>` with the given variables.
///
/// `string_vars` becomes `-f name=value` (the value is passed as a
/// GraphQL `String!`); `int_vars` becomes `-F name=value` (numeric
/// coercion, suitable for `Int!`). The GitHub CLI handles the JSON
/// envelope and authentication.
fn api_graphql(
    query: &str,
    string_vars: &[(&str, &str)],
    int_vars: &[(&str, i64)],
) -> Result<Value, GhError> {
    let mut cmd = Command::new("gh");
    cmd.arg("api").arg("graphql");
    cmd.arg("-f").arg(format!("query={query}"));
    for (k, v) in string_vars {
        cmd.arg("-f").arg(format!("{k}={v}"));
    }
    for (k, v) in int_vars {
        cmd.arg("-F").arg(format!("{k}={v}"));
    }

    let output = cmd.output().context(SpawnSnafu)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: "gh api graphql".to_string(),
            stderr: stderr.to_string(),
        }
        .fail();
    }

    let body = String::from_utf8_lossy(&output.stdout);
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(&body).context(JsonParseSnafu {
        context: "failed to parse JSON from gh api graphql".to_string(),
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
pub fn merged_pr_for_issue(repo: &str, n: u64) -> Result<Option<PrInfo>, GhError> {
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &n.to_string(),
            "--repo",
            repo,
            "--json",
            "state,closedByPullRequestsReferences",
        ])
        .output()
        .context(SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh issue view {n} --repo {repo}"),
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
    parse_one_pr_list_entry(first).map(Some)
}

/// Parse a `gh pr list --json number,state,url` JSON array into a
/// `Vec<PrInfo>`. Empty input yields an empty Vec; one malformed entry
/// short-circuits with an error rather than silently dropping it.
pub(crate) fn parse_all_prs(body: &str) -> Result<Vec<PrInfo>, GhError> {
    let value: Value = serde_json::from_str(body).context(JsonParseSnafu {
        context: "failed to parse gh pr list JSON".to_string(),
    })?;
    let Some(arr) = value.as_array() else {
        return Ok(Vec::new());
    };
    arr.iter().map(parse_one_pr_list_entry).collect()
}

/// Lift one entry from a `gh pr list --json number,state,url` array
/// into a `PrInfo`. State strings are upper-case here (unlike the REST
/// API's lower-case `open`/`closed` + separate `merged_at`).
fn parse_one_pr_list_entry(value: &Value) -> Result<PrInfo, GhError> {
    let number = value["number"].as_u64().context(SchemaSnafu {
        message: "PR missing 'number' field",
    })?;
    let url = value["url"]
        .as_str()
        .with_context(|| SchemaSnafu {
            message: format!("PR #{number} missing 'url' field"),
        })?
        .to_string();
    let state = match value["state"].as_str() {
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
    Ok(PrInfo { number, url, state })
}

/// All OPEN PRs whose body references `Closes #n`. Returns an empty
/// Vec if none. See [`pr_for_issue`] for the single-PR (any-state)
/// variant; this helper exists so `shaka workspace status` can surface
/// live work even when the PR's bookmark isn't on the workspace's `@`.
pub fn open_prs_for_issue(repo: &str, n: u64) -> Result<Vec<PrInfo>, GhError> {
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
            "open",
            "--limit",
            "10",
            "--json",
            "number,state,url",
        ])
        .output()
        .context(SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh pr list (open, search Closes #{n})"),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }

    let body = String::from_utf8_lossy(&output.stdout);
    parse_all_prs(&body)
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
/// Shells out to `gh issue view <n> --repo <owner/repo> --json title
/// --jq .title`. `--repo` is resolved via [`detect_repo_or_env`] so this
/// works from inside a sibling jj workspace, which has no `.git` of its
/// own for `gh` to auto-detect the remote from.
///
/// Returns an error if `gh` is not authenticated or the issue does not exist.
pub fn issue_title(n: u64) -> Result<String, GhError> {
    let repo = detect_repo_or_env()?;
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &n.to_string(),
            "--repo",
            &repo,
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
            command: format!("gh issue view {n} --repo {repo}"),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }

    let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(title)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedIssue {
    pub number: u64,
    pub url: String,
}

/// Run `gh issue create` and return the created issue's number + URL.
///
/// `labels` and `milestone` are optional; both may be empty / `None`.
/// `--repo` is always passed so this works from inside a jj workspace.
pub fn issue_create(
    repo: &str,
    title: &str,
    body: &str,
    labels: &[String],
    milestone: Option<&str>,
) -> Result<CreatedIssue, GhError> {
    let mut args: Vec<String> = vec![
        "issue".into(),
        "create".into(),
        "--repo".into(),
        repo.into(),
        "--title".into(),
        title.into(),
        "--body".into(),
        body.into(),
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
            command: "gh issue create".to_string(),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let number = parse_issue_number_from_url(&url).with_context(|| SchemaSnafu {
        message: format!("could not parse issue number from gh URL: {url}"),
    })?;
    Ok(CreatedIssue { number, url })
}

pub(crate) fn parse_issue_number_from_url(url: &str) -> Option<u64> {
    url.rsplit('/').next().and_then(|s| s.parse().ok())
}

/// Fetch an issue's integer database id (distinct from its user-facing
/// number). The sub-issues API expects this id, not the number.
pub fn issue_db_id(repo: &str, number: u64) -> Result<u64, GhError> {
    let endpoint = format!("/repos/{repo}/issues/{number}");
    let value = api_get(&endpoint)?;
    value["id"].as_u64().with_context(|| SchemaSnafu {
        message: format!("issue {repo}#{number} response missing integer 'id'"),
    })
}

/// Fetch the native parent issue number for `{repo}#{number}`, if any.
///
/// Goes through GraphQL (`repository.issue.parent.number`) rather than
/// REST. GitHub's REST `/issues/{N}.parent` populates unreliably on
/// fresh sub-issue links — sometimes lagging by a day, sometimes
/// staying null indefinitely — even though the parent's
/// `/issues/{N}/sub_issues` endpoint and the GraphQL `issue.parent`
/// edge both show the link immediately. GraphQL is authoritative for
/// this relationship; see #599 for the reproduction.
///
/// Freeform `Sub-issue of #N` body text doesn't populate either edge —
/// that's the drift `shaka issue audit` flags.
pub fn issue_parent(repo: &str, number: u64) -> Result<Option<u64>, GhError> {
    let (owner, name) = repo.split_once('/').context(SchemaSnafu {
        message: format!("repo {repo:?} not in owner/name form"),
    })?;
    let query = "query($owner:String!,$name:String!,$num:Int!){repository(owner:$owner,name:$name){issue(number:$num){parent{number}}}}";
    let value = api_graphql(
        query,
        &[("owner", owner), ("name", name)],
        &[("num", number as i64)],
    )?;
    Ok(parse_issue_parent_graphql(&value))
}

/// Pluck the parent number from a `gh api graphql` response body for the
/// `issue_parent` query. Returns `None` when the issue has no native
/// parent, when the issue doesn't exist (`issue: null`), or when the
/// `parent` edge is null. Extracted from `issue_parent` so the parsing
/// logic can be unit-tested without spawning `gh`.
fn parse_issue_parent_graphql(body: &Value) -> Option<u64> {
    body.pointer("/data/repository/issue/parent/number")
        .and_then(|n| n.as_u64())
}

/// Returns `Ok(true)` if `{repo}#{number}` resolves, `Ok(false)` on 404,
/// or propagates any other error. Used to flag freeform references to
/// nonexistent issues.
pub fn issue_exists(repo: &str, number: u64) -> Result<bool, GhError> {
    let endpoint = format!("/repos/{repo}/issues/{number}");
    let output = Command::new("gh")
        .args([
            "api",
            &endpoint,
            "-H",
            "Accept: application/vnd.github+json",
        ])
        .output()
        .context(SpawnSnafu)?;
    if output.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("HTTP 404") || stderr.contains("Not Found") {
        return Ok(false);
    }
    GhCommandSnafu {
        command: format!("gh api {endpoint}"),
        stderr: stderr.trim().to_string(),
    }
    .fail()
}

/// Link `sub_id` (the integer db id of an existing issue) as a sub-issue
/// of `{repo}#{parent_number}` via GitHub's native sub-issues API.
///
/// Retries up to 3 times with 1s then 3s backoff on any failure —
/// distinguishing transient 5xx/network errors from permanent 4xx via
/// `gh` stderr is brittle, and a permanent error wastes at most ~4s
/// of backoff before surfacing.
pub fn add_sub_issue(repo: &str, parent_number: u64, sub_id: u64) -> Result<(), GhError> {
    let endpoint = format!("/repos/{repo}/issues/{parent_number}/sub_issues");
    let body = serde_json::json!({ "sub_issue_id": sub_id });

    let backoffs = [
        std::time::Duration::from_secs(1),
        std::time::Duration::from_secs(3),
    ];
    let mut last_err: Option<GhError> = None;
    for attempt in 0..=backoffs.len() {
        match api_post(&endpoint, &body) {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt < backoffs.len() {
                    std::thread::sleep(backoffs[attempt]);
                }
            }
        }
    }
    Err(last_err.expect("loop ran at least once"))
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
/// `labels` are AND-combined when more than one is supplied. When
/// `search` is `Some`, the query is passed through verbatim as GitHub
/// search syntax (combinable with the other filters).
pub fn list_issues(
    repo: &str,
    state: ListState,
    labels: &[String],
    milestone: Option<&str>,
    search: Option<&str>,
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
    if let Some(q) = search {
        args.push("--search".into());
        args.push(q.to_string());
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteLabel {
    pub name: String,
    pub color: String,
    pub description: String,
}

/// List all labels in `repo` via `gh label list --json name,color,description`.
pub fn list_labels(repo: &str) -> Result<Vec<RemoteLabel>, GhError> {
    let output = Command::new("gh")
        .args([
            "label",
            "list",
            "--repo",
            repo,
            "--limit",
            "1000",
            "--json",
            "name,color,description",
        ])
        .output()
        .context(SpawnSnafu)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: "gh label list".to_string(),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }
    let body = String::from_utf8_lossy(&output.stdout);
    parse_label_list(&body)
}

pub(crate) fn parse_label_list(body: &str) -> Result<Vec<RemoteLabel>, GhError> {
    let value: Value = serde_json::from_str(body).context(JsonParseSnafu {
        context: "failed to parse gh label list JSON".to_string(),
    })?;
    let arr = value.as_array().context(SchemaSnafu {
        message: "gh label list did not return an array",
    })?;
    arr.iter()
        .map(|v| {
            let name = v["name"]
                .as_str()
                .context(SchemaSnafu {
                    message: "label missing 'name'",
                })?
                .to_string();
            let color = v["color"]
                .as_str()
                .with_context(|| SchemaSnafu {
                    message: format!("label '{name}' missing 'color'"),
                })?
                .to_string();
            let description = v["description"].as_str().unwrap_or("").to_string();
            Ok(RemoteLabel {
                name,
                color,
                description,
            })
        })
        .collect()
}

pub fn label_create(repo: &str, name: &str, color: &str, description: &str) -> Result<(), GhError> {
    let output = Command::new("gh")
        .args([
            "label",
            "create",
            name,
            "--repo",
            repo,
            "--color",
            color,
            "--description",
            description,
        ])
        .output()
        .context(SpawnSnafu)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh label create {name}"),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }
    Ok(())
}

pub fn label_edit(repo: &str, name: &str, color: &str, description: &str) -> Result<(), GhError> {
    let output = Command::new("gh")
        .args([
            "label",
            "edit",
            name,
            "--repo",
            repo,
            "--color",
            color,
            "--description",
            description,
        ])
        .output()
        .context(SpawnSnafu)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh label edit {name}"),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }
    Ok(())
}

pub fn label_delete(repo: &str, name: &str) -> Result<(), GhError> {
    let output = Command::new("gh")
        .args(["label", "delete", name, "--repo", repo, "--yes"])
        .output()
        .context(SpawnSnafu)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GhCommandSnafu {
            command: format!("gh label delete {name}"),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }
    Ok(())
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
    fn parse_all_prs_multiple() {
        let body = r#"[
            {"number": 1, "state": "OPEN", "url": "u1"},
            {"number": 2, "state": "OPEN", "url": "u2"}
        ]"#;
        let prs = parse_all_prs(body).unwrap();
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].number, 1);
        assert_eq!(prs[1].number, 2);
    }

    #[test]
    fn parse_all_prs_empty() {
        assert!(parse_all_prs("[]").unwrap().is_empty());
    }

    #[test]
    fn parse_all_prs_short_circuits_on_bad_entry() {
        let body = r#"[
            {"number": 1, "state": "OPEN", "url": "u1"},
            {"number": 2, "state": "WAT", "url": "u2"}
        ]"#;
        assert!(parse_all_prs(body).is_err());
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

    #[test]
    fn parse_label_list_extracts_fields() {
        let body = r#"[
            {"name": "shaka", "color": "5319e7", "description": "shaka CLI"},
            {"name": "bug",   "color": "d73a4a", "description": "broken"}
        ]"#;
        let labels = parse_label_list(body).unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].name, "shaka");
        assert_eq!(labels[0].color, "5319e7");
        assert_eq!(labels[1].description, "broken");
    }

    #[test]
    fn parse_label_list_empty_description_ok() {
        let body = r#"[{"name": "x", "color": "abcdef", "description": ""}]"#;
        let labels = parse_label_list(body).unwrap();
        assert_eq!(labels[0].description, "");
    }

    #[test]
    fn parse_label_list_missing_color_errors() {
        let body = r#"[{"name": "x", "description": ""}]"#;
        assert!(parse_label_list(body).is_err());
    }

    #[test]
    fn parse_label_list_empty() {
        assert!(parse_label_list("[]").unwrap().is_empty());
    }

    #[test]
    fn parse_issue_parent_graphql_with_parent() {
        let body: Value =
            serde_json::from_str(r#"{"data":{"repository":{"issue":{"parent":{"number":289}}}}}"#)
                .unwrap();
        assert_eq!(parse_issue_parent_graphql(&body), Some(289));
    }

    #[test]
    fn parse_issue_parent_graphql_parent_null() {
        // Issue exists, no native parent — `parent` edge is null.
        let body: Value =
            serde_json::from_str(r#"{"data":{"repository":{"issue":{"parent":null}}}}"#).unwrap();
        assert_eq!(parse_issue_parent_graphql(&body), None);
    }

    #[test]
    fn parse_issue_parent_graphql_issue_missing() {
        // Issue doesn't exist — GitHub returns `issue: null`.
        // The audit caller has already filtered by existence so this
        // path is defensive, but verifying it returns None keeps the
        // behavior cleanly degraded.
        let body: Value =
            serde_json::from_str(r#"{"data":{"repository":{"issue":null}}}"#).unwrap();
        assert_eq!(parse_issue_parent_graphql(&body), None);
    }
}
