// Non-test code must not `.unwrap()`; `not(test)` exempts unit tests,
// and integration tests compile as separate crates (no attribute).
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

//! notes-web — the front-end Worker for the note app. The HTMX shell and
//! the Rust-WASM editor bundle are served as Workers Static Assets
//! (`[assets]` in `wrangler.toml`); Cloudflare serves matched paths at
//! the edge and only unmatched paths fall through to this Worker, which
//! is a 404 stub (the same pattern as the other asset-serving apps).
//!
//! The live document surface is `apps/notes-editor` (browser-wasm); the
//! editing/sync logic runs entirely there over a WebSocket to the
//! `notes-sync` Durable Object. This Worker carries no dynamic logic yet.

use worker::{event, Context, Env, Request, Response, Result};

#[event(fetch)]
async fn fetch(_req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    Response::error("Not Found", 404)
}
