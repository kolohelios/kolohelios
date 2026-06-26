//! Wasm-only Worker runtime: the `fetch` entrypoint that forwards a
//! websocket upgrade to the per-note Durable Object, and the
//! `NoteDurableObject` itself. Cfg-gated to `wasm32` because everything
//! here depends on `worker` runtime types that only exist on the
//! Cloudflare Worker target; native `cargo test` exercises the pure
//! `route`, `state`, and `git` modules instead.

use std::time::Duration;

use notes_protocol::{ClientMsg, Delta, Seq, ServerMsg};
use serde::{Deserialize, Serialize};
// `wasm_bindgen` is re-exported from `worker` and must be in scope: the
// `#[durable_object]` macro expands to JS-glue code that references it by
// bare name (see the workers-rs `counter.rs` example).
use worker::{
    console_log, durable_object, event, wasm_bindgen, Context, Date, DurableObject, Env, Error,
    Headers, Request, Response, ResponseBuilder, Result, State, WebSocket,
    WebSocketIncomingMessage, WebSocketPair,
};

use crate::auth::{authorize_owner, authorized_did};
use crate::git::{commit_with_retry, CommitError, GitTarget, WorkerGitHubClient};
use crate::route::parse_ws_note_id;
use crate::state::{is_stale, next_alarm};

/// Commit the note this long after the last edit — coalesces a burst of
/// keystrokes into a single git commit once the typing settles.
const COMMIT_DEBOUNCE: Duration = Duration::from_secs(5);
/// Commit at least this often under continuous editing, so a never-idle
/// session still backs up.
const COMMIT_BACKSTOP: Duration = Duration::from_secs(60);
/// After a failed commit, retry no sooner than this.
const COMMIT_RETRY_BACKOFF: Duration = Duration::from_secs(30);
/// Optimistic stale-ref retries within a single commit attempt.
const MAX_COMMIT_RETRIES: u32 = 3;

/// Top-level Worker entrypoint. A websocket upgrade for `/note/<id>/ws`
/// is forwarded to that note's Durable Object (`idFromName(id)`), which
/// is the single writer for the note; every other path gets a plain
/// hello response.
#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let path = req.path();

    // ATProto OAuth, authentication only. The metadata document's URL is
    // the client_id; login resolves + redirects; the callback verifies the
    // DID and mints the session cookie.
    match path.as_str() {
        "/client-metadata.json" => {
            let base = env
                .var("OAUTH_BASE_URL")
                .map(|v| v.to_string())
                .unwrap_or_default();
            return Response::from_json(&crate::oauth::client_metadata(&base));
        }
        "/oauth/login" => return crate::oauth::handle_login(req, env).await,
        "/oauth/callback" => return crate::oauth::handle_callback(req, env).await,
        "/me" => return handle_me(&req, &env),
        _ => {}
    }

    if let Some(note_id) = parse_ws_note_id(&path) {
        // The session cookie gates the upgrade. The `sub`-vs-DID check at
        // login is where identity is established; this only re-checks the
        // signed cookie that login minted.
        if !session_authorized(&req, &env) {
            return Response::error("unauthorized", 401);
        }
        let stub = env
            .durable_object("NOTE")?
            .id_from_name(note_id)?
            .get_stub()?;
        return stub.fetch_with_request(req).await;
    }

    Response::ok("notes-sync: ok")
}

/// Whether the request is the owner's authenticated session. Authn-only
/// OAuth proves *an* identity; this proves it's *ours* — the signed
/// cookie must verify against `SESSION_SECRET` and carry the `OWNER_DID`.
/// Fails closed: a missing secret or owner var, or any non-owner/invalid
/// cookie, denies. (Earlier phases ran with the gate open while the OAuth
/// flow was being built; that fail-open default is gone now that the
/// secret and owner are configured.)
fn session_authorized(req: &Request, env: &Env) -> bool {
    let secret = env.secret("SESSION_SECRET").ok().map(|s| s.to_string());
    let owner = env.var("OWNER_DID").ok().map(|v| v.to_string());
    let header = req.headers().get("Cookie").ok().flatten();
    let now = Date::now().as_millis() as i64 / 1000;
    authorize_owner(
        header.as_deref(),
        secret.as_deref().map(str::as_bytes),
        owner.as_deref(),
        now,
    )
}

/// `GET /me` — the shell's auth probe. Returns `{"did": …}` with `200`
/// when the request carries the owner's session cookie, else `401`. The
/// session cookie is `HttpOnly`, so the front end can't read it directly;
/// this lets it learn whether it's signed in (and as whom) without
/// exposing the cookie. CORS allows the same-site cross-origin
/// credentialed fetch from the shell origin (`OAUTH_APP_URL`).
fn handle_me(req: &Request, env: &Env) -> Result<Response> {
    let secret = env.secret("SESSION_SECRET").ok().map(|s| s.to_string());
    let owner = env.var("OWNER_DID").ok().map(|v| v.to_string());
    let header = req.headers().get("Cookie").ok().flatten();
    let now = Date::now().as_millis() as i64 / 1000;
    let did = authorized_did(
        header.as_deref(),
        secret.as_deref().map(str::as_bytes),
        owner.as_deref(),
        now,
    );

    let allow_origin = env
        .var("OAUTH_APP_URL")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let headers = Headers::new();
    // A specific origin (not `*`) is required for credentialed CORS.
    headers.set("Access-Control-Allow-Origin", &allow_origin)?;
    headers.set("Access-Control-Allow-Credentials", "true")?;
    headers.set("Vary", "Origin")?;
    headers.set("Content-Type", "application/json")?;
    let (status, body) = match did {
        Some(did) => (200, serde_json::json!({ "did": did }).to_string()),
        None => (401, "{}".to_owned()),
    };
    Ok(ResponseBuilder::new()
        .with_status(status)
        .with_headers(headers)
        .fixed(body.into_bytes()))
}

/// Per-connection state persisted across hibernation via the socket
/// attachment. On wake the Durable Object is rebuilt from a lean
/// constructor, so anything connection-scoped (here, the note id) has to
/// ride the socket itself rather than a struct field.
#[derive(Serialize, Deserialize)]
struct SocketAttachment {
    note_id: String,
}

/// Storage key for the log entry at `seq`, zero-padded so a prefix
/// listing returns the deltas in sequence order. The log is append-only
/// and is the source of truth — the materialized `text`/`seq` snapshot is
/// only a fast path.
fn log_key(seq: Seq) -> String {
    format!("d:{seq:020}")
}

/// Serialize a server message to its JSON wire frame and send it.
fn send(ws: &WebSocket, msg: &ServerMsg) -> Result<()> {
    let frame = serde_json::to_string(msg).map_err(|e| Error::RustError(e.to_string()))?;
    ws.send_with_str(frame)
}

/// One Durable Object per note. The hibernation-critical invariant: all
/// durable state lives in `state.storage()` (the append-only edit log
/// plus a materialized snapshot) or the socket attachment, never in
/// `self` fields — so an evicted-and-rebuilt object replays losslessly.
#[durable_object]
pub struct NoteDurableObject {
    state: State,
    env: Env,
}

impl NoteDurableObject {
    /// Current `(seq, text)` snapshot, read fresh from durable storage so
    /// it reflects whatever survived a possible eviction. A fresh note is
    /// `(0, "")`.
    async fn load_state(&self) -> Result<(Seq, String)> {
        let seq = self.state.storage().get("seq").await?.unwrap_or(0);
        let text = self.state.storage().get("text").await?.unwrap_or_default();
        Ok((seq, text))
    }

    /// Append `delta` to the log under `seq` and update the materialized
    /// `(seq, text)` snapshot. All three writes land in one message
    /// handler, so workerd's output gate applies them atomically before
    /// the `Ack` is observed.
    async fn commit_edit(&self, seq: Seq, delta: &Delta, text: &str) -> Result<()> {
        self.state.storage().put(&log_key(seq), delta).await?;
        self.state.storage().put("seq", seq).await?;
        self.state.storage().put("text", text).await?;
        Ok(())
    }

    /// Schedule the lazy git commit after an accepted edit: push the
    /// debounce deadline out to now + `COMMIT_DEBOUNCE`, arm the backstop
    /// once if it isn't already, and set the DO's single alarm to the
    /// earlier of the two.
    async fn schedule_commit(&self) -> Result<()> {
        let now = Date::now().as_millis() as i64;
        let debounce = now + COMMIT_DEBOUNCE.as_millis() as i64;
        self.state
            .storage()
            .put("commit_debounce_due", debounce)
            .await?;

        let backstop = match self.state.storage().get("commit_backstop_due").await? {
            Some(existing) => existing,
            None => {
                let b = now + COMMIT_BACKSTOP.as_millis() as i64;
                self.state.storage().put("commit_backstop_due", b).await?;
                b
            }
        };

        if let Some(at) = next_alarm(Some(debounce), Some(backstop)) {
            let delay = (at - now).max(0) as u64;
            self.state
                .storage()
                .set_alarm(Duration::from_millis(delay))
                .await?;
        }
        Ok(())
    }

    /// Clear the commit deadlines (after a commit lands, or before a
    /// reschedule).
    async fn clear_commit_deadlines(&self) {
        let _ = self.state.storage().delete("commit_debounce_due").await;
        let _ = self.state.storage().delete("commit_backstop_due").await;
    }

    /// Resolve the GitHub target for this note from env config plus the
    /// stored note id. The body lives at `notes/<note_id>.md`.
    async fn git_target(&self) -> Result<GitTarget> {
        let note_id: String = self
            .state
            .storage()
            .get("note_id")
            .await?
            .unwrap_or_default();
        Ok(GitTarget {
            owner: self.env.var("GITHUB_OWNER")?.to_string(),
            repo: self.env.var("GITHUB_REPO")?.to_string(),
            branch: self.env.var("GITHUB_BRANCH")?.to_string(),
            path: format!("notes/{note_id}.md"),
        })
    }

    /// Commit the current body to git with optimistic stale-ref retry.
    /// Returns the new commit sha.
    async fn commit_now(&self, seq: Seq, text: &str) -> std::result::Result<String, CommitError> {
        let target = self
            .git_target()
            .await
            .map_err(|e| CommitError::Transport(e.to_string()))?;
        let token = self
            .env
            .secret("GITHUB_TOKEN")
            .map_err(|e| CommitError::Transport(e.to_string()))?
            .to_string();
        let client = WorkerGitHubClient::new(token);
        let message = format!("note: update {} (seq {seq})", target.path);
        commit_with_retry(&client, &target, text, &message, MAX_COMMIT_RETRIES).await
    }

    /// Send `msg` to every connected editor (e.g. a `BackedUp` after a
    /// commit lands).
    fn broadcast(&self, msg: &ServerMsg) {
        for ws in self.state.get_websockets() {
            let _ = send(&ws, msg);
        }
    }
}

impl DurableObject for NoteDurableObject {
    fn new(state: State, env: Env) -> Self {
        // Lean constructor: reruns on every wake from hibernation. Do no
        // eager I/O here — handlers read from storage lazily so the value
        // is whatever survived in the durable tier.
        console_log!("NoteDurableObject constructed (cold start or hibernation wake)");
        Self { state, env }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let note_id = parse_ws_note_id(&req.path())
            .map(str::to_owned)
            .unwrap_or_default();
        // Persist the note id so the alarm handler (which has no socket)
        // can resolve the git path on wake.
        self.state.storage().put("note_id", &note_id).await?;

        let pair = WebSocketPair::new()?;
        let server = pair.server;

        // Hibernation accept: the runtime may evict this object while the
        // socket stays open, waking it on the next inbound message.
        self.state.accept_web_socket(&server);
        server.serialize_attachment(SocketAttachment { note_id })?;

        Ok(ResponseBuilder::new()
            .with_status(101)
            .with_websocket(pair.client)
            .empty())
    }

    async fn websocket_message(
        &self,
        ws: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        // The attachment proves we accepted this socket and carries the
        // note id across hibernation.
        let _attachment: SocketAttachment = ws
            .deserialize_attachment()?
            .ok_or_else(|| Error::RustError("socket missing attachment".into()))?;

        let frame = match message {
            WebSocketIncomingMessage::String(s) => s,
            WebSocketIncomingMessage::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
        };

        let msg: ClientMsg = match serde_json::from_str(&frame) {
            Ok(m) => m,
            Err(e) => {
                // Unparsable frame — resync the client rather than guess.
                console_log!("dropping unparsable client frame: {e}");
                let (seq, text) = self.load_state().await?;
                return send(&ws, &ServerMsg::Sync { seq, text });
            }
        };

        match msg {
            // `since_seq` lets a future optimization skip the body when
            // the client is already current; phase 2 always sends the
            // snapshot so a reconnecting editor resyncs from the truth.
            ClientMsg::Open { since_seq: _ } => {
                let (seq, text) = self.load_state().await?;
                send(&ws, &ServerMsg::Sync { seq, text })
            }
            ClientMsg::Edit { base_seq, delta } => {
                let (seq, cur_text) = self.load_state().await?;
                if is_stale(base_seq, seq) {
                    // The client raced another accepted edit — reject by
                    // resyncing it to the current state.
                    return send(
                        &ws,
                        &ServerMsg::Sync {
                            seq,
                            text: cur_text,
                        },
                    );
                }
                match delta.apply(&cur_text) {
                    Ok(new_text) => {
                        let new_seq = seq + 1;
                        self.commit_edit(new_seq, &delta, &new_text).await?;
                        // Persist is done; schedule the lazy git commit.
                        // The fast cadence never touches git.
                        self.schedule_commit().await?;
                        send(&ws, &ServerMsg::Ack { seq: new_seq })
                    }
                    Err(e) => {
                        // Delta didn't apply to our text — force a resync.
                        console_log!("delta did not apply ({e}); resyncing");
                        send(
                            &ws,
                            &ServerMsg::Sync {
                                seq,
                                text: cur_text,
                            },
                        )
                    }
                }
            }
        }
    }

    async fn alarm(&self) -> Result<Response> {
        let (seq, text) = self.load_state().await?;
        let committed: Seq = self
            .state
            .storage()
            .get("committed_seq")
            .await?
            .unwrap_or(0);
        // Clear deadlines up front; a failed commit re-arms a backstop.
        self.clear_commit_deadlines().await;

        if seq <= committed {
            return Response::ok("nothing to commit");
        }

        match self.commit_now(seq, &text).await {
            Ok(commit_sha) => {
                self.state.storage().put("committed_seq", seq).await?;
                self.broadcast(&ServerMsg::BackedUp {
                    commit_sha: Some(commit_sha),
                });
                Response::ok("committed")
            }
            Err(e) => {
                console_log!("git commit failed: {e}; backing off");
                let backstop =
                    Date::now().as_millis() as i64 + COMMIT_RETRY_BACKOFF.as_millis() as i64;
                self.state
                    .storage()
                    .put("commit_backstop_due", backstop)
                    .await?;
                self.state.storage().set_alarm(COMMIT_RETRY_BACKOFF).await?;
                Response::error("commit failed; retry scheduled", 500)
            }
        }
    }

    async fn websocket_close(
        &self,
        _ws: WebSocket,
        _code: usize,
        _reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        // On the last socket leaving, flush to git immediately rather than
        // waiting for the alarm. `get_websockets()` still includes the
        // socket being closed, so `<= 1` means "this was the last one".
        if self.state.get_websockets().len() <= 1 {
            let (seq, text) = self.load_state().await?;
            let committed: Seq = self
                .state
                .storage()
                .get("committed_seq")
                .await?
                .unwrap_or(0);
            if seq > committed {
                match self.commit_now(seq, &text).await {
                    Ok(_) => {
                        self.state.storage().put("committed_seq", seq).await?;
                        self.clear_commit_deadlines().await;
                    }
                    Err(e) => {
                        // Leave the deadlines armed so the alarm retries.
                        console_log!("commit on disconnect failed: {e}");
                    }
                }
            }
        }
        Ok(())
    }

    async fn websocket_error(&self, _ws: WebSocket, _error: Error) -> Result<()> {
        Ok(())
    }
}
