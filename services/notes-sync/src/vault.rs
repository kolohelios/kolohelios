//! Disposable per-note index records and content/model-keyed connections.
//! No Worker types: persistence and network calls live in vault_runtime.
use crate::embeddings::valid_vector;
use notes_protocol::frontmatter;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheKey {
    pub content_hash: String,
    pub model_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub key: CacheKey,
    pub has_body: bool,
    pub links: Vec<String>,
    pub vector: Option<Vec<f32>>,
    /// Source refresh failed or is pending; never advertise its old vector.
    #[serde(default)]
    pub stale: bool,
}

impl Entry {
    pub fn from_note(id: &str, note: &str, model: &str, old: Option<&Self>) -> Self {
        let canonical = frontmatter::canonical_body(note);
        let key = CacheKey {
            content_hash: frontmatter::content_hash(note),
            model_id: model.into(),
        };
        let metadata = frontmatter::parse(note)
            .map(|(metadata, _)| metadata)
            .unwrap_or_default();
        let vector = old
            .filter(|old| old.key == key)
            .and_then(|old| old.vector.as_ref())
            .filter(|vector| valid_vector(vector))
            .cloned();
        Self {
            id: id.into(),
            title: metadata.title,
            tags: metadata.tags,
            aliases: metadata.aliases,
            key,
            has_body: !canonical.trim().is_empty(),
            links: wikilinks(&canonical),
            vector,
            stale: false,
        }
    }
    pub fn needs_embedding(&self) -> bool {
        self.has_body
            && !self
                .vector
                .as_ref()
                .is_some_and(|vector| valid_vector(vector))
    }
    /// CAS after a network await: an older response cannot label a newer
    /// body or a different model as fresh.
    pub fn apply_embedding(&mut self, key: &CacheKey, vector: Vec<f32>) -> bool {
        if self.key != *key || !valid_vector(&vector) {
            return false;
        }
        self.vector = Some(vector);
        true
    }
}

/// Match the editor's plain-text wikilinks; skip code blocks/spans and
/// Markdown links/images. Coalesce Text events because brackets are split.
pub fn wikilinks(body: &str) -> Vec<String> {
    fn flush(text: &mut String, targets: &mut BTreeSet<String>) {
        let source = std::mem::take(text);
        let mut rest = source.as_str();
        while let Some(open) = rest.find("[[") {
            let after = &rest[open + 2..];
            let Some(close) = after.find("]]") else { break };
            let target = after[..close].split('|').next().unwrap_or_default().trim();
            let target = target.split('#').next().unwrap_or_default().trim();
            let target = target.strip_suffix(".md").unwrap_or(target);
            if !target.is_empty() {
                targets.insert(target.to_string());
            }
            rest = &after[close + 2..];
        }
    }
    let mut targets = BTreeSet::new();
    let mut text = String::new();
    let mut excluded = 0u32;
    for event in Parser::new(body) {
        if !matches!(event, Event::Text(_)) {
            flush(&mut text, &mut targets);
        }
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. }) => excluded += 1,
            Event::End(TagEnd::CodeBlock | TagEnd::Link | TagEnd::Image) => {
                excluded = excluded.saturating_sub(1)
            }
            Event::Text(value) if excluded == 0 => text.push_str(&value),
            _ => {}
        }
    }
    flush(&mut text, &mut targets);
    targets.into_iter().collect()
}

/// Derive only missing vectors; metadata-only/cosmetic refreshes are free.
pub async fn derive<C: crate::embeddings::Embeddings>(
    entry: &mut Entry,
    note: &str,
    client: &C,
) -> Result<bool, crate::embeddings::EmbeddingError> {
    if entry.key.model_id != client.model_id()
        || entry.key.content_hash != frontmatter::content_hash(note)
    {
        return Err(crate::embeddings::EmbeddingError::StaleInput);
    }
    if !entry.needs_embedding() {
        return Ok(false);
    }
    let input = crate::embeddings::chunks(&frontmatter::canonical_body(note))?;
    let vectors = client.embed(&input).await?;
    let vector = crate::embeddings::pool(&input, &vectors)?;
    if !entry.apply_embedding(&entry.key.clone(), vector) {
        return Err(crate::embeddings::EmbeddingError::InvalidVector);
    }
    Ok(true)
}

pub fn cosine(a: &[f32], b: &[f32]) -> Option<f64> {
    if a.len() != b.len() || !valid_vector(a) || !valid_vector(b) {
        return None;
    }
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum();
    let norm = |v: &[f32]| v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    Some((dot / (norm(a) * norm(b))).clamp(-1.0, 1.0))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Neighbor {
    pub id: String,
    pub content_hash: String,
    pub score: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connections {
    pub id: String,
    pub key: CacheKey,
    pub embedding_fresh: bool,
    pub source_stale: bool,
    pub similarity: Vec<Neighbor>,
    pub wikilinks: Vec<String>,
    pub unresolved_links: Vec<String>,
}

/// Exact path first; otherwise a unique basename/title/alias. Never pick an
/// arbitrary target for an ambiguous link, or a note excluded as pending.
fn resolve(target: &str, entries: &BTreeMap<String, Entry>) -> Option<String> {
    if entries.get(target).is_some_and(|entry| !entry.stale) {
        return Some(target.into());
    }
    let mut candidates = entries
        .values()
        .filter(|entry| !entry.stale)
        .filter(|entry| {
            entry.id.rsplit('/').next() == Some(target)
                || entry.title.as_deref() == Some(target)
                || entry.aliases.iter().any(|alias| alias == target)
        });
    let first = candidates.next()?;
    candidates.next().is_none().then(|| first.id.clone())
}

pub fn connections(
    id: &str,
    entries: &BTreeMap<String, Entry>,
    model: &str,
    k: usize,
) -> Option<Connections> {
    let source = entries.get(id)?;
    let vector = source.vector.as_deref().filter(|_| {
        source.key.model_id == model
            && !source.stale
            && source.vector.as_ref().is_some_and(|v| valid_vector(v))
    });
    let mut similarity = Vec::new();
    if let Some(vector) = vector {
        for target in entries
            .values()
            .filter(|entry| entry.id != id && entry.key.model_id == model && !entry.stale)
        {
            if let Some(score) = target
                .vector
                .as_deref()
                .and_then(|other| cosine(vector, other))
            {
                similarity.push(Neighbor {
                    id: target.id.clone(),
                    content_hash: target.key.content_hash.clone(),
                    score,
                });
            }
        }
    }
    similarity.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    similarity.truncate(k);
    let mut links = BTreeSet::new();
    let mut unresolved = Vec::new();
    for target in source.links.iter().filter(|_| !source.stale) {
        match resolve(target, entries) {
            Some(target) if target != id => {
                links.insert(target);
            }
            Some(_) => {}
            None => unresolved.push(target.clone()),
        }
    }
    Some(Connections {
        id: id.into(),
        key: source.key.clone(),
        embedding_fresh: vector.is_some(),
        source_stale: source.stale,
        similarity,
        wikilinks: links.into_iter().collect(),
        unresolved_links: unresolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(id: &str, body: &str, vector: &[f32]) -> Entry {
        let mut entry = Entry::from_note(id, body, "test-model", None);
        assert!(entry.apply_embedding(&entry.key.clone(), vector.to_vec()));
        entry
    }
    #[test]
    fn cosmetic_edits_reuse_vectors_but_body_and_model_changes_do_not() {
        let old = entry("a", "body  \r\n\r\n", &[1.0, 0.0]);
        assert!(Entry::from_note("a", "body\n", "test-model", Some(&old))
            .vector
            .is_some());
        assert!(Entry::from_note("a", "new body", "test-model", Some(&old)).needs_embedding());
        assert!(Entry::from_note("a", "body", "new-model", Some(&old)).needs_embedding());
        let mut changed = Entry::from_note("a", "new body", "test-model", Some(&old));
        assert!(!changed.apply_embedding(&old.key, vec![1.0, 0.0]));
        let mut wrong_model = changed.key.clone();
        wrong_model.model_id = "old-model".into();
        assert!(!changed.apply_embedding(&wrong_model, vec![1.0, 0.0]));
        assert!(!changed.apply_embedding(&changed.key.clone(), vec![0.0, 0.0]));
        changed.stale = true;
        assert!(changed.apply_embedding(&changed.key.clone(), vec![1.0, 0.0]));
        assert!(
            changed.stale,
            "an HTTP result must not clear a pending source refresh"
        );
        let mut corrupt = old.clone();
        corrupt.vector = Some(vec![0.0, 0.0]);
        assert!(Entry::from_note("a", "body", "test-model", Some(&corrupt)).needs_embedding());
    }
    #[tokio::test]
    async fn derivation_only_calls_the_model_for_changed_notes() {
        use crate::embeddings::{EmbeddingError, Embeddings};
        struct Client {
            model: String,
            calls: std::cell::Cell<usize>,
            fail: bool,
        }
        impl Embeddings for Client {
            fn model_id(&self) -> &str {
                &self.model
            }
            async fn embed(&self, input: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
                self.calls.set(self.calls.get() + 1);
                if self.fail {
                    return Err(EmbeddingError::Status { status: 503 });
                }
                Ok(input.iter().map(|_| vec![1.0, 0.0]).collect())
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault");
        let note = std::fs::read_to_string(root.join("target.md")).unwrap();
        let retitled = std::fs::read_to_string(root.join("retitled.md")).unwrap();
        let client = Client {
            model: "model-a".into(),
            calls: std::cell::Cell::new(0),
            fail: false,
        };
        let mut entry = Entry::from_note("target", &note, client.model_id(), None);
        assert!(derive(&mut entry, &note, &client).await.unwrap());
        let mut retitled_entry =
            Entry::from_note("target", &retitled, client.model_id(), Some(&entry));
        assert!(!derive(&mut retitled_entry, &retitled, &client)
            .await
            .unwrap());
        assert_eq!(client.calls.get(), 1);
        let changed_body = "different prose";
        let mut changed = Entry::from_note("target", changed_body, client.model_id(), Some(&entry));
        assert!(derive(&mut changed, changed_body, &client).await.unwrap());
        assert_eq!(client.calls.get(), 2);
        let other = Client {
            model: "model-b".into(),
            calls: std::cell::Cell::new(0),
            fail: false,
        };
        let mut changed_model = Entry::from_note("target", &note, other.model_id(), Some(&entry));
        assert!(derive(&mut changed_model, &note, &other).await.unwrap());
        assert_eq!(other.calls.get(), 1);
        let mut stale = Entry::from_note("target", changed_body, client.model_id(), None);
        assert!(matches!(
            derive(&mut stale, &note, &client).await,
            Err(EmbeddingError::StaleInput)
        ));
        let failure = Client {
            model: "model-a".into(),
            calls: std::cell::Cell::new(0),
            fail: true,
        };
        assert!(derive(&mut stale, changed_body, &failure).await.is_err());
        assert!(stale.vector.is_none());
        let mut blank = Entry::from_note("blank", "", client.model_id(), None);
        assert!(!derive(&mut blank, "", &client).await.unwrap());
    }

    #[test]
    fn neighbors_are_stable_dimension_safe_and_exclude_other_models() {
        let mut entries = BTreeMap::from([
            ("a".into(), entry("a", "a", &[1.0, 0.0])),
            ("b".into(), entry("b", "b", &[1.0, 0.0])),
            ("c".into(), entry("c", "c", &[1.0, 0.0])),
            ("d".into(), entry("d", "d", &[1.0, 0.0, 0.0])),
        ]);
        let result = connections("a", &entries, "test-model", 1).unwrap();
        assert_eq!(result.similarity[0].id, "b");
        entries.get_mut("b").unwrap().key.model_id = "old-model".into();
        assert_eq!(
            connections("a", &entries, "test-model", 1)
                .unwrap()
                .similarity[0]
                .id,
            "c"
        );
        assert!(connections("a", &entries, "new-model", 3)
            .unwrap()
            .similarity
            .is_empty());
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 1.0]), Some(0.0));
    }
    #[test]
    fn real_markdown_fixtures_resolve_links_and_refresh_metadata() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault");
        let source = std::fs::read_to_string(root.join("source.md")).unwrap();
        let target = std::fs::read_to_string(root.join("target.md")).unwrap();
        let original = Entry::from_note("projects/target", &target, "test-model", None);
        let mut embedded = original.clone();
        embedded.vector = Some(vec![1.0, 0.0]);
        let retitled = std::fs::read_to_string(root.join("retitled.md")).unwrap();
        let updated = Entry::from_note("projects/target", &retitled, "test-model", Some(&embedded));
        assert_eq!(updated.vector, embedded.vector);
        assert_ne!(updated.title, embedded.title);
        let mut entries = BTreeMap::from([
            (
                "source".into(),
                Entry::from_note("source", &source, "test-model", None),
            ),
            (original.id.clone(), original),
        ]);
        let result = connections("source", &entries, "test-model", 5).unwrap();
        assert_eq!(result.wikilinks, vec!["projects/target"]);
        assert_eq!(result.unresolved_links, vec!["missing"]);
        let serialized = serde_json::to_vec(&entries).unwrap();
        let rebuilt = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(
            connections("source", &rebuilt, "test-model", 5),
            Some(result)
        );
        let mut duplicate = entries["projects/target"].clone();
        duplicate.id = "other/target".into();
        entries.insert(duplicate.id.clone(), duplicate);
        let ambiguous = connections("source", &entries, "test-model", 5).unwrap();
        assert_eq!(ambiguous.wikilinks, vec!["projects/target"]);
        assert_eq!(
            ambiguous.unresolved_links,
            vec!["Alternate", "Target", "missing"]
        );
        entries.remove("other/target");
        entries.get_mut("projects/target").unwrap().stale = true;
        assert!(connections("source", &entries, "test-model", 5)
            .unwrap()
            .wikilinks
            .is_empty());
        entries.remove("projects/target");
        assert!(connections("source", &entries, "test-model", 5)
            .unwrap()
            .wikilinks
            .is_empty());
    }
}
