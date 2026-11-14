//! Vault HTTP operations and thin source/text adapters.
use super::*;
use crate::{
    drift::{self, Reason, Snapshot},
    search, webhook,
};
use futures_util::StreamExt;

pub(super) async fn read_body(req: &mut Request, limit: usize) -> Result<Option<Vec<u8>>> {
    if req
        .headers()
        .get("Content-Length")?
        .and_then(|v| v.parse::<usize>().ok())
        .is_some_and(|n| n > limit)
    {
        return Ok(None);
    }
    let mut stream = match req.stream() {
        Ok(stream) => stream,
        Err(_) => return Ok(Some(Vec::new())),
    };
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len() + chunk.len() > limit {
            return Ok(None);
        }
        body.extend(chunk);
    }
    Ok(Some(body))
}

pub async fn handle_webhook(mut req: Request, env: &Env) -> Result<Response> {
    if req.method() != Method::Post {
        return Response::error("method not allowed", 405);
    }
    let Ok(secret) = env.secret("SESSION_SECRET") else {
        return Response::error("webhook not configured", 503);
    };
    let signature = req
        .headers()
        .get("X-Hub-Signature-256")?
        .unwrap_or_default();
    if signature.len() != 71 {
        return Response::error("invalid signature", 401);
    }
    let Some(body) = read_body(&mut req, 2 * 1024 * 1024).await? else {
        return Response::error("payload too large", 413);
    };
    if !webhook::valid_signature(secret.to_string().as_bytes(), &body, &signature) {
        return Response::error("invalid signature", 401);
    }
    let event = req.headers().get("X-GitHub-Event")?.unwrap_or_default();
    if event == "ping" {
        return Response::ok("pong");
    }
    if event != "push" {
        return Response::ok("ignored");
    }
    let repository = format!("{}/{}", env.var("GITHUB_OWNER")?, env.var("GITHUB_REPO")?);
    let changes = match webhook::changes(&body, &repository, &env.var("GITHUB_BRANCH")?.to_string())
    {
        Ok(changes) => changes,
        Err(_) => return Response::error("invalid repository or push payload", 400),
    };
    // A single durable request acknowledges large pushes promptly. Discovery
    // runs under an alarm, not GitHub's webhook response deadline.
    match changes {
        webhook::Changes::Ignore => Response::ok("ignored"),
        webhook::Changes::Rebuild => forward(env, "/request-reindex", None, None).await,
        webhook::Changes::Notes(ids) if ids.len() > 32 => {
            forward(env, "/request-reindex", None, None).await
        }
        webhook::Changes::Notes(ids) => {
            forward(
                env,
                "/git-refresh",
                None,
                Some(serde_json::to_string(&ids)?),
            )
            .await
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct VerifyRequest {
    #[serde(default)]
    repair: bool,
    after: Option<String>,
}
#[derive(Serialize)]
struct Row {
    id: String,
    reasons: Vec<Reason>,
    repairable: bool,
    queued: bool,
}

impl VaultDurableObject {
    fn github(&self) -> Result<crate::git::WorkerGitHubClient> {
        Ok(crate::git::WorkerGitHubClient::new(
            self.env.secret("GITHUB_TOKEN")?.to_string(),
        ))
    }
    async fn blobs(&self) -> Result<BTreeMap<String, String>> {
        self.github()?
            .list_note_blobs(
                &self.env.var("GITHUB_OWNER")?.to_string(),
                &self.env.var("GITHUB_REPO")?.to_string(),
                &self.env.var("GITHUB_BRANCH")?.to_string(),
            )
            .await
            .map_err(|e| Error::RustError(e.to_string()))
    }
    async fn git_text(&self, id: &str) -> Result<Option<String>> {
        let text = self
            .github()?
            .get_file_content(&crate::runtime::note_git_target(&self.env, id)?)
            .await
            .map_err(|e| Error::RustError(e.to_string()))?;
        if text.is_none() {
            self.blobs().await?;
        } // Missing branch != deleted note.
        Ok(text)
    }
    async fn note_snapshot(&self, id: &str) -> Result<Snapshot> {
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
        response.json().await
    }
    pub(super) async fn source(&self, id: &str, git: bool) -> Result<(Option<String>, bool)> {
        let mut snapshot = self.note_snapshot(id).await?;
        if !git {
            if snapshot.deleted {
                return Ok((None, false));
            }
            if snapshot.seq > 0 {
                return Ok((Some(snapshot.text), false));
            }
            return Ok((self.git_text(id).await?, false));
        }
        let text = self.git_text(id).await?;
        match drift::reconcile(&snapshot, text.as_deref(), 0) {
            drift::Reconcile::Conflict => {
                return Ok(((!snapshot.deleted).then_some(snapshot.text), true))
            }
            drift::Reconcile::Unchanged => {}
            drift::Reconcile::Apply { .. } => {
                let headers = worker::Headers::new();
                headers.set("X-Note-Id", id)?;
                let mut init = RequestInit::new();
                init.with_method(Method::Post)
                    .with_headers(headers)
                    .with_body(Some(
                        serde_json::to_string(&drift::Import {
                            expected: snapshot,
                            text,
                        })?
                        .into(),
                    ));
                let request = Request::new_with_init(
                    &format!("https://note{}", crate::runtime::RECONCILE_PATH),
                    &init,
                )?;
                let stub = self
                    .env
                    .durable_object("NOTE")?
                    .id_from_name(id)?
                    .get_stub()?;
                let mut response = stub.fetch_with_request(request).await?;
                if response.status_code() == 409 {
                    snapshot = self.note_snapshot(id).await?;
                    return Ok(((!snapshot.deleted).then_some(snapshot.text), true));
                }
                if response.status_code() != 200 {
                    return Err(Error::RustError("Git import failed".into()));
                }
                snapshot = response.json().await?;
            }
        }
        Ok(((!snapshot.deleted).then_some(snapshot.text), false))
    }
    pub(super) async fn read_text(&self, entry: &Entry) -> Result<Option<String>> {
        if entry.text_hash.as_ref() != Some(&entry.key.content_hash) {
            return Ok(None);
        }
        let mut text = String::new();
        for index in 0..entry.text_chunks {
            let Some(chunk) = self
                .state
                .storage()
                .get::<String>(&format!("text:{}:{index:06}", entry.id))
                .await?
            else {
                return Ok(None);
            };
            text.push_str(&chunk);
        }
        Ok(drift::text_matches(entry, &text).then_some(text))
    }
    pub(super) async fn store_text(
        &self,
        id: &str,
        note: &str,
        old: Option<&Entry>,
        entry: &mut Entry,
    ) -> Result<()> {
        let chunks = drift::text_chunks(note);
        let canonical = notes_protocol::frontmatter::canonical_body(note);
        let old_text = match old {
            Some(old) => self.read_text(old).await?,
            None => None,
        };
        if old_text.as_deref() != Some(&canonical)
            || old.is_none_or(|old| old.id != id || old.text_chunks != chunks.len())
        {
            for (index, chunk) in chunks.iter().enumerate() {
                self.state
                    .storage()
                    .put(&format!("text:{id}:{index:06}"), chunk)
                    .await?;
            }
            for index in chunks.len()..old.map_or(0, |old| old.text_chunks) {
                self.state
                    .storage()
                    .delete(&format!("text:{id}:{index:06}"))
                    .await?;
            }
        }
        entry.text_chunks = chunks.len();
        entry.text_hash = Some(entry.key.content_hash.clone());
        Ok(())
    }
    pub(super) async fn remove_entry(&self, id: &str) -> Result<()> {
        if let Some(entry) = self
            .state
            .storage()
            .get::<Entry>(&format!("{PREFIX}{id}"))
            .await?
        {
            for index in 0..entry.text_chunks {
                self.state
                    .storage()
                    .delete(&format!("text:{id}:{index:06}"))
                    .await?;
            }
        }
        self.state
            .storage()
            .delete(&format!("{PREFIX}{id}"))
            .await?;
        self.state
            .storage()
            .delete(&format!("connections:{id}"))
            .await?;
        self.changed().await
    }
    pub(super) async fn discover(&self) -> Result<()> {
        let mut ids: BTreeSet<_> = self.blobs().await?.into_keys().collect();
        ids.extend(self.entries().await?.into_keys());
        let discovered = ids.len();
        ids.retain(|id| crate::route::is_valid_note_id(id));
        for id in &ids {
            self.enqueue(id, true, true).await?;
        }
        self.state.storage().put("last_discovery", serde_json::json!({"queued":ids.len(), "skipped_invalid_ids":discovered - ids.len()})).await
    }
    pub(super) async fn verify(&self, req: &mut Request) -> Result<Response> {
        let options: VerifyRequest = match req.json().await {
            Ok(options) => options,
            Err(_) => return Response::error("invalid verify request", 400),
        };
        let blobs = self.blobs().await?;
        // Merge a small KV page with the Git/pending registries. Never load
        // every embedding just to paginate a verification scan.
        let after = options.after.as_deref().unwrap_or_default();
        let page = self
            .state
            .storage()
            .list_with_options(
                ListOptions::new()
                    .prefix(PREFIX)
                    .start(&format!("{PREFIX}{after}"))
                    .limit(22),
            )
            .await?;
        let mut ids: BTreeSet<_> = blobs
            .keys()
            .filter(|id| id.as_str() > after)
            .cloned()
            .collect();
        for pair in page.entries() {
            let pair = js_sys::Array::from(&pair?);
            let key = pair
                .get(0)
                .as_string()
                .ok_or_else(|| Error::RustError("invalid index key".into()))?;
            if let Some(id) = key.strip_prefix(PREFIX).filter(|id| *id > after) {
                ids.insert(id.to_owned());
            }
        }
        let pending: BTreeSet<_> = self
            .pending()
            .await?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        ids.extend(pending.iter().filter(|id| id.as_str() > after).cloned());
        let mut ids: Vec<_> = ids.into_iter().take(21).collect();
        let next = (ids.len() > 20).then(|| ids[19].clone());
        ids.truncate(20);
        let mut rows = Vec::new();
        for id in ids {
            if !crate::route::is_valid_note_id(&id) {
                rows.push(Row {
                    id,
                    reasons: vec![Reason::UnsupportedId],
                    repairable: false,
                    queued: false,
                });
                continue;
            }
            let entry: Option<Entry> = self.state.storage().get(&format!("{PREFIX}{id}")).await?;
            let text = match &entry {
                Some(entry) => self.read_text(entry).await?,
                None => None,
            };
            let snapshot = match self.note_snapshot(&id).await {
                Ok(snapshot) => snapshot,
                Err(_) => {
                    rows.push(Row {
                        id,
                        reasons: vec![Reason::SourceUnavailable],
                        repairable: false,
                        queued: false,
                    });
                    continue;
                }
            };
            let git_mismatch = if snapshot.deleted {
                blobs.contains_key(&id)
            } else {
                snapshot.seq > 0 && blobs.get(&id) != Some(&drift::git_blob_hash(&snapshot.text))
            };
            // Ordinary uncommitted edits legitimately lead the cold tier.
            // A clean mirror mismatch, or a push-detected conflict, does not.
            let cold_conflict = git_mismatch
                && (snapshot.seq == snapshot.committed_seq
                    || entry.as_ref().is_some_and(|entry| entry.git_conflict));
            let note = if snapshot.deleted {
                None
            } else if snapshot.seq > 0 {
                Some(snapshot.text.clone())
            } else if blobs.contains_key(&id) {
                match self.git_text(&id).await {
                    Ok(text) => text,
                    Err(_) => {
                        rows.push(Row {
                            id,
                            reasons: vec![Reason::SourceUnavailable],
                            repairable: false,
                            queued: false,
                        });
                        continue;
                    }
                }
            } else {
                None
            };
            let mut reasons = drift::reasons(
                &id,
                note.as_deref(),
                entry.as_ref(),
                text.as_deref(),
                &model(&self.env),
                pending.contains(&id),
            );
            if cold_conflict && !reasons.contains(&Reason::GitConflict) {
                reasons.push(Reason::GitConflict);
            }
            let can_import = !snapshot.leased
                && snapshot.seq == snapshot.committed_seq
                && snapshot.active_sockets == 0;
            let repairable = !reasons.is_empty() && (!cold_conflict || can_import);
            let queue = options.repair
                && !reasons.is_empty()
                && (repairable || reasons.iter().any(|r| *r != Reason::GitConflict));
            if queue {
                self.enqueue(
                    &id,
                    false,
                    reasons.contains(&Reason::GitConflict) && (!cold_conflict || can_import),
                )
                .await?;
            }
            rows.push(Row {
                id,
                reasons,
                repairable,
                queued: queue,
            });
        }
        Response::from_json(
            &serde_json::json!({"clean":rows.iter().all(|row| row.reasons.is_empty()), "rows":rows, "next_after":next, "model_id":model(&self.env)}),
        )
    }
    pub(super) async fn search(&self, req: &mut Request) -> Result<Response> {
        let query: search::Query = match req.json().await {
            Ok(query) => query,
            Err(_) => return Response::error("invalid search request", 400),
        };
        if !query.valid() {
            return Response::error("query/limit out of range", 400);
        }
        let desired = model(&self.env);
        let vector = if query.mode == search::Mode::Semantic {
            let Some(client) = embedding_client(&self.env) else {
                return Response::error("embeddings not configured", 503);
            };
            let input = crate::embeddings::chunks(query.query.trim())
                .map_err(|e| Error::RustError(e.to_string()))?;
            let vectors = client
                .embed(&input)
                .await
                .map_err(|_| Error::RustError("query embedding unavailable".into()))?;
            Some(
                crate::embeddings::pool(&input, &vectors)
                    .map_err(|e| Error::RustError(e.to_string()))?,
            )
        } else {
            None
        };
        let generation = self.generation().await?;
        let mut entries = self.entries().await?;
        let pending: BTreeSet<_> = self
            .pending()
            .await?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let mut hits = Vec::new();
        for entry in entries.values_mut() {
            if pending.contains(&entry.id) {
                entry.stale = true;
            }
            if entry.stale
                || entry.text_hash.is_none()
                || (query.mode == search::Mode::Semantic
                    && (entry.key.model_id != desired || entry.needs_embedding()))
            {
                self.enqueue(&entry.id, false, false).await?;
                continue;
            }
            if let Some(text) = self.read_text(entry).await? {
                // Keep only excerpts/scores, not the entire vault's prose
                // alongside all vectors in the isolate's limited memory.
                hits.extend(search::rank(
                    &query,
                    &[(entry.clone(), text)],
                    &desired,
                    vector.as_deref(),
                ));
            } else {
                self.enqueue(&entry.id, false, false).await?;
            }
        }
        if self.generation().await? != generation {
            return Response::error("index changed; retry shortly", 409);
        }
        Response::from_json(
            &serde_json::json!({"hits":search::finish(hits, query.limit), "model_id":desired, "pending":pending.len()}),
        )
    }
}
