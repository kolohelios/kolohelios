//! Wasm-only Worker runtime: the `fetch` entrypoint that forwards a
//! websocket upgrade to the per-note Durable Object, and the
//! `NoteDurableObject` itself. Cfg-gated to `wasm32` because everything
//! here depends on `worker` runtime types that only exist on the
//! Cloudflare Worker target; native `cargo test` exercises the pure
//! `route` module instead (mirrors the `pollen-alert` split).

use serde::{Deserialize, Serialize};
// `wasm_bindgen` is re-exported from `worker` and must be in scope: the
// `#[durable_object]` macro expands to JS-glue code that references it by
// bare name (see the workers-rs `counter.rs` example).
use worker::{
    console_log, durable_object, event, wasm_bindgen, Context, DurableObject, Env, Error, Request,
    Response, ResponseBuilder, Result, State, WebSocket, WebSocketIncomingMessage, WebSocketPair,
};

use crate::route::parse_ws_note_id;

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

/// One Durable Object per note. The hibernation-critical invariant: all
/// durable state lives in `state.storage()` (or the socket attachment),
/// never in `self` fields, so an evicted-and-rebuilt object replays
/// losslessly. The append-only edit log lands here in phase 2; phase 1
/// keeps a single `seq` counter to prove storage survives eviction.
#[durable_object]
pub struct NoteDurableObject {
    state: State,
    #[allow(dead_code)]
    env: Env,
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
        let attachment: SocketAttachment = ws
            .deserialize_attachment()?
            .ok_or_else(|| Error::RustError("socket missing attachment".into()))?;

        // Storage-backed counter — read fresh from the durable tier each
        // message so the value survives an eviction between messages.
        let mut seq: u64 = self.state.storage().get("seq").await?.unwrap_or(0);
        seq += 1;
        self.state.storage().put("seq", seq).await?;

        let text = match message {
            WebSocketIncomingMessage::String(s) => s,
            WebSocketIncomingMessage::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
        };

        ws.send_with_str(format!("echo[{}#{seq}]: {text}", attachment.note_id))?;
        Ok(())
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
