//! notes-sync — Cloudflare Worker + per-note Durable Object backing the
//! live-synced note app. Each note maps to one Durable Object via
//! `idFromName(noteId)`; while a note is open that DO is the single
//! writer and source of truth.
//!
//! Phase 1 scope (issue #757): a hello-world hibernatable WebSocket echo
//! that de-risks the Durable Object hibernation lifecycle on the `worker`
//! crate — accept via the hibernation API, survive eviction by reading
//! durable state from DO storage (not in-memory fields) on wake, and
//! carry per-connection state across hibernation via the socket
//! attachment.
//!
//! Phase 2 (issue #763) adds the append-only edit log: the editor speaks
//! the `notes-protocol` wire types over the socket, accepted edits are
//! appended to DO storage, and the body rehydrates by replaying the log
//! (see [`state`]). Two-alarm persistence, the lazy GitHub commit, and
//! auth land in later phases.

pub mod route;
pub mod state;

#[cfg(target_arch = "wasm32")]
mod runtime;
