//! Strict decode-only support for the official, hash-pinned Xialong vocabulary.
//! No downloads, tokenizer inference, BPE encoding, or editorial text cleanup.

use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};

use super::ResponseProblem;
use crate::writing;

pub const MODEL: &str = "xialong-v1";
pub const RESOURCE_URL: &str =
    "https://novelai.net/tokenizer/compressed/glm45_tokenizer.def?v=2&static=true";
pub const COMPRESSED_SHA256: &str =
    "d862f215272db05b6708671eb027f31ab83fdda9f3d41f8c6102c5f5002e2768";
const RESOURCE_LIMIT: usize = 4 * 1024 * 1024;
const INFLATED_LIMIT: u64 = 16 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 1024 * 1024;
const USER_MARKER: u32 = 151336;

#[derive(Debug, Serialize)]
pub struct DecodingProvenance {
    pub tokenizer_url: &'static str,
    pub tokenizer_sha256: &'static str,
    pub decoded_raw_sha256: String,
    pub token_count: usize,
    pub removed_terminal_token_ids: Vec<u32>,
}

pub(super) struct GlmTokenizer {
    vocabulary: BTreeMap<u32, String>,
    reverse_bytes: BTreeMap<char, u8>,
}

fn invalid(reason: &'static str) -> ResponseProblem {
    ResponseProblem::TokenDecoding { reason }
}

#[derive(Deserialize)]
struct Vocabulary {
    vocab: BTreeMap<String, u32>,
}

impl GlmTokenizer {
    pub fn load() -> std::result::Result<Self, ResponseProblem> {
        let path = option_env!("BLOGCTL_GLM_TOKENIZER").ok_or_else(|| {
            invalid("pinned tokenizer missing; build inside the project Nix devshell")
        })?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| invalid("cannot open pinned tokenizer"))?
            .take(RESOURCE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid("cannot read pinned tokenizer"))?;
        Self::from_compressed(&bytes)
    }

    fn from_compressed(bytes: &[u8]) -> std::result::Result<Self, ResponseProblem> {
        if bytes.len() > RESOURCE_LIMIT || writing::hash_bytes(bytes) != COMPRESSED_SHA256 {
            return Err(invalid("pinned tokenizer hash mismatch"));
        }
        let mut inflated = Vec::new();
        flate2::read::DeflateDecoder::new(bytes)
            .take(INFLATED_LIMIT + 1)
            .read_to_end(&mut inflated)
            .map_err(|_| invalid("invalid compressed tokenizer"))?;
        if inflated.len() as u64 > INFLATED_LIMIT {
            return Err(invalid("inflated tokenizer exceeds limit"));
        }
        let vocabulary: Vocabulary = serde_json::from_slice(&inflated)
            .map_err(|_| invalid("invalid tokenizer vocabulary"))?;
        let count = vocabulary.vocab.len();
        let vocabulary: BTreeMap<_, _> = vocabulary
            .vocab
            .into_iter()
            .map(|(text, id)| (id, text))
            .collect();
        if vocabulary.len() != count
            || vocabulary.get(&USER_MARKER).map(String::as_str) != Some("<|user|>")
        {
            return Err(invalid("tokenizer identity mismatch"));
        }
        let mut reverse_bytes = BTreeMap::new();
        for byte in (33u8..=126).chain(161..=172).chain(174..=255) {
            reverse_bytes.insert(char::from(byte), byte);
        }
        let mut extra = 0;
        for byte in 0u8..=255 {
            if !reverse_bytes.values().any(|value| *value == byte) {
                reverse_bytes.insert(
                    char::from_u32(256 + extra).expect("byte mapping is Unicode"),
                    byte,
                );
                extra += 1;
            }
        }
        Ok(Self {
            vocabulary,
            reverse_bytes,
        })
    }

    pub fn decode(
        &self,
        ids: &[u32],
        finish_reason: Option<&str>,
    ) -> std::result::Result<(String, DecodingProvenance), ResponseProblem> {
        if ids.is_empty() || ids.len() > 8193 {
            return Err(invalid("invalid token count"));
        }
        let mut bytes = Vec::new();
        for id in ids {
            let token = self
                .vocabulary
                .get(id)
                .ok_or_else(|| invalid("unknown token ID"))?;
            for character in token.chars() {
                let byte = self
                    .reverse_bytes
                    .get(&character)
                    .ok_or_else(|| invalid("invalid token byte representation"))?;
                bytes.push(*byte);
                if bytes.len() > OUTPUT_LIMIT {
                    return Err(invalid("decoded output exceeds limit"));
                }
            }
        }
        let raw = String::from_utf8(bytes)
            .map_err(|_| invalid("decoded output is not complete UTF-8"))?;
        let mut provenance = DecodingProvenance {
            tokenizer_url: RESOURCE_URL,
            tokenizer_sha256: COMPRESSED_SHA256,
            decoded_raw_sha256: writing::hash(&raw),
            token_count: ids.len(),
            removed_terminal_token_ids: Vec::new(),
        };
        // Protocol handling is narrow and observable. Preserve every other byte,
        // including embedded markers, reasoning, whitespace, and punctuation.
        let body = if finish_reason == Some("stop") && ids.last() == Some(&USER_MARKER) {
            provenance.removed_terminal_token_ids.push(USER_MARKER);
            raw.strip_suffix("<|user|>")
                .ok_or_else(|| invalid("terminal protocol marker mismatch"))?
                .to_owned()
        } else {
            raw
        };
        Ok((body, provenance))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_hash_failures_are_rejected() {
        let bytes = std::fs::read(env!(
            "BLOGCTL_GLM_TOKENIZER",
            "run tests inside the Nix devshell"
        ))
        .unwrap();
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        assert!(GlmTokenizer::from_compressed(&changed).is_err());
        assert!(GlmTokenizer::from_compressed(&[]).is_err());
        assert!(GlmTokenizer::from_compressed(&vec![0; RESOURCE_LIMIT + 1]).is_err());
        assert!(GlmTokenizer::from_compressed(&bytes).is_ok());
    }
}
