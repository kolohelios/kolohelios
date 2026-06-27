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

    /// Remove `target.path` from git. A path with no committed file is a
    /// no-op success, so a delete is idempotent.
    async fn delete_file(&self, target: &GitTarget, message: &str) -> Result<(), CommitError>;
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

/// Move a note's backing file from `old` to `new`, carrying `content`.
/// **Create-before-delete**: the destination file is committed first (so the
/// body is durable at the new path before anything is removed), and only
/// then is the source file dropped. A partial failure never *loses* data,
/// but it is not automatically clean to retry:
///
/// - If the **create** fails, nothing changed — the source still holds the
///   body and a retry is safe.
/// - If the create succeeds but the **delete** (or any later orchestration
///   step) fails, the *destination* now lingers alongside the source. A
///   naive retry would be rejected as a destination collision; the caller
///   must instead resume the move idempotently (see [`crate::route::plan_rename`]
///   and [`crate::route::RenamePlan::Resume`]). Re-running this helper as
///   part of that resume is safe: the create re-PUTs the same body and the
///   delete then removes the source.
///
/// Returns the new file's commit sha. The caller is responsible for refusing
/// an overwrite of a *different* note already at the destination; this helper
/// carries `content` to `new` unconditionally.
pub async fn move_note_file<C: GitHubClient>(
    client: &C,
    old: &GitTarget,
    new: &GitTarget,
    content: &str,
    max_retries: u32,
) -> Result<String, CommitError> {
    let create_msg = format!("note: rename {} -> {}", old.path, new.path);
    let sha = commit_with_retry(client, new, content, &create_msg, max_retries).await?;
    // Destination is durable; now drop the source.
    let delete_msg = format!("note: rename remove {}", old.path);
    client.delete_file(old, &delete_msg).await?;
    Ok(sha)
}

/// Parse a GitHub git-tree listing (the `git/trees/<ref>?recursive=1`
/// response) into the vault's note ids: every blob under `notes/` whose
/// name ends in `.md`, with the `notes/` prefix and `.md` suffix removed,
/// sorted for a stable listing. Directories, non-markdown files, and
/// anything outside `notes/` are ignored. This is the pure half of the
/// vault index — the wasm client just fetches the tree and hands it here.
pub fn note_paths_from_tree(tree_json: &str) -> Result<Vec<String>, CommitError> {
    let v: serde_json::Value =
        serde_json::from_str(tree_json).map_err(|e| CommitError::Transport(e.to_string()))?;
    let entries = v
        .get("tree")
        .and_then(|t| t.as_array())
        .ok_or_else(|| CommitError::Transport("tree listing missing `tree` array".into()))?;
    let mut ids: Vec<String> = entries
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("blob"))
        .filter_map(|e| e.get("path").and_then(|p| p.as_str()))
        .filter_map(|path| path.strip_prefix("notes/")?.strip_suffix(".md"))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    ids.sort();
    Ok(ids)
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

        async fn delete_file(
            &self,
            _target: &GitTarget,
            _message: &str,
        ) -> Result<(), CommitError> {
            Ok(())
        }
    }

    /// Fake that records every operation in order so a test can assert the
    /// create-before-delete sequence and the path/content a move writes.
    struct RecordingClient {
        events: std::cell::RefCell<Vec<String>>,
    }

    impl RecordingClient {
        fn new() -> Self {
            Self {
                events: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl GitHubClient for RecordingClient {
        async fn current_sha(&self, target: &GitTarget) -> Result<Option<String>, CommitError> {
            self.events
                .borrow_mut()
                .push(format!("read:{}", target.path));
            // Destination is free (a fresh create); the move helper assumes so.
            Ok(None)
        }

        async fn put_file(
            &self,
            target: &GitTarget,
            content: &str,
            _base_sha: Option<&str>,
            _message: &str,
        ) -> Result<PutOutcome, CommitError> {
            self.events
                .borrow_mut()
                .push(format!("put:{}={}", target.path, content));
            Ok(PutOutcome::Committed("dest-sha".into()))
        }

        async fn delete_file(&self, target: &GitTarget, _message: &str) -> Result<(), CommitError> {
            self.events
                .borrow_mut()
                .push(format!("delete:{}", target.path));
            Ok(())
        }
    }

    fn note_target(path: &str) -> GitTarget {
        GitTarget {
            owner: "kolohelios".into(),
            repo: "notes".into(),
            branch: "main".into(),
            path: path.into(),
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

    #[tokio::test]
    async fn move_creates_destination_before_deleting_source() {
        let c = RecordingClient::new();
        let sha = move_note_file(
            &c,
            &note_target("notes/old.md"),
            &note_target("notes/projects/new.md"),
            "hello body",
            3,
        )
        .await
        .unwrap();
        assert_eq!(sha, "dest-sha");

        let events = c.events.borrow();
        // The destination is written (with the carried content) and only then
        // is the source removed — create-before-delete, so no body is ever
        // left unreferenced.
        assert_eq!(
            *events,
            vec![
                "read:notes/projects/new.md".to_string(),
                "put:notes/projects/new.md=hello body".to_string(),
                "delete:notes/old.md".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn move_propagates_a_destination_write_failure_without_deleting() {
        // A client whose create fails must never reach the delete — the source
        // file has to survive so the body isn't lost.
        struct FailingPut;
        impl GitHubClient for FailingPut {
            async fn current_sha(&self, _t: &GitTarget) -> Result<Option<String>, CommitError> {
                Ok(None)
            }
            async fn put_file(
                &self,
                _t: &GitTarget,
                _c: &str,
                _b: Option<&str>,
                _m: &str,
            ) -> Result<PutOutcome, CommitError> {
                Err(CommitError::Transport("boom".into()))
            }
            async fn delete_file(&self, _t: &GitTarget, _m: &str) -> Result<(), CommitError> {
                panic!("delete must not run when the destination write failed");
            }
        }
        let err = move_note_file(
            &FailingPut,
            &note_target("notes/old.md"),
            &note_target("notes/new.md"),
            "body",
            3,
        )
        .await
        .unwrap_err();
        assert_eq!(err, CommitError::Transport("boom".into()));
    }

    #[tokio::test]
    async fn move_propagates_a_delete_failure_after_the_create_landed() {
        // The data-safety contract: when the destination create succeeds but
        // the source delete fails, the error propagates *and* the create has
        // already happened — the body is durable at the new path before the
        // failure, so the caller can resume rather than lose data.
        struct CreatesThenFailsDelete {
            created: Cell<bool>,
        }
        impl GitHubClient for CreatesThenFailsDelete {
            async fn current_sha(&self, _t: &GitTarget) -> Result<Option<String>, CommitError> {
                Ok(None)
            }
            async fn put_file(
                &self,
                _t: &GitTarget,
                _c: &str,
                _b: Option<&str>,
                _m: &str,
            ) -> Result<PutOutcome, CommitError> {
                self.created.set(true);
                Ok(PutOutcome::Committed("dest-sha".into()))
            }
            async fn delete_file(&self, _t: &GitTarget, _m: &str) -> Result<(), CommitError> {
                Err(CommitError::Transport("delete boom".into()))
            }
        }
        let client = CreatesThenFailsDelete {
            created: Cell::new(false),
        };
        let err = move_note_file(
            &client,
            &note_target("notes/old.md"),
            &note_target("notes/new.md"),
            "body",
            3,
        )
        .await
        .unwrap_err();
        assert_eq!(err, CommitError::Transport("delete boom".into()));
        // Destination-durable-before-error: the create ran before the delete
        // failed, so the body is safe at the new path for a resume.
        assert!(client.created.get());
    }

    #[test]
    fn note_paths_extracts_markdown_blobs_under_notes() {
        // A recursive tree mixes nested blobs, directory entries, files
        // outside `notes/`, and non-markdown files — only `notes/*.md`
        // blobs are notes, prefix/suffix stripped, and the result sorted.
        let json = r#"{"tree":[
            {"path":"notes/scratch.md","type":"blob"},
            {"path":"notes/projects","type":"tree"},
            {"path":"notes/projects/foo.md","type":"blob"},
            {"path":"notes/diagram.png","type":"blob"},
            {"path":"README.md","type":"blob"}
        ],"truncated":false}"#;
        assert_eq!(
            note_paths_from_tree(json).unwrap(),
            vec!["projects/foo".to_string(), "scratch".to_string()]
        );
    }

    #[test]
    fn note_paths_is_empty_for_a_vault_with_no_notes() {
        let json = r#"{"tree":[{"path":"README.md","type":"blob"}],"truncated":false}"#;
        assert!(note_paths_from_tree(json).unwrap().is_empty());
    }

    #[test]
    fn note_paths_errors_on_a_malformed_listing() {
        assert!(note_paths_from_tree("not json").is_err());
        assert!(note_paths_from_tree("{}").is_err());
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

        /// List the vault's note ids by walking the repo's git tree under
        /// `notes/` (recursive trees API). Returns an empty list when the
        /// branch has no commits yet (a fresh repo) or holds no notes.
        pub async fn list_note_paths(
            &self,
            owner: &str,
            repo: &str,
            branch: &str,
        ) -> Result<Vec<String>, CommitError> {
            let url = format!(
                "https://api.github.com/repos/{owner}/{repo}/git/trees/{branch}?recursive=1"
            );
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
                    super::note_paths_from_tree(&body)
                }
                // No commits on the branch yet (empty repo) — an empty vault.
                404 | 409 => Ok(Vec::new()),
                other => Err(CommitError::Transport(format!(
                    "GET trees returned {other}"
                ))),
            }
        }

        /// Read the decoded UTF-8 body committed at `target.path`, or `None`
        /// if no file exists there. The rename orchestrator uses this to
        /// recover the source body when the source Durable Object is cold
        /// (its in-memory text is empty though git holds the note) and to
        /// read the destination body when resuming a half-finished move.
        /// Carrying `""` from a cold DO would erase the note on the move, so
        /// this git fallback is what keeps a transient display gap from
        /// becoming permanent data loss.
        pub async fn get_file_content(
            &self,
            target: &GitTarget,
        ) -> Result<Option<String>, CommitError> {
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
                    // GitHub returns base64 with embedded newlines; the
                    // STANDARD engine rejects whitespace, so strip it first.
                    let encoded: String = v
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or_default()
                        .split_whitespace()
                        .collect();
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(encoded.as_bytes())
                        .map_err(|e| CommitError::Transport(e.to_string()))?;
                    let text = String::from_utf8(bytes)
                        .map_err(|e| CommitError::Transport(e.to_string()))?;
                    Ok(Some(text))
                }
                404 => Ok(None),
                other => Err(CommitError::Transport(format!(
                    "GET contents returned {other}"
                ))),
            }
        }

        /// Remove `target.path` from git. A note that was never committed
        /// has no file — that's a no-op success, so a delete is idempotent.
        /// Thin alias over the [`GitHubClient::delete_file`] trait method so
        /// existing callers (the `/delete` purge) read naturally.
        pub async fn delete_note_file(
            &self,
            target: &GitTarget,
            message: &str,
        ) -> Result<(), CommitError> {
            self.delete_file(target, message).await
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

        async fn delete_file(&self, target: &GitTarget, message: &str) -> Result<(), CommitError> {
            // The contents API needs the current blob sha to delete it.
            let Some(sha) = self.current_sha(target).await? else {
                return Ok(());
            };
            let body = serde_json::json!({
                "message": message,
                "sha": sha,
                "branch": target.branch,
            });
            let mut init = RequestInit::new();
            init.with_method(Method::Delete)
                .with_headers(self.headers()?)
                .with_body(Some(body.to_string().into()));
            let req = Request::new_with_init(&Self::contents_url(target), &init)
                .map_err(|e| CommitError::Transport(e.to_string()))?;
            let resp = self.send(req).await?;
            match resp.status_code() {
                200 => Ok(()),
                other => Err(CommitError::Transport(format!(
                    "DELETE contents returned {other}"
                ))),
            }
        }
    }
}
