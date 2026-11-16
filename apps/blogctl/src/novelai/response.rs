//! Private bounded response evidence, published before semantic parsing.

use std::io::{Read, Write};
use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use snafu::Snafu;
use tempfile::NamedTempFile;

use super::{
    Error, GeneratedCompletion, PreparedGeneration, Result, Transport, Usage, RESPONSE_LIMIT,
};
use crate::writing::{self, Store};

#[derive(Debug, Serialize, Snafu)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ResponseProblem {
    #[snafu(display("transport failed before a response was available"))]
    Transport,
    #[snafu(display("HTTP {status}"))]
    Http { status: u16 },
    #[snafu(display("response body exceeded the byte limit; only a prefix was retained"))]
    Oversized,
    #[snafu(display("response body could not be read completely; partial bytes were retained"))]
    Read,
    #[snafu(display("response contained the API credential; body was withheld from storage"))]
    CredentialEcho,
    #[snafu(display("invalid JSON at line {line}, column {column}"))]
    Json { line: usize, column: usize },
    #[snafu(display("response schema mismatch at {field}"))]
    Schema { field: &'static str },
    #[snafu(display("expected one choice, received {count}"))]
    Choices { count: usize },
    #[snafu(display("chat message role was not assistant"))]
    Role,
    #[snafu(display("provider returned an empty deliverable"))]
    Empty,
    #[snafu(display("provider returned reasoning but no deliverable"))]
    ReasoningOnly,
    #[snafu(display("provider finish reason was invalid"))]
    FinishReason,
}

#[derive(Serialize)]
struct ResponseRecord<'a> {
    version: u32,
    endpoint: &'static str,
    request: serde_json::Value,
    status: Option<u16>,
    body_limit: u64,
    retained_bytes: usize,
    truncated: bool,
    read_failed: bool,
    credential_echo_detected: bool,
    body_sha256: String,
    body_base64: String,
    transport_failure: bool,
    // Only the endpoint and JSON payload enter this record, never headers.
    model: &'a str,
}

pub(super) struct Capture {
    pending: NamedTempFile,
    destination: PathBuf,
    pub id: String,
}

pub(super) struct CapturedBody {
    pub bytes: Vec<u8>,
    pub problem: Option<ResponseProblem>,
    truncated: bool,
    read_failed: bool,
    credential_echo_detected: bool,
}

impl CapturedBody {
    pub fn read(reader: impl Read, token: &str) -> Self {
        let mut bytes = Vec::new();
        let read_failed = reader
            .take(RESPONSE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .is_err();
        let truncated = bytes.len() as u64 > RESPONSE_LIMIT;
        let credential_echo_detected = bytes
            .windows(token.len())
            .any(|window| window == token.as_bytes());
        bytes.truncate(RESPONSE_LIMIT as usize);
        // Exact response preservation must not become persistent token storage.
        if credential_echo_detected {
            bytes.clear();
        }
        let problem = if credential_echo_detected {
            Some(ResponseProblem::CredentialEcho)
        } else if read_failed {
            Some(ResponseProblem::Read)
        } else if truncated {
            Some(ResponseProblem::Oversized)
        } else {
            None
        };
        Self {
            bytes,
            problem,
            truncated,
            read_failed,
            credential_echo_detected,
        }
    }
}

impl Capture {
    pub fn prepare(store: &Store) -> Result<Self> {
        let directory = store
            .directory(&["provider-responses"], true)
            .map_err(|_| Error::CapturePreparation)?
            .ok_or(Error::CapturePreparation)?;
        let pending = NamedTempFile::new_in(&directory).map_err(|_| Error::CapturePreparation)?;
        let id = pending
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(Error::CapturePreparation)?
            .trim_start_matches('.')
            .to_owned();
        let destination = directory.join(format!("response-{id}.json"));
        Ok(Self {
            pending,
            destination,
            id: format!("response-{id}"),
        })
    }

    pub fn save(
        mut self,
        store: &Store,
        generation: &PreparedGeneration,
        status: Option<u16>,
        body: Option<&CapturedBody>,
    ) -> Result<String> {
        let directory = store
            .directory(&["provider-responses"], false)
            .map_err(|_| Error::CaptureSave {
                capture_id: self.id.clone(),
            })?
            .ok_or_else(|| Error::CaptureSave {
                capture_id: self.id.clone(),
            })?;
        if self.destination.parent() != Some(directory.as_path()) {
            return Err(Error::CaptureSave {
                capture_id: self.id.clone(),
            });
        }
        let bytes = body.map(|body| body.bytes.as_slice()).unwrap_or_default();
        let record = ResponseRecord {
            version: 1,
            endpoint: generation.transport.endpoint(),
            request: serde_json::to_value(&generation.request)
                .map_err(|_| Error::InvalidRequest)?,
            status,
            body_limit: RESPONSE_LIMIT,
            retained_bytes: bytes.len(),
            truncated: body.is_some_and(|body| body.truncated),
            read_failed: body.is_some_and(|body| body.read_failed),
            credential_echo_detected: body.is_some_and(|body| body.credential_echo_detected),
            body_sha256: writing::hash_bytes(bytes),
            body_base64: STANDARD.encode(bytes),
            transport_failure: status.is_none(),
            model: &generation.request.model,
        };
        serde_json::to_writer(&mut self.pending, &record).map_err(|_| Error::CaptureSave {
            capture_id: self.id.clone(),
        })?;
        self.pending.flush().map_err(|_| Error::CaptureSave {
            capture_id: self.id.clone(),
        })?;
        writing::publish(self.pending, &self.destination, 0o400).map_err(|_| {
            Error::CaptureSave {
                capture_id: self.id.clone(),
            }
        })?;
        Ok(self.id)
    }
}

pub(super) fn parse_completion(
    bytes: &[u8],
    transport: Transport,
) -> std::result::Result<GeneratedCompletion, ResponseProblem> {
    use serde_json::Value;
    let response: Value = serde_json::from_slice(bytes).map_err(|error| ResponseProblem::Json {
        line: error.line(),
        column: error.column(),
    })?;
    let choices =
        response
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(ResponseProblem::Schema {
                field: "choices (array required)",
            })?;
    if choices.len() != 1 {
        return Err(ResponseProblem::Choices {
            count: choices.len(),
        });
    }
    let choice = &choices[0];
    let reasoning = if transport == Transport::Chat {
        optional_string(
            &choice["message"],
            "reasoning_content",
            "choices[0].message.reasoning_content",
        )?
    } else {
        None
    };
    let body = match transport {
        Transport::Chat => {
            let message = choice
                .get("message")
                .filter(|value| value.is_object())
                .ok_or(ResponseProblem::Schema {
                    field: "choices[0].message (object required)",
                })?;
            let role =
                message
                    .get("role")
                    .and_then(Value::as_str)
                    .ok_or(ResponseProblem::Schema {
                        field: "choices[0].message.role (string required)",
                    })?;
            if role != "assistant" {
                return Err(ResponseProblem::Role);
            }
            match message.get("content") {
                Some(Value::String(content)) => content.clone(),
                None | Some(Value::Null)
                    if reasoning
                        .as_ref()
                        .is_some_and(|value| !value.trim().is_empty()) =>
                {
                    return Err(ResponseProblem::ReasoningOnly)
                }
                _ => {
                    return Err(ResponseProblem::Schema {
                        field: "choices[0].message.content (string required)",
                    })
                }
            }
        }
        Transport::Completion => choice
            .get("text")
            .and_then(Value::as_str)
            .ok_or(ResponseProblem::Schema {
                field: "choices[0].text (string required)",
            })?
            .to_owned(),
    };
    if body.trim().is_empty() {
        return Err(
            if reasoning
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty())
            {
                ResponseProblem::ReasoningOnly
            } else {
                ResponseProblem::Empty
            },
        );
    }
    let finish_reason = optional_string(choice, "finish_reason", "choices[0].finish_reason")?;
    if finish_reason
        .as_ref()
        .is_some_and(|value| value.len() > 128 || value.chars().any(char::is_control))
    {
        return Err(ResponseProblem::FinishReason);
    }
    let usage = match response.get("usage") {
        None | Some(Value::Null) => None,
        Some(value) if value.is_object() => Some(Usage {
            prompt_tokens: optional_count(value, "prompt_tokens", "usage.prompt_tokens")?,
            completion_tokens: optional_count(
                value,
                "completion_tokens",
                "usage.completion_tokens",
            )?,
            total_tokens: optional_count(value, "total_tokens", "usage.total_tokens")?,
        }),
        _ => {
            return Err(ResponseProblem::Schema {
                field: "usage (object required)",
            })
        }
    };
    Ok(GeneratedCompletion {
        body,
        finish_reason,
        reasoning_content: reasoning,
        usage,
        response_record: None,
    })
}

fn optional_string(
    value: &serde_json::Value,
    key: &str,
    field: &'static str,
) -> std::result::Result<Option<String>, ResponseProblem> {
    match value.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(ResponseProblem::Schema { field }),
    }
}

fn optional_count(
    value: &serde_json::Value,
    key: &str,
    field: &'static str,
) -> std::result::Result<Option<u64>, ResponseProblem> {
    match value.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or(ResponseProblem::Schema { field }),
    }
}
