//! OpenAI-compatible batched embeddings over the existing OpenRouter credential.
use serde::Deserialize;
use snafu::{ResultExt, Snafu};

pub const DEFAULT_MODEL: &str = "openai/text-embedding-3-small";
pub const DEFAULT_ENDPOINT: &str = "https://openrouter.ai/api/v1/embeddings";
pub const MAX_DIMENSIONS: usize = 4096;
/// At most 8,000 UTF-8 bytes per chunk, within the default model's token limit.
pub const CHUNK_CHARS: usize = 2000;
pub const MAX_CHUNKS: usize = 128;

/// Provider and preprocessing are part of model identity, not a migration.
pub fn cache_model_id(model: &str, endpoint: &str) -> String {
    use sha2::{Digest, Sha256};
    let recipe =
        format!("notes-embeddings-v2:weighted-unit-mean:{CHUNK_CHARS}:{MAX_CHUNKS}:{endpoint}");
    format!("{model}@{:x}", Sha256::digest(recipe.as_bytes()))
}

#[derive(Debug, Snafu)]
pub enum EmbeddingError {
    #[snafu(display("embedding transport failed: {message}"))]
    Transport { message: String },
    #[snafu(display("embedding endpoint returned HTTP {status}"))]
    Status { status: u16 },
    #[snafu(display("invalid embedding response: {source}"))]
    Parse { source: serde_json::Error },
    #[snafu(display("embedding response must contain indexed, nonzero, finite vectors of a consistent dimension"))]
    InvalidVector,
    #[snafu(display(
        "note exceeds the {MAX_CHUNKS}-chunk embedding limit; no text was silently truncated"
    ))]
    InputTooLarge,
    #[snafu(display("embedding input does not match the entry's content/model key"))]
    StaleInput,
}

pub fn request_body(model: &str, input: &[String]) -> serde_json::Value {
    serde_json::json!({"model": model, "input": input, "encoding_format": "float"})
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}
#[derive(Deserialize)]
struct EmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}

pub fn valid_vector(vector: &[f32]) -> bool {
    !vector.is_empty()
        && vector.len() <= MAX_DIMENSIONS
        && vector.iter().all(|value| value.is_finite())
        && vector.iter().any(|value| *value != 0.0)
}

pub fn parse_response(json: &str, expected: usize) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    let mut response: EmbeddingResponse = serde_json::from_str(json).context(ParseSnafu)?;
    if expected == 0 || expected > MAX_CHUNKS || response.data.len() != expected {
        return Err(EmbeddingError::InvalidVector);
    }
    response.data.sort_by_key(|data| data.index);
    let dimensions = response
        .data
        .first()
        .map(|data| data.embedding.len())
        .unwrap_or(0);
    for (index, data) in response.data.iter().enumerate() {
        if data.index != index
            || data.embedding.len() != dimensions
            || !valid_vector(&data.embedding)
        {
            return Err(EmbeddingError::InvalidVector);
        }
    }
    Ok(response
        .data
        .into_iter()
        .map(|data| data.embedding)
        .collect())
}

pub fn chunks(body: &str) -> Result<Vec<String>, EmbeddingError> {
    let mut chunks = Vec::new();
    let mut chars = body.chars();
    loop {
        let chunk: String = chars.by_ref().take(CHUNK_CHARS).collect();
        if chunk.is_empty() {
            return Ok(chunks);
        }
        if chunks.len() == MAX_CHUNKS {
            return Err(EmbeddingError::InputTooLarge);
        }
        chunks.push(chunk);
    }
}

/// Length-weighted mean of unit vectors, normalized again. Every chunk
/// contributes, including the end of long notes; provider scale is irrelevant.
pub fn pool(input: &[String], vectors: &[Vec<f32>]) -> Result<Vec<f32>, EmbeddingError> {
    let dimensions = vectors.first().map(Vec::len).unwrap_or(0);
    if input.len() != vectors.len() || dimensions == 0 {
        return Err(EmbeddingError::InvalidVector);
    }
    let mut mean = vec![0.0f64; dimensions];
    for (text, vector) in input.iter().zip(vectors) {
        if vector.len() != dimensions || !valid_vector(vector) {
            return Err(EmbeddingError::InvalidVector);
        }
        let norm = vector
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>()
            .sqrt();
        let weight = text.chars().count() as f64;
        for (sum, value) in mean.iter_mut().zip(vector) {
            *sum += f64::from(*value) / norm * weight;
        }
    }
    let norm = mean.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm == 0.0 || !norm.is_finite() {
        return Err(EmbeddingError::InvalidVector);
    }
    let vector: Vec<f32> = mean
        .into_iter()
        .map(|value| (value / norm) as f32)
        .collect();
    if !valid_vector(&vector) {
        return Err(EmbeddingError::InvalidVector);
    }
    Ok(vector)
}

#[allow(async_fn_in_trait)]
pub trait Embeddings {
    fn model_id(&self) -> &str;
    async fn embed(&self, input: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
}

#[cfg(target_arch = "wasm32")]
pub struct WorkerEmbeddingClient {
    api_key: String,
    model: String,
    endpoint: String,
    cache_id: String,
}

#[cfg(target_arch = "wasm32")]
impl WorkerEmbeddingClient {
    pub fn new(api_key: String, model: String, endpoint: String) -> Self {
        let cache_id = cache_model_id(&model, &endpoint);
        Self {
            api_key,
            model,
            endpoint,
            cache_id,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Embeddings for WorkerEmbeddingClient {
    fn model_id(&self) -> &str {
        &self.cache_id
    }
    async fn embed(&self, input: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        use worker::{Fetch, Headers, Method, Request, RequestInit};
        let transport = |error: worker::Error| EmbeddingError::Transport {
            message: error.to_string(),
        };
        let headers = Headers::new();
        headers
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .map_err(transport)?;
        headers
            .set("Content-Type", "application/json")
            .map_err(transport)?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(request_body(&self.model, input).to_string().into()));
        let request = Request::new_with_init(&self.endpoint, &init).map_err(transport)?;
        let mut response = Fetch::Request(request).send().await.map_err(transport)?;
        let status = response.status_code();
        if !(200..300).contains(&status) {
            return Err(EmbeddingError::Status { status });
        }
        parse_response(&response.text().await.map_err(transport)?, input.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn response_fixtures() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/embeddings");
        for (directory, expected, valid) in [
            ("valid", 1, true),
            ("invalid", 1, false),
            ("batch", 2, true),
            ("batch-invalid", 2, false),
        ] {
            for file in std::fs::read_dir(root.join(directory)).unwrap() {
                let path = file.unwrap().path();
                let json = std::fs::read_to_string(&path).unwrap();
                assert_eq!(
                    parse_response(&json, expected).is_ok(),
                    valid,
                    "{}",
                    path.display()
                );
            }
        }
    }
    #[test]
    fn batched_request_round_trips_against_a_mocked_endpoint() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/embeddings");
        let response = std::fs::read_to_string(root.join("batch/reordered.json")).unwrap();
        let input = vec!["first chunk".to_string(), "second chunk".to_string()];
        let body = request_body("test-model", &input);
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/embeddings")
            .match_header("authorization", "Bearer test-credential")
            .match_body(mockito::Matcher::Json(body.clone()))
            .with_header("content-type", "application/json")
            .with_body(response)
            .create();
        let result = ureq::post(&format!("{}/embeddings", server.url()))
            .set("Authorization", "Bearer test-credential")
            .send_json(&body)
            .unwrap();
        let vectors = parse_response(&result.into_string().unwrap(), 2).unwrap();
        assert_eq!(vectors, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
        mock.assert();
    }
    #[test]
    fn invalid_numeric_vectors_are_rejected() {
        assert!(!valid_vector(&[f32::NAN]));
        assert!(!valid_vector(&[f32::INFINITY]));
        assert!(!valid_vector(&vec![1.0; MAX_DIMENSIONS + 1]));
    }
    #[test]
    fn chunking_preserves_unicode_and_refuses_to_truncate() {
        let body = "é".repeat(CHUNK_CHARS + 3);
        let input = chunks(&body).unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input.concat(), body);
        assert!(chunks(&"x".repeat(CHUNK_CHARS * MAX_CHUNKS + 1)).is_err());
        let pooled = pool(
            &["ab".into(), "c".into()],
            &[vec![2.0, 0.0], vec![0.0, 3.0]],
        )
        .unwrap();
        assert!(pooled[0] > pooled[1] && pooled[1] > 0.0);
        assert!(pool(&["a".into()], &[vec![0.0, 0.0]]).is_err());
        assert_ne!(
            cache_model_id("m", "https://a"),
            cache_model_id("m", "https://b")
        );
    }
}
