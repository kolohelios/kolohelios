// Non-test code must not `.unwrap()`; `not(test)` exempts unit tests,
// and integration tests compile as separate crates (no attribute).
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

//! notes-editor — the live document surface and socket client for the
//! note app, compiled to browser-wasm. The pure [`client`] logic (the
//! document state and protocol decisions, native-tested) is wired to a
//! `<textarea>` and a `WebSocket` by the wasm-only `dom` entrypoint. The
//! editor and the Durable Object share the `notes-protocol` types, so the
//! two ends serialize the same bytes.
//!
//! v1 input surface is a `<textarea>` and the edit delta is the whole
//! body (the always-correct fallback); the splice/CRDT path can slot in
//! behind [`client::ClientState::local_edit`] without touching the
//! protocol.

pub mod client;

#[cfg(target_arch = "wasm32")]
mod dom;
