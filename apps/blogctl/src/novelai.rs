//! NovelAI's documented OpenAI-compatible chat and text endpoints, not its
//! token-based legacy API and not OpenRouter chat. No retries, redirects, or credential
//! discovery. The caller supplies NOVELAI_API_TOKEN in the process environment.

use std::collections::BTreeMap;
use std::env;
use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::writing::{GenerationMetadata, PreparedExperiment, Store};

mod response;
#[cfg(test)]
use response::parse_completion;
pub use response::ResponseProblem;
use response::{parse_response, Capture, CapturedBody};
mod tokenizer;
pub use response::decode_response_record;
pub use tokenizer::DecodingProvenance;

const BASE: &str = "https://text.novelai.net/oa/v1";
const RESPONSE_LIMIT: u64 = 4 * 1024 * 1024;
const DEFAULT_MAX_TOKENS: u32 = 600;
const MAX_TOKENS_LIMIT: u32 = 8192;
const DEFAULT_TEMPERATURE: f64 = 0.8;
const MAX_TEMPERATURE: f64 = 2.0;

/// Errors intentionally contain neither API bodies, credentials, nor raw
/// transport errors. Debug formatting is safe for the same reason.
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display(
        "NOVELAI_API_TOKEN is missing; supply it through 1Password at invocation time"
    ))]
    MissingToken,
    #[snafu(display("NOVELAI_API_TOKEN is not a valid single-line API token"))]
    InvalidToken,
    #[snafu(display(
        "NOVELAI_API_TOKEN contains an unresolved 1Password reference; invoke through `op run`"
    ))]
    UnresolvedReference,
    #[snafu(display("specify a nonempty model ID from `blogctl novelai models` (maximum 200 bytes, no control characters)"))]
    InvalidModel,
    #[snafu(display("max tokens must be between 1 and {MAX_TOKENS_LIMIT}"))]
    InvalidMaxTokens,
    #[snafu(display("temperature must be a finite number between 0 and {MAX_TEMPERATURE}"))]
    InvalidTemperature,
    #[snafu(display("could not construct the NovelAI request"))]
    InvalidRequest,
    #[snafu(display("NovelAI authentication failed (HTTP 401); check the persistent API token. No retry was made"))]
    Authentication,
    #[snafu(display("NovelAI model/subscription access denied (HTTP 403). No retry was made"))]
    ModelAccess,
    #[snafu(display("NovelAI rate limited the request (HTTP 429). Wait before a new manual invocation; no retry was made"))]
    RateLimited,
    #[snafu(display("NovelAI returned HTTP {status}. No redirect or retry was made; generation outcome may be unknown"))]
    Http { status: u16 },
    #[snafu(display("NovelAI transport failed. No retry was made; generation outcome may be unknown. Inspect account usage before another invocation"))]
    Transport,
    #[snafu(display("NovelAI returned an invalid, incomplete, or oversized response. No retry was made; generation outcome may be unknown. Inspect account usage before another invocation"))]
    Response,
    #[snafu(display("could not prepare private NovelAI response storage; no request was sent"))]
    CapturePreparation,
    #[snafu(display("cannot read a complete, hash-verified private NovelAI response record"))]
    ResponseRecord,
    #[snafu(display("cannot prepare pinned NovelAI decoder: {problem}. No request was sent"))]
    DecoderPreparation { problem: ResponseProblem },
    #[snafu(display("NovelAI request was already attempted, but saving response record {capture_id} could not be confirmed. Inspect the private response directory before any new request. No retry was made; generation outcome may be unknown"))]
    CaptureSave { capture_id: String },
    #[snafu(display("NovelAI response validation failed: {problem}. Private response record: .writing/provider-responses/{capture_id}.json. No retry was made; generation outcome may be unknown"))]
    CapturedResponse {
        capture_id: String,
        problem: ResponseProblem,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Explicit endpoint contract; never inferred from model names or retried with
/// another transport after a provider failure.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    #[default]
    Chat,
    Completion,
}

impl Transport {
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Chat => "chat/completions",
            Self::Completion => "completions",
        }
    }
}

#[derive(Debug, Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

/// Private fields keep candidate count, streaming, and thinking non-configurable.
#[derive(Debug, Serialize)]
struct CompletionRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    messages: Option<Vec<Message>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_thinking: Option<bool>,
    max_tokens: u32,
    temperature: f64,
    n: u8,
    best_of: u8,
    stream: bool,
    echo: bool,
}

/// Captures the local prompt before network IO. Model selection is
/// always explicit; no discovery request or fallback happens during generation.
#[derive(Debug)]
pub struct PreparedGeneration {
    snapshot: PreparedExperiment,
    request: CompletionRequest,
    transport: Transport,
}

impl PreparedGeneration {
    pub fn new(
        snapshot: PreparedExperiment,
        model: String,
        max_tokens: Option<u32>,
        temperature: Option<f64>,
    ) -> Result<Self> {
        Self::with_transport(snapshot, model, max_tokens, temperature, Transport::Chat)
    }

    pub fn with_transport(
        snapshot: PreparedExperiment,
        model: String,
        max_tokens: Option<u32>,
        temperature: Option<f64>,
        transport: Transport,
    ) -> Result<Self> {
        if model.trim().is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
            return Err(Error::InvalidModel);
        }
        let max_tokens = max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
        if !(1..=MAX_TOKENS_LIMIT).contains(&max_tokens) {
            return Err(Error::InvalidMaxTokens);
        }
        let temperature = temperature.unwrap_or(DEFAULT_TEMPERATURE);
        if !temperature.is_finite() || !(0.0..=MAX_TEMPERATURE).contains(&temperature) {
            return Err(Error::InvalidTemperature);
        }
        let (system, user) = snapshot.editorial_messages();
        let request = CompletionRequest {
            model,
            prompt: (transport == Transport::Completion).then(|| snapshot.prompt().into()),
            messages: (transport == Transport::Chat).then(|| {
                vec![
                    Message {
                        role: "system",
                        content: system,
                    },
                    Message {
                        role: "user",
                        content: user,
                    },
                ]
            }),
            enable_thinking: (transport == Transport::Chat).then_some(false),
            max_tokens,
            temperature,
            n: 1,
            best_of: 1,
            stream: false,
            echo: false,
        };
        Ok(Self {
            snapshot,
            request,
            transport,
        })
    }

    /// Preview includes no authentication header or credential lookup.
    pub fn preview(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.request).map_err(|_| Error::InvalidRequest)
    }

    pub fn snapshot(&self) -> &PreparedExperiment {
        &self.snapshot
    }

    /// Only allowlisted request fields enter provenance, never headers/tokens.
    pub fn metadata(&self) -> GenerationMetadata {
        GenerationMetadata {
            provider: "novelai".into(),
            model: self.request.model.clone(),
            ai_provenance: "NovelAI generation; human review required. Inspect endpoint, exact request, and response status in parameters.".into(),
            parameters: BTreeMap::from([
                ("max_tokens".into(), self.request.max_tokens.into()),
                ("temperature".into(), self.request.temperature.into()),
                ("n".into(), self.request.n.into()),
                ("best_of".into(), self.request.best_of.into()),
                ("stream".into(), self.request.stream.into()),
                ("echo".into(), self.request.echo.into()),
                ("endpoint".into(), self.transport.endpoint().into()),
                ("request_format_version".into(), 2.into()),
                ("request".into(), serde_json::to_value(&self.request).expect("request serializes")),
            ]),
        }
    }
}

/// The credential is never Debug-formatted, serialized, stored, or accepted as a
/// CLI argument. A fixed production endpoint avoids credential exfiltration via
/// an environment-controlled API base URL. Tests inject a loopback server only
/// through private constructors in this module.
pub struct Client {
    token: String,
    base: String,
    agent: ureq::Agent,
}

impl Client {
    pub fn from_env() -> Result<Self> {
        let token = env::var("NOVELAI_API_TOKEN").map_err(|_| Error::MissingToken)?;
        Self::new(token, BASE.into(), Duration::from_secs(60))
    }

    fn new(token: String, base: String, timeout: Duration) -> Result<Self> {
        if token.is_empty() {
            return Err(Error::MissingToken);
        }
        if token.starts_with("op://") {
            return Err(Error::UnresolvedReference);
        }
        if token.len() > 8192 || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(Error::InvalidToken);
        }
        // ureq 2 can retry on stale pooled sockets. Disable pooling as well as
        // redirects so a method call cannot hide another HTTP attempt.
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .max_idle_connections(0)
            .timeout(timeout)
            .build();
        Ok(Self { token, base, agent })
    }

    pub fn models(&self) -> Result<Vec<String>> {
        let request = self
            .agent
            .get(&format!("{}/models", self.base))
            .set("Authorization", &format!("Bearer {}", self.token));
        let response = checked(request.call())?;
        let response: ModelsResponse = decode(response)?;
        let mut models = Vec::new();
        for model in response.data {
            if model.id.trim().is_empty() || model.id.chars().any(char::is_control) {
                return Err(Error::Response);
            }
            models.push(model.id);
        }
        models.sort();
        models.dedup();
        Ok(models)
    }

    /// Exactly one POST per explicit call, with no model lookup, redirect,
    /// automatic retry, or subsequent request on malformed output.
    pub fn generate(
        &self,
        generation: &PreparedGeneration,
        store: &Store,
    ) -> Result<GeneratedCompletion> {
        let tokenizer = if generation.transport == Transport::Chat
            && generation.request.model == tokenizer::MODEL
        {
            Some(
                tokenizer::GlmTokenizer::load()
                    .map_err(|problem| Error::DecoderPreparation { problem })?,
            )
        } else {
            None
        };
        let capture = Capture::prepare(store)?;
        let request = self
            .agent
            .post(&format!(
                "{}/{}",
                self.base,
                generation.transport.endpoint()
            ))
            .set("Authorization", &format!("Bearer {}", self.token));
        let body = serde_json::to_string(&generation.request).map_err(|_| Error::InvalidRequest)?;
        let response = match request
            .set("Content-Type", "application/json")
            .send_string(&body)
        {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(ureq::Error::Transport(_)) => {
                let capture_id = capture.save(store, generation, None, None)?;
                return Err(Error::CapturedResponse {
                    capture_id,
                    problem: ResponseProblem::Transport,
                });
            }
        };
        let status = response.status();
        let captured = CapturedBody::read(response.into_reader(), &self.token);
        let capture_id = capture.save(store, generation, Some(status), Some(&captured))?;
        if let Some(problem) = captured.problem {
            return Err(Error::CapturedResponse {
                capture_id,
                problem,
            });
        }
        if status != 200 {
            return Err(Error::CapturedResponse {
                capture_id,
                problem: ResponseProblem::Http { status },
            });
        }
        parse_response(
            &captured.bytes,
            generation.transport,
            &generation.request.model,
            generation.request.max_tokens,
            tokenizer.as_ref(),
        )
        .map(|mut completion| {
            completion.response_record = Some(capture_id.clone());
            completion
        })
        .map_err(|problem| Error::CapturedResponse {
            capture_id,
            problem,
        })
    }
}

fn checked(response: std::result::Result<ureq::Response, ureq::Error>) -> Result<ureq::Response> {
    let (status, response) = match response {
        Ok(response) => (response.status(), Some(response)),
        Err(ureq::Error::Status(status, _)) => (status, None),
        // Discard raw errors: they can include request/response material.
        Err(ureq::Error::Transport(_)) => return Err(Error::Transport),
    };
    match status {
        200 => response.ok_or(Error::Response),
        401 => Err(Error::Authentication),
        403 => Err(Error::ModelAccess),
        429 => Err(Error::RateLimited),
        status => Err(Error::Http { status }),
    }
}

fn decode<T: serde::de::DeserializeOwned>(response: ureq::Response) -> Result<T> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(RESPONSE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Response)?;
    if bytes.len() as u64 > RESPONSE_LIMIT {
        return Err(Error::Response);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Response)
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct GeneratedCompletion {
    pub body: String,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
    pub reasoning_content: Option<String>,
    pub response_record: Option<String>,
    pub decoding: Option<DecodingProvenance>,
}

impl GeneratedCompletion {
    pub fn metadata(&self, generation: &PreparedGeneration) -> GenerationMetadata {
        let mut metadata = generation.metadata();
        if let Some(decoding) = &self.decoding {
            metadata.parameters.insert(
                "decoding".into(),
                serde_json::to_value(decoding).expect("decoding provenance serializes"),
            );
        }
        if let Some(record) = &self.response_record {
            metadata
                .parameters
                .insert("response_record".into(), record.clone().into());
        }
        metadata
            .parameters
            .insert("finish_reason".into(), self.finish_reason.clone().into());
        if let Some(usage) = &self.usage {
            metadata.parameters.insert(
                "usage".into(),
                serde_json::to_value(usage).expect("usage serializes"),
            );
        }
        if let Some(reasoning) = &self.reasoning_content {
            metadata
                .parameters
                .insert("reasoning_content".into(), reasoning.clone().into());
        }
        metadata
    }

    pub fn warnings(&self) -> Vec<&'static str> {
        let mut warnings = Vec::new();
        match self.finish_reason.as_deref() {
            Some("stop") => {}
            Some("length") => {
                warnings.push("provider hit the output token limit; candidate may be incomplete")
            }
            _ => warnings
                .push("provider did not report a normal stop; completion is not established"),
        }
        if self
            .reasoning_content
            .as_ref()
            .is_some_and(|text| !text.is_empty())
        {
            warnings.push("provider returned reasoning despite disabled thinking; preserved separately from prose");
        }
        if self.body.contains("<|") {
            warnings.push("decoded body contains an embedded protocol marker; no editorial cleanup was applied");
        }
        if self.body.contains("<think>")
            || self.body.contains("</think>")
            || self
                .body
                .lines()
                .any(|line| line.starts_with("Approach (") || line == "Candidate:")
        {
            warnings.push(
                "candidate contains reasoning or editorial scaffolding; inspect before selection",
            );
        }
        warnings
    }
}

#[cfg(test)]
mod tests;
