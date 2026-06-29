//! OpenRouter (OpenAI-compatible) chat-completions client and the note
//! autonaming it feeds.
//!
//! Mirrors the shape of [`crate::git`]: a pure, native-testable core
//! (request/response serde, the title-derivation and cleanup helpers, the
//! autoname gate) behind a [`ChatCompletions`] trait, with the real
//! `worker::Fetch` transport (`WorkerOpenRouterClient`) gated to
//! `wasm32`. Native `cargo test` exercises the wire shapes against a
//! `mockito` server and drives the title helpers directly; the wasm path
//! is build-checked.
//!
//! The request/response serde is ported from `apps/blogctl/src/openrouter.rs`
//! — the same OpenAI chat-completions shape — but the transport is the
//! worker `Fetch` client rather than blocking `ureq`, which does not
//! compile on `wasm32`.

use serde::Deserialize;
#[cfg(any(target_arch = "wasm32", test))]
use serde::Serialize;
use snafu::{ResultExt, Snafu};

/// Production chat-completions endpoint.
pub const DEFAULT_ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";

/// Default model used for autonaming when none is configured (override via
/// the `OPENROUTER_MODEL` worker var). Matches `blogctl`'s default so the
/// repo speaks to one model family.
pub const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4-6";

/// Cap the body slice sent for titling — a title only needs the opening of
/// a note, and a giant note would otherwise blow the prompt budget.
const MAX_PROMPT_CHARS: usize = 4000;

/// A derived title is clamped to this many words so it stays a label, not a
/// sentence.
const MAX_TITLE_WORDS: usize = 6;

/// Instruction steering the model to emit a bare title and nothing else, so
/// [`clean_title`] has little to strip.
const TITLE_SYSTEM_PROMPT: &str = "You generate a concise title for a note. \
Reply with ONLY the title: at most 6 words, no surrounding quotes, no trailing \
punctuation, no markdown, plain text on a single line.";

/// Why a chat-completions call (or its parse) failed.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum LlmError {
    /// Transport or request-construction failure (no usable HTTP response).
    #[snafu(display("OpenRouter transport error: {message}"))]
    Transport { message: String },
    /// The endpoint returned a non-2xx status.
    #[snafu(display("OpenRouter returned status {status}"))]
    Status { status: u16 },
    /// The response body was not the expected chat-completions JSON.
    #[snafu(display("could not parse OpenRouter response: {source}"))]
    Parse { source: serde_json::Error },
    /// A 2xx response that carried no choices to read a reply from.
    #[snafu(display("OpenRouter returned no choices"))]
    EmptyResponse,
}

/// One chat message in an OpenAI-compatible request.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

/// An OpenAI-compatible chat-completions request body.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
}

/// The slice of a chat-completions response we read: the assistant text of
/// the first choice.
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

/// Build the chat-completions request body for a `system` + `user` message
/// pair. Pure so the wasm transport and the native test client serialize
/// the identical wire shape. Serialization cannot fail — every field is a
/// plain string.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn build_request_body(model: &str, system: &str, user: &str) -> serde_json::Value {
    let request = ChatRequest {
        model,
        messages: vec![
            Message {
                role: "system",
                content: system,
            },
            Message {
                role: "user",
                content: user,
            },
        ],
    };
    serde_json::to_value(&request).expect("ChatRequest is serializable")
}

/// Extract the first choice's assistant text from a chat-completions
/// response body. Pure, shared by the wasm transport and the native tests.
pub fn parse_response(json: &str) -> Result<String, LlmError> {
    let parsed: ChatResponse = serde_json::from_str(json).context(ParseSnafu)?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or(LlmError::EmptyResponse)
}

/// The chat-completions transport. Implemented by the wasm
/// `WorkerOpenRouterClient` and by a `ureq`/`mockito`-backed client in
/// native tests.
//
// `async fn` in a trait is fine here: the only consumers are the generic
// `derive_title` (static dispatch) and the wasm runtime, and we deliberately
// want no `Send` bound — the `worker::Fetch` futures are `!Send`.
#[allow(async_fn_in_trait)]
pub trait ChatCompletions {
    /// Send a `system` + `user` message pair and return the assistant's
    /// reply text.
    async fn complete(&self, system: &str, user: &str) -> Result<String, LlmError>;

    /// The model id this client calls — exposed so a caller can stamp a
    /// derived artifact (e.g. a title cache key) with the model that
    /// produced it.
    fn model_id(&self) -> &str;
}

/// Whether autonaming should run at all, given the configured API key. An
/// unset, empty, or whitespace-only key means OpenRouter is not provisioned
/// yet — a clean no-op so the worker ships and deploys *before* the key
/// lands in 1Password.
pub fn autoname_enabled(api_key: Option<&str>) -> bool {
    api_key.is_some_and(|k| !k.trim().is_empty())
}

/// Truncate `s` to at most `max` chars on a char boundary.
fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Normalize a raw model reply into a usable title, or `None` when nothing
/// usable remains. Defends against the model ignoring the instruction:
/// takes the first non-blank line, strips a leading markdown heading
/// marker, drops surrounding quotes/backticks and trailing punctuation, and
/// clamps to `MAX_TITLE_WORDS` words.
pub fn clean_title(raw: &str) -> Option<String> {
    let first_line = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    // A model that emits "# Heading" or "## Heading" — drop the markers.
    let s = first_line.trim_start_matches('#').trim();
    // Surrounding quotes/backticks the model may wrap the title in.
    let s = s.trim_matches(['"', '\'', '`']).trim();
    // Trailing sentence punctuation a title shouldn't carry.
    let s = s.trim_end_matches(['.', ',', ';', ':', '!', '?']).trim();
    let title: String = s
        .split_whitespace()
        .take(MAX_TITLE_WORDS)
        .collect::<Vec<_>>()
        .join(" ");
    (!title.is_empty()).then_some(title)
}

/// Derive a concise title from a note `body` by asking the model, then
/// cleaning the reply. Returns `Ok(None)` for a blank body (no call made)
/// or when the cleaned reply is empty.
pub async fn derive_title<C: ChatCompletions>(
    client: &C,
    body: &str,
) -> Result<Option<String>, LlmError> {
    if body.trim().is_empty() {
        return Ok(None);
    }
    let snippet = truncate(body, MAX_PROMPT_CHARS);
    let reply = client.complete(TITLE_SYSTEM_PROMPT, snippet).await?;
    Ok(clean_title(&reply))
}

/// Whether `note` warrants a (re)title. A blank body is never titled (there
/// is nothing to name). Otherwise `force` titles regardless of an existing
/// title (the on-demand retitle), while the lazy cadence only titles a note
/// that has none.
pub fn should_autoname(note: &str, force: bool) -> bool {
    let body = notes_protocol::frontmatter::body_without_frontmatter(note);
    if body.trim().is_empty() {
        return false;
    }
    if force {
        return true;
    }
    // A malformed front-matter block parses as an error; treat that as
    // "no usable title" so a note still gets named rather than wedging.
    match notes_protocol::frontmatter::parse(note) {
        Ok((fm, _)) => fm.title.is_none(),
        Err(_) => true,
    }
}

#[cfg(target_arch = "wasm32")]
pub use worker_client::WorkerOpenRouterClient;

/// Real OpenRouter client, wasm-only because it depends on `worker::Fetch`.
/// `derive_title` drives it through the [`ChatCompletions`] trait.
#[cfg(target_arch = "wasm32")]
mod worker_client {
    use super::{build_request_body, parse_response, ChatCompletions, LlmError, DEFAULT_ENDPOINT};
    use worker::{Fetch, Headers, Method, Request, RequestInit};

    /// Chat-completions client authenticated with a static API key held as
    /// a Wrangler secret (`OPENROUTER_API_KEY`).
    pub struct WorkerOpenRouterClient {
        api_key: String,
        model: String,
        endpoint: String,
    }

    impl WorkerOpenRouterClient {
        /// New client for `model`, hitting the production endpoint.
        pub fn new(api_key: String, model: String) -> Self {
            Self {
                api_key,
                model,
                endpoint: DEFAULT_ENDPOINT.to_string(),
            }
        }
    }

    fn transport(e: impl std::fmt::Display) -> LlmError {
        LlmError::Transport {
            message: e.to_string(),
        }
    }

    impl ChatCompletions for WorkerOpenRouterClient {
        async fn complete(&self, system: &str, user: &str) -> Result<String, LlmError> {
            let body = build_request_body(&self.model, system, user);

            let headers = Headers::new();
            headers
                .set("Authorization", &format!("Bearer {}", self.api_key))
                .map_err(transport)?;
            headers
                .set("Content-Type", "application/json")
                .map_err(transport)?;
            // OpenRouter asks for these for attribution; harmless if ignored.
            headers
                .set("HTTP-Referer", "https://notes-sync.kolohelios.com")
                .map_err(transport)?;
            headers
                .set("X-Title", "kolohelios-notes")
                .map_err(transport)?;

            let mut init = RequestInit::new();
            init.with_method(Method::Post)
                .with_headers(headers)
                .with_body(Some(body.to_string().into()));
            let req = Request::new_with_init(&self.endpoint, &init).map_err(transport)?;
            let mut resp = Fetch::Request(req).send().await.map_err(transport)?;

            let status = resp.status_code();
            if !(200..300).contains(&status) {
                return Err(LlmError::Status { status });
            }
            let text = resp.text().await.map_err(transport)?;
            parse_response(&text)
        }

        fn model_id(&self) -> &str {
            &self.model
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A native [`ChatCompletions`] client over `ureq`, used to exercise the
    /// real request/response wire shapes against a `mockito` server (the
    /// wasm `worker::Fetch` client can't run off-target).
    struct UreqClient {
        endpoint: String,
        model: String,
    }

    impl ChatCompletions for UreqClient {
        async fn complete(&self, system: &str, user: &str) -> Result<String, LlmError> {
            let body = build_request_body(&self.model, system, user);
            let resp = ureq::post(&self.endpoint)
                .set("Authorization", "Bearer test-key")
                .set("Content-Type", "application/json")
                .send_json(body)
                .map_err(|e| LlmError::Transport {
                    message: e.to_string(),
                })?;
            let text = resp.into_string().map_err(|e| LlmError::Transport {
                message: e.to_string(),
            })?;
            parse_response(&text)
        }

        fn model_id(&self) -> &str {
            &self.model
        }
    }

    /// A canned client that returns a fixed reply without any network — for
    /// driving `derive_title`'s cleanup without a server.
    struct FakeClient {
        reply: String,
    }

    impl ChatCompletions for FakeClient {
        async fn complete(&self, _system: &str, _user: &str) -> Result<String, LlmError> {
            Ok(self.reply.clone())
        }
        fn model_id(&self) -> &str {
            "fake/model"
        }
    }

    #[test]
    fn request_body_serializes_to_openrouter_shape() {
        let body = build_request_body("anthropic/claude-sonnet-4-6", "sys", "the note body");
        assert_eq!(
            body,
            serde_json::json!({
                "model": "anthropic/claude-sonnet-4-6",
                "messages": [
                    {"role": "system", "content": "sys"},
                    {"role": "user", "content": "the note body"},
                ],
            })
        );
    }

    #[test]
    fn parse_response_extracts_first_choice_content() {
        let raw = serde_json::json!({
            "choices": [
                {"message": {"role": "assistant", "content": "Project Roadmap"}},
                {"message": {"role": "assistant", "content": "ignored"}},
            ]
        })
        .to_string();
        assert_eq!(parse_response(&raw).unwrap(), "Project Roadmap");
    }

    #[test]
    fn parse_response_empty_choices_is_empty_response_error() {
        let raw = serde_json::json!({ "choices": [] }).to_string();
        assert!(matches!(
            parse_response(&raw).unwrap_err(),
            LlmError::EmptyResponse
        ));
    }

    #[test]
    fn parse_response_malformed_json_is_parse_error() {
        assert!(matches!(
            parse_response("not json").unwrap_err(),
            LlmError::Parse { .. }
        ));
    }

    #[tokio::test]
    async fn chat_round_trips_against_a_mocked_endpoint() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/v1/chat/completions")
            // Assert the request carries our model and the note body.
            .match_body(mockito::Matcher::PartialJsonString(
                serde_json::json!({
                    "model": "test/model",
                    "messages": [
                        {"role": "system"},
                        {"role": "user", "content": "name this note please"},
                    ],
                })
                .to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "choices": [
                        {"message": {"role": "assistant", "content": "Grocery List"}}
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = UreqClient {
            endpoint: format!("{}/api/v1/chat/completions", server.url()),
            model: "test/model".to_string(),
        };
        let title = derive_title(&client, "name this note please")
            .await
            .unwrap();
        assert_eq!(title.as_deref(), Some("Grocery List"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn chat_non_2xx_status_is_an_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/v1/chat/completions")
            .with_status(500)
            .with_body("upstream boom")
            .create_async()
            .await;
        let client = UreqClient {
            endpoint: format!("{}/api/v1/chat/completions", server.url()),
            model: "test/model".to_string(),
        };
        assert!(derive_title(&client, "body").await.is_err());
    }

    #[test]
    fn model_id_is_exposed_for_cache_keys() {
        let client = UreqClient {
            endpoint: "http://example.invalid".to_string(),
            model: "anthropic/claude-sonnet-4-6".to_string(),
        };
        assert_eq!(client.model_id(), "anthropic/claude-sonnet-4-6");
    }

    #[test]
    fn clean_title_passes_through_a_clean_title() {
        assert_eq!(
            clean_title("Project Roadmap").as_deref(),
            Some("Project Roadmap")
        );
    }

    #[test]
    fn clean_title_strips_surrounding_quotes_and_trailing_period() {
        assert_eq!(
            clean_title("\"Weekly Standup Notes.\"").as_deref(),
            Some("Weekly Standup Notes")
        );
    }

    #[test]
    fn clean_title_strips_a_markdown_heading_marker() {
        assert_eq!(
            clean_title("## Meeting Agenda").as_deref(),
            Some("Meeting Agenda")
        );
    }

    #[test]
    fn clean_title_takes_only_the_first_nonblank_line() {
        assert_eq!(
            clean_title("\n\nFirst Line Title\nsome explanation follows").as_deref(),
            Some("First Line Title")
        );
    }

    #[test]
    fn clean_title_clamps_to_six_words() {
        assert_eq!(
            clean_title("one two three four five six seven eight").as_deref(),
            Some("one two three four five six")
        );
    }

    #[test]
    fn clean_title_empty_or_punctuation_only_is_none() {
        assert_eq!(clean_title(""), None);
        assert_eq!(clean_title("   \n  "), None);
        assert_eq!(clean_title("\"\""), None);
    }

    #[tokio::test]
    async fn derive_title_cleans_a_noisy_reply() {
        let client = FakeClient {
            reply: "  \"My Great Note.\"  \nblah".to_string(),
        };
        let title = derive_title(&client, "some body text").await.unwrap();
        assert_eq!(title.as_deref(), Some("My Great Note"));
    }

    #[tokio::test]
    async fn derive_title_blank_body_makes_no_call() {
        // A blank body returns None without invoking the client at all — a
        // fake that would panic if called proves the short-circuit.
        struct PanicClient;
        impl ChatCompletions for PanicClient {
            async fn complete(&self, _s: &str, _u: &str) -> Result<String, LlmError> {
                panic!("derive_title must not call the model for a blank body");
            }
            fn model_id(&self) -> &str {
                "panic"
            }
        }
        assert_eq!(derive_title(&PanicClient, "   \n\t").await.unwrap(), None);
    }

    #[tokio::test]
    async fn derive_title_blank_reply_is_none() {
        let client = FakeClient {
            reply: "   \n  ".to_string(),
        };
        assert_eq!(derive_title(&client, "body").await.unwrap(), None);
    }

    #[test]
    fn autoname_enabled_only_with_a_real_key() {
        assert!(!autoname_enabled(None));
        assert!(!autoname_enabled(Some("")));
        assert!(!autoname_enabled(Some("   ")));
        assert!(autoname_enabled(Some("sk-or-v1-abc123")));
    }

    #[test]
    fn should_autoname_untitled_note_with_body() {
        assert!(should_autoname("a note with content\n", false));
    }

    #[test]
    fn should_autoname_skips_an_already_titled_note() {
        let note = "---\ntitle: Already Named\n---\nbody\n";
        assert!(!should_autoname(note, false));
    }

    #[test]
    fn should_autoname_force_retitles_even_when_titled() {
        let note = "---\ntitle: Already Named\n---\nbody\n";
        assert!(should_autoname(note, true));
    }

    #[test]
    fn should_autoname_never_titles_an_empty_body_even_forced() {
        // A title needs a body to name; force does not conjure one.
        assert!(!should_autoname("", true));
        assert!(!should_autoname("---\ntitle: X\n---\n\n   \n", true));
    }
}
