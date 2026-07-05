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
    Headers, Method, Request, RequestInit, Response, ResponseBuilder, Result, State, WebSocket,
    WebSocketIncomingMessage, WebSocketPair,
};

use crate::auth::{authorize_owner, authorized_did};
use crate::git::{commit_with_retry, move_note_file, CommitError, GitTarget, WorkerGitHubClient};
use crate::route::{
    choose_rename_body, is_valid_note_id, parse_ws_note_id, plan_rename,
    rename_blocked_by_live_socket, rename_lease_active, RenameError, RenamePlan,
};
use crate::state::{is_stale, next_alarm};

/// Internal DO control path: the top-level `/delete` handler forwards a
/// `POST` here to the note's Durable Object so it purges its own storage
/// and backing git file. Never routed from the public surface — only the
/// orchestrator reaches it, with the note id in `X-Note-Id`.
const PURGE_PATH: &str = "/__purge";

/// Internal DO control path: the `/rename` orchestrator forwards a `POST`
/// here to the *source* note's Durable Object to read its materialized body
/// (returned as `{"seq":…,"text":…}`) before the move. Read-only — it never
/// mutates the source, so a later failure leaves the source recoverable.
const EXPORT_PATH: &str = "/__export";

/// Internal DO control path: the `/rename` orchestrator forwards a `POST`
/// here to the *destination* note's Durable Object with the migrated body in
/// the request JSON (`{"text":…}`) and the new id in `X-Note-Id`, so opening
/// `?note=<new>` shows the content. The DO does not hydrate from git on open,
/// so a fresh id is an empty object until it is seeded here.
const SEED_PATH: &str = "/__seed";

/// Internal DO control path: the `/rename` orchestrator forwards a `POST`
/// here to a note's Durable Object to read how many live editor websockets
/// it currently holds (returned as `{"count":N}`) *and*, when that count is
/// zero, take a short-lived rename lease in one atomic handler run. The count
/// lets the orchestrator refuse a rename whose source or destination is open
/// in another tab; the lease then blocks a *new* socket from attaching during
/// the move, closing the TOCTOU window between this probe and the source
/// purge (see `rename_lease_active`). Leasing only when idle avoids locking
/// out reopens of a note that already refused the rename.
const LEASE_PATH: &str = "/__lease";

/// Internal DO control path: the `/rename` orchestrator forwards a `POST`
/// here to clear a rename lease taken via `LEASE_PATH`. On a completed rename
/// the source purge and destination seed each `delete_all` the lease away, so
/// this only matters on a refused or failed move — it frees the note promptly
/// instead of waiting out the lease TTL.
const RELEASE_PATH: &str = "/__release";

/// Storage key holding a rename lease's expiry (epoch millis). Present and
/// unexpired means a rename is mid-flight and new editor sockets are refused;
/// the expiry lets a crashed orchestrator self-heal.
const RENAME_LEASE_KEY: &str = "rename_lease_until";

/// How long a rename lease holds before it self-expires. Sized well above the
/// realistic rename window (~0.5-3s, even with control-op and commit retries)
/// so the lease never lapses mid-move, while still self-healing quickly if the
/// orchestrator dies before releasing. The normal path releases explicitly, so
/// this bound only bites on a crash.
const RENAME_LEASE_TTL: Duration = Duration::from_secs(30);

/// Commit the note this long after the last edit — coalesces a burst of
/// keystrokes into a single git commit once the typing settles. Durability
/// lives in DO storage (an edit is persisted before its `Ack`), so git is
/// only backup/history/embedding-source; the cadence is sized for a legible
/// history, not for minimizing data loss.
const COMMIT_DEBOUNCE: Duration = Duration::from_secs(60);
/// Commit at least this often under continuous editing, so a never-idle
/// session still backs up. Bounds how far GitHub trails the DO; not a
/// durability bound (DO storage already holds every accepted edit).
const COMMIT_BACKSTOP: Duration = Duration::from_secs(300);
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
        "/vault" => return handle_vault(&req, &env).await,
        "/delete" => return handle_delete(&req, &env).await,
        "/rename" => return handle_rename(&req, &env).await,
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

    let headers = shell_cors_headers(env)?;
    let (status, body) = match did {
        Some(did) => (200, serde_json::json!({ "did": did }).to_string()),
        None => (401, "{}".to_owned()),
    };
    Ok(ResponseBuilder::new()
        .with_status(status)
        .with_headers(headers)
        .fixed(body.into_bytes()))
}

/// JSON-response headers for the shell's credentialed cross-origin fetches
/// (`/me`, `/vault`). A specific origin (`OAUTH_APP_URL`, not `*`) is
/// required when `Access-Control-Allow-Credentials` is set.
fn shell_cors_headers(env: &Env) -> Result<Headers> {
    let allow_origin = env
        .var("OAUTH_APP_URL")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let headers = Headers::new();
    headers.set("Access-Control-Allow-Origin", &allow_origin)?;
    headers.set("Access-Control-Allow-Credentials", "true")?;
    headers.set("Vary", "Origin")?;
    headers.set("Content-Type", "application/json")?;
    Ok(headers)
}

/// `GET /vault` — the sidebar's note listing. Returns
/// `{"notes": ["projects/foo", "scratch", …]}` for the owner's session,
/// else `401`. The index is a git-tree walk of `notes/`, so a note that's
/// been edited but not yet committed (within the lazy debounce/backstop
/// window) won't appear until its first commit lands.
async fn handle_vault(req: &Request, env: &Env) -> Result<Response> {
    if !session_authorized(req, env) {
        return Response::error("unauthorized", 401);
    }
    let owner = env.var("GITHUB_OWNER")?.to_string();
    let repo = env.var("GITHUB_REPO")?.to_string();
    let branch = env.var("GITHUB_BRANCH")?.to_string();
    let token = env.secret("GITHUB_TOKEN")?.to_string();
    let notes = WorkerGitHubClient::new(token)
        .list_note_paths(&owner, &repo, &branch)
        .await
        .map_err(|e| Error::RustError(e.to_string()))?;

    let headers = shell_cors_headers(env)?;
    let body = serde_json::json!({ "notes": notes }).to_string();
    Ok(ResponseBuilder::new()
        .with_status(200)
        .with_headers(headers)
        .fixed(body.into_bytes()))
}

/// `POST /delete?note=<id>` — delete a note for the owner's session.
/// Forwards to the note's Durable Object, which purges its own storage and
/// removes the backing git file (idempotent: deleting an uncommitted or
/// already-gone note still succeeds). Returns `{"ok": true}`.
async fn handle_delete(req: &Request, env: &Env) -> Result<Response> {
    if !session_authorized(req, env) {
        return Response::error("unauthorized", 401);
    }
    let note = req
        .url()?
        .query_pairs()
        .find(|(k, _)| k == "note")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    if !is_valid_note_id(&note) {
        return Response::error("invalid note id", 400);
    }

    if purge_note(env, &note).await.is_err() {
        return Response::error("delete failed", 502);
    }

    let headers = shell_cors_headers(env)?;
    Ok(ResponseBuilder::new()
        .with_status(200)
        .with_headers(headers)
        .fixed(b"{\"ok\":true}".to_vec()))
}

/// How many times to re-attempt an idempotent Durable Object control op
/// (seed/purge) before giving up. These run *after* the git move has already
/// made the destination durable, so a transient failure here must not be
/// allowed to leave the old path resurrectable or the new path blank.
const CONTROL_OP_RETRIES: u32 = 3;

/// `POST /rename?note=<old>&to=<new>` — move a note to a new path for the
/// owner's session. Requires the note not be open elsewhere: if a live
/// editor socket is connected to the source (or destination) the move is
/// refused (409), since an in-flight edit would otherwise be committed to
/// the old path and erased, and the purged source could be resurrected from
/// its stale socket. The orchestration is ordered for data safety:
///
/// 0. Refuse a source or destination that is open in another tab (live
///    websocket), before any mutation, so a refusal leaves both untouched.
///    On an idle note the same step takes a short-lived rename lease so a
///    *new* tab can't attach mid-move — closing the TOCTOU window between
///    this probe and step 5's purge. The lease is released (or, on success,
///    cleared by the purge/seed `delete_all`) once the move settles.
/// 1. Read the source body. The source DO's materialized text is preferred,
///    but a cold DO (never opened since eviction/migration) reports `""`
///    though git holds the note, so the committed git blob is the fallback —
///    carrying `""` would erase the source on the move.
/// 2. Refuse to clobber a live destination: a note typed into but not yet
///    committed to git is invisible to the committed-listing guard, so probe
///    the destination DO and reject (409) if it holds uncommitted content.
/// 3. Git move: create the destination with the body, then drop the source
///    (create-before-delete, so the body is durable at the new path first).
/// 4. Seed the destination DO so opening `?note=<new>` shows the body
///    (the DO does not hydrate from git on open), retried since the git move
///    has already committed.
/// 5. Clear the source DO so the old path can't be resurrected from a stale
///    reopen, also retried.
///
/// A destination that already exists in git is not a hard error when the
/// source is still present: it signals a half-finished earlier attempt
/// (the create landed but a later step failed), which is *resumed*
/// idempotently rather than wedged behind a 409 — provided the destination
/// holds the body being carried (a different body there is a genuine
/// collision). Returns `{"ok": true}`.
async fn handle_rename(req: &Request, env: &Env) -> Result<Response> {
    if !session_authorized(req, env) {
        return Response::error("unauthorized", 401);
    }
    let url = req.url()?;
    let param = |key: &str| {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default()
    };
    let old = param("note");
    let new = param("to");

    // The plan is decided against the committed vault listing; the same
    // client then performs the git move.
    let owner = env.var("GITHUB_OWNER")?.to_string();
    let repo = env.var("GITHUB_REPO")?.to_string();
    let branch = env.var("GITHUB_BRANCH")?.to_string();
    let token = env.secret("GITHUB_TOKEN")?.to_string();
    let client = WorkerGitHubClient::new(token);
    let existing = client
        .list_note_paths(&owner, &repo, &branch)
        .await
        .map_err(|e| Error::RustError(e.to_string()))?;

    let plan = match plan_rename(&old, &new, &existing) {
        Ok(plan) => plan,
        Err(RenameError::InvalidId) => return Response::error("invalid note id", 400),
        Err(RenameError::SameId) => {
            return Response::error("source and destination are the same", 400)
        }
        Err(RenameError::SourceMissing) => return Response::error("note not found", 404),
    };

    // 0. Refuse a note that's open in another tab, and lease an idle one. The
    //    move reads the source body, rewrites git, then purges the source DO;
    //    a live editor socket would have its in-flight edits committed to the
    //    old path and then erased, and could resurrect the purged note from
    //    its stale connection. The destination is probed too: `seed` does a
    //    `delete_all`, so a live editor there would lose its state. A count of
    //    zero also takes a rename lease, so a tab opening *after* the probe
    //    can't slip a fresh socket in before the purge/seed. Both checks run
    //    before any mutation, so a refusal leaves everything untouched.
    if rename_blocked_by_live_socket(acquire_rename_lease(env, &old).await?) {
        return Response::error("note is open in another tab; close it and retry", 409);
    }
    match acquire_rename_lease(env, &new).await {
        Ok(count) if rename_blocked_by_live_socket(count) => {
            // Source was idle, so it's now leased; free it before bailing so a
            // busy destination doesn't lock the source shut for the lease TTL.
            let _ = release_rename_lease(env, &old).await;
            return Response::error(
                "destination note is open in another tab; close it and retry",
                409,
            );
        }
        Ok(_) => {}
        Err(e) => {
            let _ = release_rename_lease(env, &old).await;
            return Err(e);
        }
    }

    // Both source and destination are now leased. Run the move, then release
    // both leases regardless of outcome: on success the purge/seed `delete_all`
    // already cleared them (release is then a no-op); on any refusal or error
    // this frees them promptly instead of waiting out the TTL.
    let result = perform_rename(env, &client, &old, &new, plan).await;
    let _ = release_rename_lease(env, &old).await;
    let _ = release_rename_lease(env, &new).await;
    result
}

/// Steps 1-5 of the rename, run once both notes hold a rename lease (see
/// `handle_rename`). Split out so `handle_rename` can release the leases on a
/// single join point covering every exit — the `?` short-circuits, the
/// `Response::error` refusals, and the success path alike.
async fn perform_rename(
    env: &Env,
    client: &WorkerGitHubClient,
    old: &str,
    new: &str,
    plan: RenamePlan,
) -> Result<Response> {
    let old_target = note_git_target(env, old)?;
    let new_target = note_git_target(env, new)?;

    // 1. Choose the body to carry: the source DO's live text when present,
    //    else the committed git blob (a cold DO reports `""`).
    let do_text = export_note_text(env, old).await?;
    let git_text = client
        .get_file_content(&old_target)
        .await
        .map_err(|e| Error::RustError(e.to_string()))?;
    let text = choose_rename_body(&do_text, git_text.as_deref());

    // 2. Guard the destination against a silent overwrite.
    match plan {
        RenamePlan::Fresh => {
            // The committed-listing guard already proved the destination is
            // free in git, but a note edited-but-not-yet-committed lives only
            // in its DO. Refuse rather than let `seed`'s `delete_all` wipe it.
            if !export_note_text(env, new).await?.is_empty() {
                return Response::error("destination already exists", 409);
            }
        }
        RenamePlan::Resume => {
            // Destination is already committed. It's a resumable in-progress
            // rename only if it holds the body we're carrying; a different
            // body there is a real, distinct note — a genuine collision.
            // A git-body comparison suffices to detect divergent destination
            // content because the websocket-close flush (see `websocket_close`)
            // commits any uncommitted destination edits to git on the last
            // socket leaving — so anything not yet in git still has a live
            // socket, which step 0's destination probe already rejected.
            let dest = client
                .get_file_content(&new_target)
                .await
                .map_err(|e| Error::RustError(e.to_string()))?;
            if dest.as_deref() != Some(text.as_str()) {
                return Response::error("destination already exists", 409);
            }
        }
    }

    // 3. Git move: create the destination with the body, then drop the source.
    //    Idempotent on a resume — the create re-PUTs the same body.
    move_note_file(client, &old_target, &new_target, &text, MAX_COMMIT_RETRIES)
        .await
        .map_err(|e| Error::RustError(e.to_string()))?;

    // 4. Seed the destination DO so opening `?note=<new>` shows the body. The
    //    git file already exists, so a missed seed would otherwise show blank.
    if seed_note_with_retry(env, new, &text).await.is_err() {
        return Response::error("rename seed failed", 502);
    }

    // 5. Clear the source DO (its git file is already gone) so a stale reopen
    //    of the old path cannot resurrect it.
    if purge_note_with_retry(env, old).await.is_err() {
        return Response::error("rename cleanup failed", 502);
    }

    let headers = shell_cors_headers(env)?;
    Ok(ResponseBuilder::new()
        .with_status(200)
        .with_headers(headers)
        .fixed(b"{\"ok\":true}".to_vec()))
}

/// Resolve a note's GitHub target from env config plus its (already
/// validated) id. The env config is shared across notes; only the path
/// differs, at `notes/<note_id>.md`.
fn note_git_target(env: &Env, note_id: &str) -> Result<GitTarget> {
    Ok(GitTarget {
        owner: env.var("GITHUB_OWNER")?.to_string(),
        repo: env.var("GITHUB_REPO")?.to_string(),
        branch: env.var("GITHUB_BRANCH")?.to_string(),
        path: format!("notes/{note_id}.md"),
    })
}

/// Build a `POST` control request to a note's Durable Object, carrying the
/// note id in `X-Note-Id` (so it needs no path encoding) and an optional
/// JSON body.
fn control_request(path: &str, note_id: &str, body: Option<String>) -> Result<Request> {
    let headers = Headers::new();
    headers.set("X-Note-Id", note_id)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post).with_headers(headers);
    if let Some(body) = body {
        init.with_body(Some(body.into()));
    }
    Request::new_with_init(&format!("https://do{path}"), &init)
}

/// Forward `ctrl` to the Durable Object for `note_id` and return its
/// response. The id keys the DO (`idFromName`) and rides the request so the
/// object can resolve the git path it owns.
async fn forward_to_note(env: &Env, note_id: &str, ctrl: Request) -> Result<Response> {
    let stub = env
        .durable_object("NOTE")?
        .id_from_name(note_id)?
        .get_stub()?;
    stub.fetch_with_request(ctrl).await
}

/// Read a note's current materialized body from its Durable Object.
async fn export_note_text(env: &Env, note_id: &str) -> Result<String> {
    let ctrl = control_request(EXPORT_PATH, note_id, None)?;
    let mut resp = forward_to_note(env, note_id, ctrl).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError("export failed".into()));
    }
    let body = resp.text().await?;
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| Error::RustError(e.to_string()))?;
    Ok(v.get("text")
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .to_owned())
}

/// Probe a note's live editor websocket count and, when it is zero, take a
/// rename lease on the Durable Object in the same atomic handler run (see
/// `LEASE_PATH`). Returns the count: a non-zero result means the note is open
/// elsewhere and the rename is refused (no lease taken); a zero result means
/// the note is now leased against a new socket attaching mid-move.
async fn acquire_rename_lease(env: &Env, note_id: &str) -> Result<usize> {
    let ctrl = control_request(LEASE_PATH, note_id, None)?;
    let mut resp = forward_to_note(env, note_id, ctrl).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError("socket probe failed".into()));
    }
    let body = resp.text().await?;
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| Error::RustError(e.to_string()))?;
    Ok(v.get("count").and_then(|c| c.as_u64()).unwrap_or_default() as usize)
}

/// Release a rename lease taken by `acquire_rename_lease`. Best-effort: a
/// completed rename already cleared the lease via the purge/seed `delete_all`,
/// and an unreleased lease self-expires, so a failure here only delays the
/// note reopening by the TTL rather than losing correctness.
async fn release_rename_lease(env: &Env, note_id: &str) -> Result<()> {
    let ctrl = control_request(RELEASE_PATH, note_id, None)?;
    let resp = forward_to_note(env, note_id, ctrl).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError("lease release failed".into()));
    }
    Ok(())
}

/// Seed a note's Durable Object with `text` so opening it shows the body.
async fn seed_note(env: &Env, note_id: &str, text: &str) -> Result<()> {
    let body = serde_json::json!({ "text": text }).to_string();
    let ctrl = control_request(SEED_PATH, note_id, Some(body))?;
    let resp = forward_to_note(env, note_id, ctrl).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError("seed failed".into()));
    }
    Ok(())
}

/// Purge a note's Durable Object (its storage and backing git file).
/// Idempotent: an uncommitted or already-gone note still succeeds.
async fn purge_note(env: &Env, note_id: &str) -> Result<()> {
    let ctrl = control_request(PURGE_PATH, note_id, None)?;
    let resp = forward_to_note(env, note_id, ctrl).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError("purge failed".into()));
    }
    Ok(())
}

/// Seed the destination DO, retrying a transient failure. Both `seed` and
/// the underlying control op are idempotent, so a retry just re-installs the
/// same body. Called after the git move has committed, where a permanently
/// missed seed would leave `?note=<new>` showing blank over real content.
async fn seed_note_with_retry(env: &Env, note_id: &str, text: &str) -> Result<()> {
    let mut last = Ok(());
    for _ in 0..CONTROL_OP_RETRIES {
        match seed_note(env, note_id, text).await {
            Ok(()) => return Ok(()),
            Err(e) => last = Err(e),
        }
    }
    last
}

/// Purge the source DO, retrying a transient failure. `purge` is idempotent,
/// so a retry that races a prior partial purge still converges. Called as the
/// rename's final step, where a permanently missed purge would leave the old
/// path resurrectable from a stale reopen.
async fn purge_note_with_retry(env: &Env, note_id: &str) -> Result<()> {
    let mut last = Ok(());
    for _ in 0..CONTROL_OP_RETRIES {
        match purge_note(env, note_id).await {
            Ok(()) => return Ok(()),
            Err(e) => last = Err(e),
        }
    }
    last
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
        self.git_target_for(&note_id)
    }

    /// Resolve the GitHub target for an explicit (already-validated) note
    /// id. The env config is shared across notes; only the path differs.
    fn git_target_for(&self, note_id: &str) -> Result<GitTarget> {
        note_git_target(&self.env, note_id)
    }

    /// Delete this note: remove its backing git file, then wipe all durable
    /// storage so a future open of the same id starts empty. Git is dropped
    /// first — if that fails we return the error without touching storage,
    /// keeping the DO and git consistent for a retry. Idempotent: an
    /// uncommitted note has no file, and clearing clear storage is a no-op.
    async fn purge(&self, note_id: &str) -> Result<Response> {
        let target = self.git_target_for(note_id)?;
        let token = self.env.secret("GITHUB_TOKEN")?.to_string();
        WorkerGitHubClient::new(token)
            .delete_note_file(&target, &format!("note: delete {}", target.path))
            .await
            .map_err(|e| Error::RustError(e.to_string()))?;
        self.state.storage().delete_all().await?;
        let _ = self.state.storage().delete_alarm().await;
        Response::ok("purged")
    }

    /// Seed this (destination) Durable Object with a migrated body during a
    /// rename. The object is reset to a clean single-entry log holding `text`
    /// as the initial `Whole` delta, so a later eviction replays losslessly,
    /// and the snapshot is marked already-committed — the rename's git move
    /// wrote this exact body to the new path, so no redundant commit is due.
    async fn seed(&self, note_id: &str, text: &str) -> Result<Response> {
        self.state.storage().delete_all().await?;
        let _ = self.state.storage().delete_alarm().await;
        let seq: Seq = 1;
        let delta = Delta::Whole {
            text: text.to_owned(),
        };
        self.state.storage().put(&log_key(seq), &delta).await?;
        self.state.storage().put("seq", seq).await?;
        self.state.storage().put("text", text).await?;
        self.state.storage().put("committed_seq", seq).await?;
        self.state.storage().put("note_id", note_id).await?;
        Response::ok("seeded")
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

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        // Internal control ops forwarded by the top-level orchestrators carry
        // the note id in `X-Note-Id`. Handled before the websocket path so they
        // never try to upgrade.
        if req.path() == PURGE_PATH {
            let note_id = req.headers().get("X-Note-Id")?.unwrap_or_default();
            return self.purge(&note_id).await;
        }
        // Rename, source side: report the current body so the orchestrator can
        // carry it to the new path. Read-only.
        if req.path() == EXPORT_PATH {
            let (seq, text) = self.load_state().await?;
            let body = serde_json::json!({ "seq": seq, "text": text }).to_string();
            return Response::ok(body);
        }
        // Rename guard: report how many live editor sockets this object holds
        // so the orchestrator can refuse a rename of a note open in another
        // tab, and — when idle — take a rename lease in the same run so a new
        // socket can't attach before the move finishes. Reading the count and
        // writing the lease in one handler is what makes it race-free: the DO
        // serializes handlers, so no accept slips between the two.
        if req.path() == LEASE_PATH {
            let count = self.state.get_websockets().len();
            if count == 0 {
                let until = Date::now().as_millis() as i64 + RENAME_LEASE_TTL.as_millis() as i64;
                self.state.storage().put(RENAME_LEASE_KEY, until).await?;
            }
            let body = serde_json::json!({ "count": count }).to_string();
            return Response::ok(body);
        }
        // Rename guard: drop a lease taken via `LEASE_PATH` so a refused or
        // failed move reopens the note without waiting out the TTL.
        if req.path() == RELEASE_PATH {
            self.state.storage().delete(RENAME_LEASE_KEY).await?;
            return Response::ok("released");
        }
        // Rename, destination side: install the migrated body so opening the
        // new id shows it. The body rides the request JSON.
        if req.path() == SEED_PATH {
            let note_id = req.headers().get("X-Note-Id")?.unwrap_or_default();
            let body = req.text().await?;
            let v: serde_json::Value =
                serde_json::from_str(&body).map_err(|e| Error::RustError(e.to_string()))?;
            let text = v
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_owned();
            return self.seed(&note_id, &text).await;
        }

        // Refuse a new editor socket while a rename holds a lease on this note:
        // the source is mid-purge (or the destination mid-seed), and a socket
        // attaching now would load a body about to be `delete_all`'d and lose
        // any edits typed into that window. The lease self-expires, so a
        // crashed rename reopens the note rather than wedging it shut.
        let lease_until: Option<i64> = self.state.storage().get(RENAME_LEASE_KEY).await?;
        if rename_lease_active(lease_until, Date::now().as_millis() as i64) {
            return Response::error("note is being renamed; retry shortly", 409);
        }

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
