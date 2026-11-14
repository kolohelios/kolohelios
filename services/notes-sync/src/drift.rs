//! Source reconciliation and drift classification, independent of Worker I/O.
use crate::vault::Entry;
use notes_protocol::frontmatter;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub seq: u64,
    pub text: String,
    #[serde(default)]
    pub committed_seq: u64,
    #[serde(default)]
    pub active_sockets: usize,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub leased: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Import {
    pub expected: Snapshot,
    pub text: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reconcile {
    Unchanged,
    Conflict,
    Apply { text: String, deleted: bool },
}

/// Git is accepted only through the single writer, never over hot edits.
/// The new socket may hydrate before its first Sync; background work allows
/// no sockets. Callers must re-read the snapshot after any network await.
pub fn reconcile(snapshot: &Snapshot, git: Option<&str>, allowed_sockets: usize) -> Reconcile {
    let deleted = git.is_none();
    let text = git.unwrap_or_default();
    if snapshot.text == text && snapshot.deleted == deleted {
        return Reconcile::Unchanged;
    }
    if snapshot.leased
        || snapshot.seq != snapshot.committed_seq
        || snapshot.active_sockets > allowed_sockets
    {
        return Reconcile::Conflict;
    }
    Reconcile::Apply {
        text: text.into(),
        deleted,
    }
}

/// Git's object hash lets verification compare the whole cold snapshot
/// from one tree listing, without spending a Contents API request per note.
pub fn git_blob_hash(text: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut hash = Sha1::new();
    hash.update(format!("blob {}\0", text.len()).as_bytes());
    hash.update(text.as_bytes());
    format!("{:x}", hash.finalize())
}

pub fn text_chunks(note: &str) -> Vec<String> {
    let canonical = frontmatter::canonical_body(note);
    let mut chars = canonical.chars();
    std::iter::from_fn(|| {
        let chunk: String = chars
            .by_ref()
            .take(crate::embeddings::CHUNK_CHARS)
            .collect();
        (!chunk.is_empty()).then_some(chunk)
    })
    .collect()
}

pub fn text_matches(entry: &Entry, text: &str) -> bool {
    use sha2::{Digest, Sha256};
    entry.text_hash.as_ref() == Some(&entry.key.content_hash)
        && format!("{:x}", Sha256::digest(text.as_bytes())) == entry.key.content_hash
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    UnsupportedId,
    Missing,
    Orphan,
    Content,
    Model,
    Metadata,
    FullText,
    Embedding,
    Pending,
    GitConflict,
    SourceUnavailable,
}

pub fn reasons(
    id: &str,
    note: Option<&str>,
    entry: Option<&Entry>,
    text: Option<&str>,
    model: &str,
    pending: bool,
) -> Vec<Reason> {
    let Some(note) = note else {
        return if entry.is_some() {
            vec![Reason::Orphan]
        } else {
            vec![]
        };
    };
    let Some(entry) = entry else {
        return vec![Reason::Missing];
    };
    let expected = Entry::from_note(id, note, model, None);
    let mut reasons = Vec::new();
    if entry.key.content_hash != expected.key.content_hash {
        reasons.push(Reason::Content);
    }
    if entry.key.model_id != expected.key.model_id {
        reasons.push(Reason::Model);
    }
    if entry.id != expected.id
        || entry.has_body != expected.has_body
        || entry.title != expected.title
        || entry.tags != expected.tags
        || entry.aliases != expected.aliases
        || entry.links != expected.links
    {
        reasons.push(Reason::Metadata);
    }
    if !text
        .is_some_and(|text| text_matches(entry, text) && text == frontmatter::canonical_body(note))
    {
        reasons.push(Reason::FullText);
    }
    if expected.has_body
        && !entry
            .vector
            .as_deref()
            .is_some_and(crate::embeddings::valid_vector)
    {
        reasons.push(Reason::Embedding);
    }
    if entry.stale || pending {
        reasons.push(Reason::Pending);
    }
    if entry.git_conflict {
        reasons.push(Reason::GitConflict);
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    fn note() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault/target.md"),
        )
        .unwrap()
    }
    #[test]
    fn reconciliation_preserves_hot_edits_and_live_sockets() {
        let mut snapshot = Snapshot {
            seq: 2,
            committed_seq: 1,
            text: "hot".into(),
            ..Snapshot::default()
        };
        assert_eq!(reconcile(&snapshot, Some("git"), 0), Reconcile::Conflict);
        assert_eq!(reconcile(&snapshot, None, 0), Reconcile::Conflict);
        snapshot.committed_seq = 2;
        snapshot.active_sockets = 1;
        assert_eq!(reconcile(&snapshot, Some("git"), 0), Reconcile::Conflict);
        assert_eq!(
            reconcile(&snapshot, Some("git"), 1),
            Reconcile::Apply {
                text: "git".into(),
                deleted: false
            }
        );
        snapshot.active_sockets = 0;
        snapshot.leased = true;
        assert_eq!(reconcile(&snapshot, Some("git"), 0), Reconcile::Conflict);
        snapshot.leased = false;
        assert_eq!(
            reconcile(&snapshot, None, 0),
            Reconcile::Apply {
                text: String::new(),
                deleted: true
            }
        );
        assert_eq!(reconcile(&snapshot, Some("hot"), 0), Reconcile::Unchanged);
    }
    #[test]
    fn scan_detects_content_metadata_model_text_and_vector_drift() {
        let note = note();
        let mut entry = Entry::from_note("target", &note, "model", None);
        let text = frontmatter::canonical_body(&note);
        entry.text_hash = Some(entry.key.content_hash.clone());
        entry.text_chunks = text_chunks(&note).len();
        entry.vector = Some(vec![1.0, 0.0]);
        assert!(reasons(
            "target",
            Some(&note),
            Some(&entry),
            Some(&text),
            "model",
            false
        )
        .is_empty());
        assert_eq!(
            reasons("target", Some(&note), None, None, "model", false),
            vec![Reason::Missing]
        );
        assert_eq!(
            reasons("target", None, Some(&entry), None, "model", false),
            vec![Reason::Orphan]
        );
        assert!(reasons(
            "target",
            Some("new body"),
            Some(&entry),
            Some(&text),
            "model",
            false
        )
        .contains(&Reason::Content));
        assert!(reasons(
            "target",
            Some(&note),
            Some(&entry),
            Some("corrupt"),
            "model",
            false
        )
        .contains(&Reason::FullText));
        assert!(reasons(
            "target",
            Some(&note),
            Some(&entry),
            Some(&text),
            "new-model",
            false
        )
        .contains(&Reason::Model));
        entry.title = None;
        entry.vector = None;
        entry.git_conflict = true;
        let result = reasons(
            "target",
            Some(&note),
            Some(&entry),
            Some(&text),
            "model",
            true,
        );
        assert!(result.contains(&Reason::Metadata) && result.contains(&Reason::Embedding));
        assert!(result.contains(&Reason::Pending) && result.contains(&Reason::GitConflict));
    }
}
