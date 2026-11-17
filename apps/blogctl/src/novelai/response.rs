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
    #[snafu(display("token response decoding failed: {reason}"))]
    TokenDecoding { reason: &'static str },
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

#[cfg(test)]
pub(super) fn parse_completion(
    bytes: &[u8],
    transport: Transport,
) -> std::result::Result<GeneratedCompletion, ResponseProblem> {
    parse_response(bytes, transport, "model-a", 8192, None)
}

/// Inspect captured output locally. No credentials, requests, imports, or writes.
pub fn decode_response_record(store: &Store, id: &str) -> Result<GeneratedCompletion> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if !id.starts_with("response-")
        || id.len() > 120
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(Error::ResponseRecord);
    }
    let directory = store
        .directory(&["provider-responses"], false)
        .map_err(|_| Error::ResponseRecord)?
        .ok_or(Error::ResponseRecord)?;
    let path = directory.join(format!("{id}.json"));
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| Error::ResponseRecord)?;
    const RECORD_LIMIT: u64 = 32 * 1024 * 1024;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > RECORD_LIMIT
    {
        return Err(Error::ResponseRecord);
    }
    let mut bytes = Vec::new();
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|_| Error::ResponseRecord)?
        .take(RECORD_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::ResponseRecord)?;
    if bytes.len() as u64 > RECORD_LIMIT {
        return Err(Error::ResponseRecord);
    }
    let record: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| Error::ResponseRecord)?;
    if record["version"].as_u64() != Some(1)
        || record["status"].as_u64() != Some(200)
        || [
            "truncated",
            "read_failed",
            "credential_echo_detected",
            "transport_failure",
        ]
        .iter()
        .any(|field| record[field].as_bool() != Some(false))
    {
        return Err(Error::ResponseRecord);
    }
    let encoded = record["body_base64"]
        .as_str()
        .ok_or(Error::ResponseRecord)?;
    if encoded.len() as u64 > RESPONSE_LIMIT.div_ceil(3) * 4 {
        return Err(Error::ResponseRecord);
    }
    let body = STANDARD
        .decode(encoded)
        .map_err(|_| Error::ResponseRecord)?;
    if body.len() as u64 > RESPONSE_LIMIT
        || record["retained_bytes"].as_u64() != Some(body.len() as u64)
        || record["body_sha256"].as_str() != Some(writing::hash_bytes(&body).as_str())
    {
        return Err(Error::ResponseRecord);
    }
    let model = record["request"]["model"]
        .as_str()
        .ok_or(Error::ResponseRecord)?;
    if record["model"].as_str() != Some(model) {
        return Err(Error::ResponseRecord);
    }
    let max_tokens = record["request"]["max_tokens"]
        .as_u64()
        .filter(|n| (1..=8192).contains(n))
        .ok_or(Error::ResponseRecord)? as u32;
    let transport = match record["endpoint"].as_str() {
        Some("chat/completions") => Transport::Chat,
        Some("completions") => Transport::Completion,
        _ => return Err(Error::ResponseRecord),
    };
    let tokenizer = if model == super::tokenizer::MODEL && transport == Transport::Chat {
        Some(
            super::tokenizer::GlmTokenizer::load()
                .map_err(|problem| Error::DecoderPreparation { problem })?,
        )
    } else {
        None
    };
    let mut completion = parse_response(&body, transport, model, max_tokens, tokenizer.as_ref())
        .map_err(|problem| Error::CapturedResponse {
            capture_id: id.into(),
            problem,
        })?;
    completion.response_record = Some(id.into());
    Ok(completion)
}

pub(super) fn parse_response(
    bytes: &[u8],
    transport: Transport,
    model: &str,
    max_tokens: u32,
    tokenizer: Option<&super::tokenizer::GlmTokenizer>,
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
    let finish_reason = optional_string(choice, "finish_reason", "choices[0].finish_reason")?;
    if finish_reason
        .as_ref()
        .is_some_and(|value| value.len() > 128 || value.chars().any(char::is_control))
    {
        return Err(ResponseProblem::FinishReason);
    }
    let mut decoding = None;
    let body = match transport {
        Transport::Chat if choice.get("message").is_none() && choice.get("token_ids").is_some() => {
            if model != super::tokenizer::MODEL
                || response["model"].as_str() != Some(model)
                || response["object"].as_str() != Some("chat.completion.chunk")
                || choice["index"].as_u64() != Some(0)
                || choice["text"].as_str() != Some("")
            {
                return Err(ResponseProblem::TokenDecoding {
                    reason: "unrecognized or conflicting token response contract",
                });
            }
            let values = choice["token_ids"]
                .as_array()
                .ok_or(ResponseProblem::TokenDecoding {
                    reason: "token IDs must be an array",
                })?;
            if values.len() > max_tokens as usize + 1 {
                return Err(ResponseProblem::TokenDecoding {
                    reason: "token count exceeds the requested bound",
                });
            }
            let ids: Vec<u32> = values
                .iter()
                .map(|value| {
                    value.as_u64().and_then(|id| u32::try_from(id).ok()).ok_or(
                        ResponseProblem::TokenDecoding {
                            reason: "token IDs must be unsigned 32-bit integers",
                        },
                    )
                })
                .collect::<std::result::Result<_, _>>()?;
            let tokenizer = tokenizer.ok_or(ResponseProblem::TokenDecoding {
                reason: "pinned tokenizer unavailable",
            })?;
            let (body, provenance) = tokenizer.decode(&ids, finish_reason.as_deref())?;
            decoding = Some(provenance);
            body
        }
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
        decoding,
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
