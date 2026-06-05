//! GitHub as the cold storage tier: committing a note's current body to a
//! repo as a single file, with an optimistic retry when the ref moves
//! under us. The fast cadence never reaches here — git commits are lazy
//! (debounce + backstop alarms, and on last-socket-disconnect).
//!
//! The retry loop (`commit_with_retry`) is generic over a [`GitHubClient`]
//! so it's native-testable with a fake; the real client (wasm-only) talks
//! to the GitHub contents API via `worker::Fetch`.

/// Where a note's markdown lives in the backing GitHub repo.
#[derive(Debug, Clone)]
pub struct GitTarget {
    pub owner: String,
    pub repo: String,
    pub branch: String,
    /// Repo-relative path, e.g. `notes/<id>.md`.
    pub path: String,
}

/// Result of a single write attempt.
pub enum PutOutcome {
    /// Committed; carries the new commit sha.
    Committed(String),
    /// The base sha was stale — the ref moved under us. Re-read and retry.
    StaleRef,
}

/// Why a commit could not be landed.
#[derive(Debug, PartialEq, Eq)]
pub enum CommitError {
    /// Transport or API error (status + message).
    Transport(String),
    /// The ref kept moving past `max_retries` attempts.
    RetriesExhausted,
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommitError::Transport(m) => write!(f, "github transport error: {m}"),
            CommitError::RetriesExhausted => {
                write!(f, "ref kept moving; commit retries exhausted")
            }
        }
    }
}

impl std::error::Error for CommitError {}

/// The GitHub operations the retry loop needs. Implemented for real by a
/// `worker::Fetch`-backed client on wasm, and by fakes in tests.
//
// `async fn` in a trait is fine here: the only consumer is the generic
// `commit_with_retry` (static dispatch), and we deliberately want no
// `Send` bound — the wasm `worker::Fetch` futures are `!Send`.
#[allow(async_fn_in_trait)]
pub trait GitHubClient {
    /// Current blob sha of the file at `target.path`, or `None` if the
    /// file doesn't exist yet (a fresh note).
    async fn current_sha(&self, target: &GitTarget) -> Result<Option<String>, CommitError>;

    /// Write `content` to `target.path` on top of `base_sha` (`None` to
    /// create a new file). Returns `Committed(sha)` or `StaleRef`.
    async fn put_file(
        &self,
        target: &GitTarget,
        content: &str,
        base_sha: Option<&str>,
        message: &str,
    ) -> Result<PutOutcome, CommitError>;
}

/// Commit `text` to `target`, re-reading the sha and retrying when the ref
/// moves under us. Returns the new commit sha. No commit coordinator is
/// needed at single-user scale — the optimistic retry absorbs the rare
/// concurrent write.
pub async fn commit_with_retry<C: GitHubClient>(
    client: &C,
    target: &GitTarget,
    text: &str,
    message: &str,
    max_retries: u32,
) -> Result<String, CommitError> {
    for _ in 0..=max_retries {
        let base_sha = client.current_sha(target).await?;
        match client
            .put_file(target, text, base_sha.as_deref(), message)
            .await?
        {
            PutOutcome::Committed(sha) => return Ok(sha),
            // Ref moved between the read and the write — loop to re-read.
            PutOutcome::StaleRef => continue,
        }
    }
    Err(CommitError::RetriesExhausted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Fake that returns `stale_first` StaleRef outcomes before
    /// succeeding, counting how many times each method was called.
    struct FakeClient {
        stale_first: Cell<u32>,
        reads: Cell<u32>,
        puts: Cell<u32>,
    }

    impl FakeClient {
        fn new(stale_first: u32) -> Self {
            Self {
                stale_first: Cell::new(stale_first),
                reads: Cell::new(0),
                puts: Cell::new(0),
            }
        }
    }

    impl GitHubClient for FakeClient {
        async fn current_sha(&self, _target: &GitTarget) -> Result<Option<String>, CommitError> {
            self.reads.set(self.reads.get() + 1);
            Ok(Some("base-sha".into()))
        }

        async fn put_file(
            &self,
            _target: &GitTarget,
            _content: &str,
            _base_sha: Option<&str>,
            _message: &str,
        ) -> Result<PutOutcome, CommitError> {
            self.puts.set(self.puts.get() + 1);
            if self.stale_first.get() > 0 {
                self.stale_first.set(self.stale_first.get() - 1);
                Ok(PutOutcome::StaleRef)
            } else {
                Ok(PutOutcome::Committed("new-commit-sha".into()))
            }
        }
    }

    fn target() -> GitTarget {
        GitTarget {
            owner: "kolohelios".into(),
            repo: "notes".into(),
            branch: "main".into(),
            path: "notes/demo.md".into(),
        }
    }

    #[tokio::test]
    async fn commits_on_the_first_try_when_ref_is_fresh() {
        let c = FakeClient::new(0);
        let sha = commit_with_retry(&c, &target(), "body", "msg", 3)
            .await
            .unwrap();
        assert_eq!(sha, "new-commit-sha");
        assert_eq!(c.reads.get(), 1);
        assert_eq!(c.puts.get(), 1);
    }

    #[tokio::test]
    async fn retries_then_succeeds_after_stale_ref_conflicts() {
        let c = FakeClient::new(2);
        let sha = commit_with_retry(&c, &target(), "body", "msg", 3)
            .await
            .unwrap();
        assert_eq!(sha, "new-commit-sha");
        // Re-read the sha before each of the 3 attempts.
        assert_eq!(c.reads.get(), 3);
        assert_eq!(c.puts.get(), 3);
    }

    #[tokio::test]
    async fn exhausts_retries_when_ref_keeps_moving() {
        let c = FakeClient::new(99);
        let err = commit_with_retry(&c, &target(), "body", "msg", 3)
            .await
            .unwrap_err();
        assert_eq!(err, CommitError::RetriesExhausted);
        // max_retries=3 → 4 attempts (the initial try plus 3 retries).
        assert_eq!(c.puts.get(), 4);
    }
}

#[cfg(target_arch = "wasm32")]
pub use worker_client::WorkerGitHubClient;

/// Real GitHub contents-API client, wasm-only because it depends on
/// `worker::Fetch`. The `commit_with_retry` loop above drives it.
#[cfg(target_arch = "wasm32")]
mod worker_client {
    use super::{CommitError, GitHubClient, GitTarget, PutOutcome};
    use base64::Engine;
    use worker::{Fetch, Headers, Method, Request, RequestInit};

    /// A GitHub contents-API client authenticated with a static token (a
    /// GitHub App installation token or a PAT) held as a Wrangler secret.
    pub struct WorkerGitHubClient {
        token: String,
    }

    impl WorkerGitHubClient {
        pub fn new(token: String) -> Self {
            Self { token }
        }

        fn contents_url(target: &GitTarget) -> String {
            format!(
                "https://api.github.com/repos/{}/{}/contents/{}",
                target.owner, target.repo, target.path
            )
        }

        fn headers(&self) -> Result<Headers, CommitError> {
            let headers = Headers::new();
            let set = |h: &Headers, k: &str, v: &str| {
                h.set(k, v)
                    .map_err(|e| CommitError::Transport(e.to_string()))
            };
            set(&headers, "Authorization", &format!("Bearer {}", self.token))?;
            set(&headers, "Accept", "application/vnd.github+json")?;
            set(&headers, "X-GitHub-Api-Version", "2022-11-28")?;
            // GitHub rejects requests without a User-Agent.
            set(&headers, "User-Agent", "kolohelios-notes-sync")?;
            Ok(headers)
        }

        async fn send(&self, req: Request) -> Result<worker::Response, CommitError> {
            Fetch::Request(req)
                .send()
                .await
                .map_err(|e| CommitError::Transport(e.to_string()))
        }
    }

    impl GitHubClient for WorkerGitHubClient {
        async fn current_sha(&self, target: &GitTarget) -> Result<Option<String>, CommitError> {
            let url = format!("{}?ref={}", Self::contents_url(target), target.branch);
            let mut init = RequestInit::new();
            init.with_method(Method::Get).with_headers(self.headers()?);
            let req = Request::new_with_init(&url, &init)
                .map_err(|e| CommitError::Transport(e.to_string()))?;
            let mut resp = self.send(req).await?;

            match resp.status_code() {
                200 => {
                    let body = resp
                        .text()
                        .await
                        .map_err(|e| CommitError::Transport(e.to_string()))?;
                    let v: serde_json::Value = serde_json::from_str(&body)
                        .map_err(|e| CommitError::Transport(e.to_string()))?;
                    Ok(v.get("sha").and_then(|s| s.as_str()).map(str::to_owned))
                }
                404 => Ok(None),
                other => Err(CommitError::Transport(format!(
                    "GET contents returned {other}"
                ))),
            }
        }

        async fn put_file(
            &self,
            target: &GitTarget,
            content: &str,
            base_sha: Option<&str>,
            message: &str,
        ) -> Result<PutOutcome, CommitError> {
            let encoded = base64::engine::general_purpose::STANDARD.encode(content);
            let mut body = serde_json::json!({
                "message": message,
                "content": encoded,
                "branch": target.branch,
            });
            if let Some(sha) = base_sha {
                body["sha"] = serde_json::Value::String(sha.to_owned());
            }

            let mut init = RequestInit::new();
            init.with_method(Method::Put)
                .with_headers(self.headers()?)
                .with_body(Some(body.to_string().into()));
            let req = Request::new_with_init(&Self::contents_url(target), &init)
                .map_err(|e| CommitError::Transport(e.to_string()))?;
            let mut resp = self.send(req).await?;

            match resp.status_code() {
                200 | 201 => {
                    let text = resp
                        .text()
                        .await
                        .map_err(|e| CommitError::Transport(e.to_string()))?;
                    let v: serde_json::Value = serde_json::from_str(&text)
                        .map_err(|e| CommitError::Transport(e.to_string()))?;
                    let sha = v
                        .get("commit")
                        .and_then(|c| c.get("sha"))
                        .and_then(|s| s.as_str())
                        .unwrap_or_default()
                        .to_owned();
                    Ok(PutOutcome::Committed(sha))
                }
                // The supplied sha didn't match the current ref.
                409 => Ok(PutOutcome::StaleRef),
                other => Err(CommitError::Transport(format!(
                    "PUT contents returned {other}"
                ))),
            }
        }
    }
}
