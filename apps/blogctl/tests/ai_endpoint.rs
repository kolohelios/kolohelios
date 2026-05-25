//! Exercises the `OPENROUTER_API_BASE` seam end-to-end: spin up a
//! `mockito` server, point blogctl at it, call `commands::ai::ping`,
//! confirm the request hit the mock and the reply text round-trips.
//!
//! This is what justifies the seam from commit 1. Production never
//! sets the env var; the test does, so the full HTTP path —
//! `ChatRequest` serialization, request firing, response parsing —
//! runs without depending on real OpenRouter.

use std::env;
use std::sync::Mutex;

use blogctl::commands;

// `cargo test` runs tests in parallel by default; both env-touching
// tests below have to serialize to avoid stepping on each other. A
// process-level mutex held across the env-var dance is the simplest
// guard.
static ENV_LOCK: Mutex<()> = Mutex::new(());

const CANNED_OPENROUTER_RESPONSE: &str = r#"{
  "id": "gen-test-001",
  "object": "chat.completion",
  "model": "test-model",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "pong from the mock"
      },
      "finish_reason": "stop"
    }
  ]
}"#;

#[test]
fn ping_hits_endpoint_via_env_override() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(CANNED_OPENROUTER_RESPONSE)
        .create();

    let prev_base = env::var("OPENROUTER_API_BASE").ok();
    let prev_key = env::var("OPENROUTER_API_KEY").ok();
    env::set_var(
        "OPENROUTER_API_BASE",
        format!("{}/chat/completions", server.url()),
    );
    env::set_var("OPENROUTER_API_KEY", "test-key");

    let result = commands::ai::ping("hello".into(), "test-model".into());

    // Restore env before asserting so a panic doesn't poison subsequent
    // tests.
    match prev_base {
        Some(v) => env::set_var("OPENROUTER_API_BASE", v),
        None => env::remove_var("OPENROUTER_API_BASE"),
    }
    match prev_key {
        Some(v) => env::set_var("OPENROUTER_API_KEY", v),
        None => env::remove_var("OPENROUTER_API_KEY"),
    }

    result.expect("ping should succeed against mock");
    mock.assert();
}

#[test]
fn openrouter_api_key_missing_surfaces_clean_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let prev_key = env::var("OPENROUTER_API_KEY").ok();
    env::remove_var("OPENROUTER_API_KEY");

    let result = commands::ai::ping("hello".into(), "test-model".into());

    if let Some(v) = prev_key {
        env::set_var("OPENROUTER_API_KEY", v);
    }

    let err = result.expect_err("missing API key must error");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("openrouter") && msg.contains("api") && msg.contains("key"),
        "error should mention the missing key; got: {err}"
    );
}
