//! Thin durable adapter: per-note KV records, a durable work queue, and
//! per-note connection caches invalidated by the index generation.
use crate::embeddings::{Embeddings, WorkerEmbeddingClient, DEFAULT_ENDPOINT, DEFAULT_MODEL};
use crate::vault::{self, Connections, Entry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use worker::{
    console_log, durable_object, wasm_bindgen, DurableObject, Env, Error, ListOptions, Method,
    Request, RequestInit, Response, Result, State,
};

const PREFIX: &str = "entry:";
const PENDING: &str = "pending:";
const RETRY: Duration = Duration::from_secs(30);

#[derive(Clone, Serialize, Deserialize)]
struct Job {
    ticket: u64,
    attempts: u8,
}
#[derive(Serialize, Deserialize)]
struct Cached {
    generation: u64,
    model: String,
    connections: Connections,
}

fn model_name(env: &Env) -> String {
    env.var("OPENROUTER_EMBEDDINGS_MODEL")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| DEFAULT_MODEL.into())
}
fn endpoint(env: &Env) -> String {
    env.var("OPENROUTER_EMBEDDINGS_ENDPOINT")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| DEFAULT_ENDPOINT.into())
}
fn model(env: &Env) -> String {
    crate::embeddings::cache_model_id(&model_name(env), &endpoint(env))
}
fn embedding_client(env: &Env) -> Option<WorkerEmbeddingClient> {
    let key = env.secret("OPENROUTER_API_KEY").ok()?.to_string();
    if key.trim().is_empty() {
        return None;
    }
    Some(WorkerEmbeddingClient::new(
        key,
        model_name(env),
        endpoint(env),
    ))
}

pub async fn forward(env: &Env, path: &str, id: Option<&str>) -> Result<Response> {
    let mut init = RequestInit::new();
    init.with_method(Method::Post);
    let suffix = id.map(|id| format!("?id={id}")).unwrap_or_default();
    let request = Request::new_with_init(&format!("https://vault{path}{suffix}"), &init)?;
    env.durable_object("VAULT")?
        .id_from_name("owner")?
        .get_stub()?
        .fetch_with_request(request)
        .await
}

pub async fn notify(env: &Env, id: &str) -> Result<()> {
    let response = forward(env, "/refresh", Some(id)).await?;
    if response.status_code() != 200 {
        return Err(Error::RustError("vault notification failed".into()));
    }
    Ok(())
}

pub async fn handle(req: &Request, env: &Env) -> Result<Response> {
    if !crate::runtime::session_authorized(req, env) {
        return Response::error("unauthorized", 401);
    }
    if req.method() == Method::Post {
        if let Some(origin) = req.headers().get("Origin")? {
            let allowed = ["OAUTH_APP_URL", "OAUTH_BASE_URL"].iter().any(|key| {
                env.var(key)
                    .ok()
                    .is_some_and(|value| value.to_string() == origin)
            });
            if !allowed {
                return Response::error("origin not allowed", 403);
            }
        }
    }
    let path = match (req.method(), req.path().as_str()) {
        (Method::Get, "/vault/index") => "/index",
        (Method::Get, "/vault/connections") => "/connections",
        (Method::Post, "/vault/rebuild") => "/rebuild",
        _ => return Response::error("method not allowed", 405),
    };
    let id = req
        .url()?
        .query_pairs()
        .find(|(key, _)| key == "id")
        .map(|(_, value)| value.into_owned());
    if path == "/connections" && !id.as_deref().is_some_and(crate::route::is_valid_note_id) {
        return Response::error("valid note id required", 400);
    }
    let mut response = forward(env, path, id.as_deref()).await?;
    response.headers_mut().set(
        "Access-Control-Allow-Origin",
        &env.var("OAUTH_APP_URL")?.to_string(),
    )?;
    response
        .headers_mut()
        .set("Access-Control-Allow-Credentials", "true")?;
    response.headers_mut().set("Vary", "Origin")?;
    Ok(response)
}

#[durable_object]
pub struct VaultDurableObject {
    state: State,
    env: Env,
}

impl VaultDurableObject {
    async fn generation(&self) -> Result<u64> {
        Ok(self.state.storage().get("generation").await?.unwrap_or(0))
    }
    async fn changed(&self) -> Result<()> {
        self.state
            .storage()
            .put("generation", self.generation().await? + 1)
            .await
    }
    async fn entries(&self) -> Result<BTreeMap<String, Entry>> {
        let map = self
            .state
            .storage()
            .list_with_options(ListOptions::new().prefix(PREFIX))
            .await?;
        let mut entries = BTreeMap::new();
        for pair in map.entries() {
            let pair = js_sys::Array::from(&pair?);
            let entry: Entry = serde_wasm_bindgen::from_value(pair.get(1))?;
            entries.insert(entry.id.clone(), entry);
        }
        Ok(entries)
    }
    async fn pending(&self) -> Result<Vec<(String, Job)>> {
        let map = self
            .state
            .storage()
            .list_with_options(ListOptions::new().prefix(PENDING))
            .await?;
        let mut jobs = Vec::new();
        for pair in map.entries() {
            let pair = js_sys::Array::from(&pair?);
            let key = pair
                .get(0)
                .as_string()
                .ok_or_else(|| Error::RustError("invalid queue key".into()))?;
            jobs.push((
                key[PENDING.len()..].to_string(),
                serde_wasm_bindgen::from_value(pair.get(1))?,
            ));
        }
        Ok(jobs)
    }
    async fn enqueue(&self, id: &str, force: bool) -> Result<()> {
        let key = format!("{PENDING}{id}");
        if !force && self.state.storage().get::<Job>(&key).await?.is_some() {
            // Recover a persisted job even if an earlier alarm exhausted
            // the platform's retries before it could re-arm itself.
            if self.state.storage().get_alarm().await?.is_none() {
                self.state
                    .storage()
                    .set_alarm(Duration::from_secs(1))
                    .await?;
            }
            return Ok(());
        }
        let ticket: u64 = self.state.storage().get("ticket").await?.unwrap_or(0);
        self.state.storage().put("ticket", ticket + 1).await?;
        self.state
            .storage()
            .put(
                &key,
                Job {
                    ticket: ticket + 1,
                    attempts: 0,
                },
            )
            .await?;
        let row = format!("{PREFIX}{id}");
        if let Some(mut entry) = self.state.storage().get::<Entry>(&row).await? {
            entry.stale = true;
            self.state.storage().put(&row, entry).await?;
        }
        self.changed().await?;
        self.state
            .storage()
            .set_alarm(Duration::from_secs(1))
            .await?;
        Ok(())
    }
    async fn current(&self, id: &str, job: &Job) -> Result<bool> {
        Ok(self
            .state
            .storage()
            .get::<Job>(&format!("{PENDING}{id}"))
            .await?
            .is_some_and(|current| current.ticket == job.ticket))
    }
    async fn source(&self, id: &str) -> Result<Option<String>> {
        let stub = self
            .env
            .durable_object("NOTE")?
            .id_from_name(id)?
            .get_stub()?;
        let mut response = stub
            .fetch_with_str(&format!("https://note{}", crate::runtime::EXPORT_PATH))
            .await?;
        if response.status_code() != 200 {
            return Err(Error::RustError("note snapshot unavailable".into()));
        }
        #[derive(Deserialize)]
        struct Snapshot {
            seq: u64,
            text: String,
        }
        let snapshot: Snapshot = response.json().await?;
        if snapshot.seq > 0 {
            return Ok(Some(snapshot.text));
        }
        // An existing cold note may predate this index. Git is the portable
        // rebuild source, never written back into the note's edit log here.
        crate::git::WorkerGitHubClient::new(self.env.secret("GITHUB_TOKEN")?.to_string())
            .get_file_content(&crate::runtime::note_git_target(&self.env, id)?)
            .await
            .map_err(|error| Error::RustError(error.to_string()))
    }
    async fn process(&self, id: &str, job: &Job) -> Result<()> {
        let source = self.source(id).await?;
        if !self.current(id, job).await? {
            return Ok(());
        }
        let key = format!("{PREFIX}{id}");
        let Some(source) = source else {
            self.state.storage().delete(&key).await?;
            self.state
                .storage()
                .delete(&format!("connections:{id}"))
                .await?;
            self.changed().await?;
            return Ok(());
        };
        let old: Option<Entry> = self.state.storage().get(&key).await?;
        let mut entry = Entry::from_note(id, &source, &model(&self.env), old.as_ref());
        self.state.storage().put(&key, &entry).await?;
        self.changed().await?;
        if entry.needs_embedding() {
            if let Some(client) = embedding_client(&self.env) {
                let _ = vault::derive(&mut entry, &source, &client)
                    .await
                    .map_err(|error| Error::RustError(error.to_string()))?;
                if client.model_id() != model(&self.env) {
                    return Ok(());
                }
                // Merge only the matching artifact, never an old metadata
                // snapshot. Keep a newer refresh's stale flag and job. If
                // that refresh proves cosmetic, it can reuse this vector.
                let current: Option<Entry> = self.state.storage().get(&key).await?;
                if let (Some(mut current), Some(vector)) = (current, entry.vector.take()) {
                    if current.apply_embedding(&entry.key, vector) {
                        self.state.storage().put(&key, &current).await?;
                        self.changed().await?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl DurableObject for VaultDurableObject {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }
    async fn fetch(&self, req: Request) -> Result<Response> {
        let id = req
            .url()?
            .query_pairs()
            .find(|(key, _)| key == "id")
            .map(|(_, value)| value.into_owned());
        match req.path().as_str() {
            "/refresh" => {
                let Some(id) = id.filter(|id| crate::route::is_valid_note_id(id)) else {
                    return Response::error("invalid id", 400);
                };
                self.enqueue(&id, true).await?;
                Response::ok("queued")
            }
            "/rebuild" => {
                let client = crate::git::WorkerGitHubClient::new(
                    self.env.secret("GITHUB_TOKEN")?.to_string(),
                );
                let paths = client
                    .list_note_paths(
                        &self.env.var("GITHUB_OWNER")?.to_string(),
                        &self.env.var("GITHUB_REPO")?.to_string(),
                        &self.env.var("GITHUB_BRANCH")?.to_string(),
                    )
                    .await
                    .map_err(|error| Error::RustError(error.to_string()))?;
                let mut ids: BTreeSet<_> = paths.into_iter().collect();
                ids.extend(self.entries().await?.into_keys());
                let discovered = ids.len();
                ids.retain(|id| crate::route::is_valid_note_id(id));
                let skipped_invalid_ids = discovered - ids.len();
                for id in &ids {
                    self.enqueue(id, true).await?;
                }
                Response::from_json(
                    &serde_json::json!({"queued": ids.len(), "skipped_invalid_ids": skipped_invalid_ids}),
                )
            }
            "/index" | "/connections" => {
                let generation = self.generation().await?;
                let mut entries = self.entries().await?;
                let desired = model(&self.env);
                // Config/model changes invalidate old vectors without a
                // migration. Recompute on demand through the same queue.
                let enabled = embedding_client(&self.env).is_some();
                for entry in entries.values() {
                    if entry.stale
                        || (enabled
                            && entry.has_body
                            && (entry.key.model_id != desired || entry.needs_embedding()))
                    {
                        self.enqueue(&entry.id, false).await?;
                    }
                }
                let pending: BTreeSet<_> = self
                    .pending()
                    .await?
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect();
                for entry in entries
                    .values_mut()
                    .filter(|entry| pending.contains(&entry.id) || entry.stale)
                {
                    entry.stale = true;
                    entry.vector = None;
                }
                if self.generation().await? != generation {
                    return Response::error("index changed; retry shortly", 409);
                }
                if req.path() == "/index" {
                    return Response::from_json(&entries.values().map(|entry| serde_json::json!({
                        "id":entry.id, "title":entry.title, "tags":entry.tags, "aliases":entry.aliases, "key":entry.key,
                        "source_stale":entry.stale,
                        "embedding_fresh": entry.vector.as_deref().is_some_and(crate::embeddings::valid_vector)
                            && entry.key.model_id == desired,
                    })).collect::<Vec<_>>());
                }
                let Some(id) = id else {
                    return Response::error("id required", 400);
                };
                let cache_key = format!("connections:{id}");
                if let Some(cache) = self.state.storage().get::<Cached>(&cache_key).await? {
                    if cache.generation == generation && cache.model == desired {
                        return Response::from_json(&cache.connections);
                    }
                }
                let Some(connections) = vault::connections(&id, &entries, &desired, 5) else {
                    return Response::error("not indexed", 404);
                };
                self.state
                    .storage()
                    .put(
                        &cache_key,
                        Cached {
                            generation,
                            model: desired,
                            connections: connections.clone(),
                        },
                    )
                    .await?;
                Response::from_json(&connections)
            }
            _ => Response::error("not found", 404),
        }
    }
    async fn alarm(&self) -> Result<Response> {
        let jobs = self.pending().await?;
        // A crash/eviction during a remote read must not strand durable
        // work after the platform's finite automatic retry sequence.
        if !jobs.is_empty() {
            self.state.storage().set_alarm(RETRY).await?;
        }
        let mut retry = false;
        for (id, mut job) in jobs.iter().take(8).cloned() {
            let result = self.process(&id, &job).await;
            if !self.current(&id, &job).await? {
                continue;
            }
            if let Err(error) = result {
                console_log!("vault derivation failed for {id}: {error}");
                job.attempts += 1;
                if job.attempts < 3 {
                    self.state
                        .storage()
                        .put(&format!("{PENDING}{id}"), job)
                        .await?;
                    retry = true;
                    continue;
                }
            }
            self.state
                .storage()
                .delete(&format!("{PENDING}{id}"))
                .await?;
            self.changed().await?;
        }
        if !self.pending().await?.is_empty() {
            self.state
                .storage()
                .set_alarm(if retry { RETRY } else { Duration::from_secs(1) })
                .await?;
        }
        Response::ok("indexed")
    }
}
