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
//! (see [`state`]).
//!
//! Phase 3 (issue #764) adds the cold tier: edits keep persisting to DO
//! storage synchronously, while the note body is committed to git lazily
//! — a debounce + backstop pair multiplexed onto the DO's single alarm,
//! plus a commit on last-socket-disconnect, landing through the
//! optimistic-retry [`git`] client.
//!
//! Phase 4 adds sign-in: ATProto OAuth, authentication only. The
//! native-tested security core lives in [`auth`] (the `sub`-vs-DID check,
//! PKCE, the signed session cookie that gates the WS) and [`dpop`] (the
//! ES256 DPoP proofs); the [`oauth`] module is the hand-rolled HTTP flow —
//! the client-metadata document, resolution, PAR, the token exchange with
//! DPoP nonce-retry, and the per-flow `AuthFlow` Durable Object — which
//! verifies the DID, discards the ATProto tokens, and mints the cookie.

pub mod auth;
pub mod dpop;
pub mod git;
pub mod oauth;
pub mod route;
pub mod state;

#[cfg(target_arch = "wasm32")]
mod runtime;
