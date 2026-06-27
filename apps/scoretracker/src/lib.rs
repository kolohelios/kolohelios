// Non-test code must not `.unwrap()`; `not(test)` exempts unit tests, and
// integration tests compile as separate crates (no attribute).
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

//! scoretracker — a public, config-driven card-game score tracker served as
//! a Cloudflare Worker at `scoretracker.kolohelios.com`.
//!
//! Each game is a static config (`games/*.cue`, embedded at build via
//! [`engine::config`]); the engine is generic over a *scoring model*
//! (`roundPoints` for Phase 10 / tic, `matchWins` for Skip-Bo), so adding a
//! game is a config edit, not engine code. One `GameState` Durable Object
//! per `(gameType, instanceId)` holds the authoritative state; totals are
//! recomputed server-side on every mutation.

pub mod engine;

#[cfg(target_arch = "wasm32")]
mod runtime;
