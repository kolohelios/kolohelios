//! Search ranking over the same content/model-stamped Vault records.
use crate::{
    drift,
    vault::{self, Entry},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Text,
    Semantic,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub query: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    20
}
impl Query {
    pub fn valid(&self) -> bool {
        !self.query.trim().is_empty()
            && self.query.chars().count() <= crate::embeddings::CHUNK_CHARS
            && (1..=100).contains(&self.limit)
    }
}
#[derive(Debug, Serialize)]
pub struct Hit {
    pub id: String,
    pub title: Option<String>,
    pub content_hash: String,
    pub score: f64,
    pub excerpt: String,
    pub git_conflict: bool,
}

pub fn rank(
    query: &Query,
    entries: &[(Entry, String)],
    model: &str,
    vector: Option<&[f32]>,
) -> Vec<Hit> {
    if !query.valid() {
        return Vec::new();
    }
    let terms: Vec<_> = query
        .query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let mut hits = Vec::new();
    for (entry, text) in entries {
        if entry.stale || !drift::text_matches(entry, text) {
            continue;
        }
        let score = match query.mode {
            Mode::Text => {
                let title = entry.title.as_deref().unwrap_or(&entry.id).to_lowercase();
                let metadata = format!(
                    "{} {} {}",
                    entry.id,
                    entry.tags.join(" "),
                    entry.aliases.join(" ")
                )
                .to_lowercase();
                let body = text.to_lowercase();
                if !terms.iter().all(|term| {
                    title.contains(term) || metadata.contains(term) || body.contains(term)
                }) {
                    continue;
                }
                terms
                    .iter()
                    .map(|term| {
                        f64::from(title.contains(term)) * 8.0
                            + f64::from(metadata.contains(term)) * 4.0
                            + body.matches(term.as_str()).count().min(20) as f64
                    })
                    .sum()
            }
            Mode::Semantic => {
                if entry.key.model_id != model {
                    continue;
                }
                let Some(score) = vector.and_then(|query| {
                    entry
                        .vector
                        .as_deref()
                        .and_then(|other| vault::cosine(query, other))
                }) else {
                    continue;
                };
                if score <= 0.0 {
                    continue;
                }
                score
            }
        };
        let line = text
            .lines()
            .find(|line| terms.iter().any(|term| line.to_lowercase().contains(term)))
            .unwrap_or(text);
        hits.push(Hit {
            id: entry.id.clone(),
            title: entry.title.clone(),
            content_hash: entry.key.content_hash.clone(),
            score,
            excerpt: line.chars().take(200).collect(),
            git_conflict: entry.git_conflict,
        });
    }
    finish(hits, query.limit)
}

pub fn finish(mut hits: Vec<Hit>, limit: usize) -> Vec<Hit> {
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_and_semantic_search_use_fresh_shared_records() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault");
        let mut entries = Vec::new();
        for (id, filename, vector) in [
            ("source", "source.md", vec![1.0, 0.0]),
            ("target", "target.md", vec![0.0, 1.0]),
        ] {
            let note = std::fs::read_to_string(root.join(filename)).unwrap();
            let mut entry = Entry::from_note(id, &note, "model", None);
            entry.text_hash = Some(entry.key.content_hash.clone());
            entry.vector = Some(vector);
            entries.push((entry, notes_protocol::frontmatter::canonical_body(&note)));
        }
        let mut query = Query {
            query: "stable prose".into(),
            mode: Mode::Text,
            limit: 10,
        };
        assert_eq!(rank(&query, &entries, "model", None)[0].id, "target");
        query.query = "Alternate".into();
        assert_eq!(rank(&query, &entries, "model", None)[0].id, "target");
        query.mode = Mode::Semantic;
        assert_eq!(
            rank(&query, &entries, "model", Some(&[1.0, 0.0]))[0].id,
            "source"
        );
        assert!(rank(&query, &entries, "other-model", Some(&[1.0, 0.0])).is_empty());
        entries[0].0.stale = true;
        assert!(rank(&query, &entries, "model", Some(&[1.0, 0.0])).is_empty());
        entries[0].0.stale = false;
        entries[0].1 = "corrupted text".into();
        assert!(rank(&query, &entries, "model", Some(&[1.0, 0.0])).is_empty());
        query.query.clear();
        assert!(!query.valid());
    }
}
