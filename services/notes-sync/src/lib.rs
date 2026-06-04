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
//! attachment. The append-only edit log, two-alarm persistence, lazy
//! GitHub commit, and auth land in later phases.

pub mod route;

#[cfg(target_arch = "wasm32")]
mod runtime;
