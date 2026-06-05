//! Wasm-only Worker runtime: the `fetch` entrypoint that forwards a
//! websocket upgrade to the per-note Durable Object, and the
//! `NoteDurableObject` itself. Cfg-gated to `wasm32` because everything
//! here depends on `worker` runtime types that only exist on the
//! Cloudflare Worker target; native `cargo test` exercises the pure
//! `route` and `state` modules instead (mirrors the `pollen-alert` split).

use notes_protocol::{ClientMsg, Delta, Seq, ServerMsg};
use serde::{Deserialize, Serialize};
// `wasm_bindgen` is re-exported from `worker` and must be in scope: the
// `#[durable_object]` macro expands to JS-glue code that references it by
// bare name (see the workers-rs `counter.rs` example).
use worker::{
    console_log, durable_object, event, wasm_bindgen, Context, DurableObject, Env, Error, Request,
    Response, ResponseBuilder, Result, State, WebSocket, WebSocketIncomingMessage, WebSocketPair,
};

use crate::route::parse_ws_note_id;
use crate::state::is_stale;

/// Top-level Worker entrypoint. A websocket upgrade for `/note/<id>/ws`
/// is forwarded to that note's Durable Object (`idFromName(id)`), which
/// is the single writer for the note; every other path gets a plain
/// hello response.
#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    if let Some(note_id) = parse_ws_note_id(&req.path()) {
        let stub = env
            .durable_object("NOTE")?
            .id_from_name(note_id)?
            .get_stub()?;
        return stub.fetch_with_request(req).await;
    }

    Response::ok("notes-sync: ok")
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
    // Unused until phase 3 (alarms read GitHub-commit config off the env).
    #[allow(dead_code)]
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

    async fn websocket_close(
        &self,
        _ws: WebSocket,
        _code: usize,
        _reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        Ok(())
    }

    async fn websocket_error(&self, _ws: WebSocket, _error: Error) -> Result<()> {
        Ok(())
    }
}
