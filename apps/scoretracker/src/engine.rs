//! The pure scoring engine — game configs, free-text parsing, scoring, and
//! the state mutations. No `worker` dependency, so it compiles natively and
//! is exercised by `cargo test`; the Durable Object (the wasm-only
//! `runtime` module) is a thin shell over it.

pub mod config;
pub mod error;
pub mod parse;
pub mod score;
pub mod state;
