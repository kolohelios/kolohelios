//! Thin client for OpenRouter's chat completions endpoint.
//!
//! Credentials flow in via the `OPENROUTER_API_KEY` environment variable so
//! `blogctl` stays ignorant of 1Password — invokers wrap calls in
//! `op run --env-file=.env -- ...` per the repo-wide credential pattern.

use std::env;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const DEFAULT_ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";
const API_KEY_VAR: &str = "OPENROUTER_API_KEY";
const API_BASE_VAR: &str = "OPENROUTER_API_BASE";

/// Resolve the chat-completions endpoint. Production reads the default;
/// tests set `OPENROUTER_API_BASE` to a `mockito` server URL so the
/// full HTTP path is exercised without depending on the live service.
fn endpoint() -> String {
    env::var(API_BASE_VAR).unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string())
}

pub const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4-6";

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
}

#[derive(Debug, Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ReceivedMessage,
}

#[derive(Debug, Deserialize)]
struct ReceivedMessage {
    content: String,
}

/// Send a one-shot prompt to OpenRouter and return the model's reply text.
pub fn chat(prompt: &str, model: &str) -> Result<String> {
    let api_key = env::var(API_KEY_VAR).map_err(|_| Error::OpenrouterApiKeyMissing)?;

    let request = ChatRequest {
        model,
        messages: vec![Message {
            role: "user",
            content: prompt,
        }],
    };

    // ChatRequest fields are all serializable primitives — to_value cannot fail.
    let body = serde_json::to_value(&request).expect("ChatRequest is serializable");

    let response = ureq::post(&endpoint())
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| Error::OpenrouterRequest {
            message: e.to_string(),
        })?;

    let parsed: ChatResponse = response.into_json().map_err(|e| Error::OpenrouterRequest {
        message: format!("could not parse response: {e}"),
    })?;

    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or(Error::OpenrouterEmptyResponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_body_serializes_to_openrouter_shape() {
        let req = ChatRequest {
            model: "anthropic/claude-sonnet-4-6",
            messages: vec![Message {
                role: "user",
                content: "hello",
            }],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "model": "anthropic/claude-sonnet-4-6",
                "messages": [{"role": "user", "content": "hello"}],
            })
        );
    }

    #[test]
    fn response_extracts_first_choice_content() {
        let raw = serde_json::json!({
            "choices": [
                {"message": {"role": "assistant", "content": "pong"}},
                {"message": {"role": "assistant", "content": "ignored"}},
            ]
        });
        let parsed: ChatResponse = serde_json::from_value(raw).unwrap();
        let content = parsed.choices.into_iter().next().map(|c| c.message.content);
        assert_eq!(content, Some("pong".to_string()));
    }

    // Both endpoint env tests mutate the same process-global var;
    // cargo runs unit tests in parallel by default, so without
    // serialization each test sees the other's half-step and asserts
    // inconsistently (observed as a flake under cargo-llvm-cov). A
    // module-local mutex held across the env-var dance is the lightest
    // guard that works.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn endpoint_defaults_to_production_url() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = env::var(API_BASE_VAR).ok();
        env::remove_var(API_BASE_VAR);
        assert_eq!(endpoint(), DEFAULT_ENDPOINT);
        if let Some(v) = prev {
            env::set_var(API_BASE_VAR, v);
        }
    }

    #[test]
    fn endpoint_honors_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = env::var(API_BASE_VAR).ok();
        env::set_var(API_BASE_VAR, "http://127.0.0.1:1234/v1/chat");
        assert_eq!(endpoint(), "http://127.0.0.1:1234/v1/chat");
        match prev {
            Some(v) => env::set_var(API_BASE_VAR, v),
            None => env::remove_var(API_BASE_VAR),
        }
    }

    #[test]
    fn empty_choices_is_caught() {
        let raw = serde_json::json!({"choices": []});
        let parsed: ChatResponse = serde_json::from_value(raw).unwrap();
        assert!(parsed.choices.is_empty());
    }

    /// Live integration test against the real OpenRouter API. Silently
    /// no-ops in environments without credentials (CI, fresh checkouts)
    /// so the suite stays green there; runs end-to-end locally when
    /// invoked through `op run --env-file=.env -- cargo test`.
    #[test]
    fn live_chat_returns_nonempty_reply() {
        if std::env::var(API_KEY_VAR).is_err() {
            return;
        }
        let reply = chat("Reply with the single word: pong.", DEFAULT_MODEL)
            .expect("live OpenRouter call should succeed");
        assert!(!reply.is_empty(), "reply should not be empty");
    }
}
