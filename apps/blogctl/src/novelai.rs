//! NovelAI's documented OpenAI-compatible text endpoints, not its token-based
//! legacy API and not OpenRouter chat. No retries, redirects, or credential
//! discovery. The caller supplies NOVELAI_API_TOKEN in the process environment.

use std::collections::BTreeMap;
use std::env;
use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::writing::{GenerationMetadata, PreparedExperiment};

const BASE: &str = "https://text.novelai.net/oa/v1";
const RESPONSE_LIMIT: u64 = 4 * 1024 * 1024;

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
}

pub type Result<T> = std::result::Result<T, Error>;

/// Exact one-candidate, bounded-output request. Fields are private so neither a
/// caller nor a future adapter can turn a single request into a batch.
#[derive(Debug, Serialize)]
struct CompletionRequest {
    model: String,
    prompt: String,
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
}

impl PreparedGeneration {
    pub fn new(snapshot: PreparedExperiment, model: String) -> Result<Self> {
        if model.trim().is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
            return Err(Error::InvalidModel);
        }
        let request = CompletionRequest {
            model,
            prompt: snapshot.prompt().into(),
            max_tokens: 600,
            temperature: 0.8,
            n: 1,
            best_of: 1,
            stream: false,
            echo: false,
        };
        Ok(Self { snapshot, request })
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
            ai_provenance: "NovelAI text completion; human review required.".into(),
            parameters: BTreeMap::from([
                ("max_tokens".into(), self.request.max_tokens.into()),
                ("temperature".into(), self.request.temperature.into()),
                ("n".into(), self.request.n.into()),
                ("best_of".into(), self.request.best_of.into()),
                ("stream".into(), self.request.stream.into()),
                ("echo".into(), self.request.echo.into()),
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
    pub fn generate(&self, generation: &PreparedGeneration) -> Result<String> {
        let request = self
            .agent
            .post(&format!("{}/completions", self.base))
            .set("Authorization", &format!("Bearer {}", self.token));
        let body = serde_json::to_string(&generation.request).map_err(|_| Error::InvalidRequest)?;
        let response: CompletionResponse = decode(checked(
            request
                .set("Content-Type", "application/json")
                .send_string(&body),
        )?)?;
        if response.choices.len() != 1 {
            return Err(Error::Response);
        }
        let text = response
            .choices
            .into_iter()
            .next()
            .ok_or(Error::Response)?
            .text;
        if text.trim().is_empty() {
            return Err(Error::Response);
        }
        Ok(text)
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

#[derive(Deserialize)]
struct CompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    text: String,
}

#[cfg(test)]
mod tests;
