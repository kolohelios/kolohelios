//! Immutable, import-only evidence packs for private writing workflows.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, OffsetDateTime};

use crate::writing::{self, Store};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimRelationship {
    Supports,
    Complicates,
    Contradicts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum SourceClassification {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceEntry {
    pub reference: String,
    pub accessed_on: String,
    pub note: String,
    pub claim: String,
    pub relationship: ClaimRelationship,
    pub classification: SourceClassification,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct EvidenceContent {
    importer: String,
    provenance: String,
    entries: Vec<EvidenceEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceImport {
    version: u32,
    importer: String,
    provenance: String,
    entries: Vec<EvidenceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidencePack {
    pub evidence_pack_id: String,
    pub imported_at: String,
    pub importer: String,
    pub provenance: String,
    pub entries: Vec<EvidenceEntry>,
    pub content_hash: String,
    pub hash_algorithm: String,
}

fn invalid(reason: impl Into<String>) -> writing::Error {
    writing::Error::Invalid {
        reason: reason.into(),
    }
}

fn required(value: &str, field: &str) -> writing::Result<()> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{field} must not be empty")));
    }
    Ok(())
}

fn pack_content(pack: &EvidencePack) -> EvidenceContent {
    EvidenceContent {
        importer: pack.importer.clone(),
        provenance: pack.provenance.clone(),
        entries: pack.entries.clone(),
    }
}

fn content_hash(content: &EvidenceContent, path: &Path) -> writing::Result<String> {
    let bytes = serde_json::to_vec(content).map_err(|source| writing::Error::Json {
        path: path.into(),
        source,
    })?;
    Ok(writing::hash_bytes(&bytes))
}

fn validate_content(content: &EvidenceContent) -> writing::Result<()> {
    required(&content.importer, "evidence importer")?;
    required(&content.provenance, "evidence provenance")?;
    if content.importer.chars().any(char::is_control) {
        return Err(invalid("evidence importer must be a single line"));
    }
    if content.entries.is_empty() {
        return Err(invalid("evidence pack needs at least one entry"));
    }
    for entry in &content.entries {
        required(&entry.reference, "evidence reference")?;
        required(&entry.note, "evidence note")?;
        required(&entry.claim, "evidence claim")?;
        if entry.reference.chars().any(char::is_control) {
            return Err(invalid("evidence reference must be a single line"));
        }
        Date::parse(
            &entry.accessed_on,
            &format_description!("[year]-[month]-[day]"),
        )
        .map_err(|_| invalid("evidence access date must use YYYY-MM-DD"))?;
    }
    Ok(())
}

impl Store {
    pub fn import_evidence_pack(&self, source: &Path) -> writing::Result<String> {
        let imported: EvidenceImport = writing::read_json(source)?;
        if imported.version != 1 {
            return Err(invalid("unsupported evidence-pack version"));
        }
        let evidence_content = EvidenceContent {
            importer: imported.importer,
            provenance: imported.provenance,
            entries: imported.entries,
        };
        validate_content(&evidence_content)?;
        let digest = content_hash(&evidence_content, source)?;
        let id = format!("evidence-{}", &digest[..20]);
        writing::validate_identifier(&id)?;
        let directory = self
            .directory(&["evidence"], true)?
            .ok_or_else(|| invalid("missing evidence directory"))?;
        let path = directory.join(format!("{id}.json"));
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(writing::Error::UnsafePath { path });
                }
                let existing = self.evidence_pack(&id)?;
                if pack_content(&existing) == evidence_content {
                    return Ok(id);
                }
                return Err(invalid("evidence-pack identity collision"));
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(writing::Error::Io { path, source }),
        }
        let pack = EvidencePack {
            evidence_pack_id: id.clone(),
            imported_at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .map_err(|source| writing::Error::Timestamp { source })?,
            importer: evidence_content.importer,
            provenance: evidence_content.provenance,
            entries: evidence_content.entries,
            content_hash: digest,
            hash_algorithm: "sha256".into(),
        };
        let bytes = serde_json::to_vec_pretty(&pack).map_err(|source| writing::Error::Json {
            path: path.clone(),
            source,
        })?;
        let mut pending =
            NamedTempFile::new_in(&directory).map_err(|source| writing::Error::Io {
                path: directory.clone(),
                source,
            })?;
        pending
            .write_all(&bytes)
            .map_err(|source| writing::Error::Io {
                path: path.clone(),
                source,
            })?;
        match writing::publish(pending, &path, 0o400) {
            Ok(()) => Ok(id),
            Err(writing::Error::Io { source, .. })
                if source.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                let existing = self.evidence_pack(&id)?;
                if pack_content(&existing) == pack_content(&pack) {
                    Ok(id)
                } else {
                    Err(invalid("evidence-pack identity collision"))
                }
            }
            Err(error) => Err(error),
        }
    }

    pub fn evidence_packs(&self) -> writing::Result<Vec<String>> {
        let Some(directory) = self.directory(&["evidence"], false)? else {
            return Ok(Vec::new());
        };
        let mut ids = Vec::new();
        for entry in fs::read_dir(&directory).map_err(|source| writing::Error::Io {
            path: directory.clone(),
            source,
        })? {
            let path = entry
                .map_err(|source| writing::Error::Io {
                    path: directory.clone(),
                    source,
                })?
                .path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|source| writing::Error::Io {
                path: path.clone(),
                source,
            })?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(writing::Error::UnsafePath { path });
            }
            let id = path
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| invalid("non-UTF-8 evidence filename"))?;
            writing::validate_identifier(id)?;
            ids.push(id.into());
        }
        ids.sort();
        Ok(ids)
    }

    pub fn evidence_pack(&self, id: &str) -> writing::Result<EvidencePack> {
        writing::validate_identifier(id)?;
        let directory = self
            .directory(&["evidence"], false)?
            .ok_or_else(|| invalid("evidence pack not found"))?;
        let path = directory.join(format!("{id}.json"));
        let pack: EvidencePack = writing::read_json(&path)?;
        if pack.evidence_pack_id != id {
            return Err(invalid("evidence-pack ID disagrees with its filename"));
        }
        if pack.hash_algorithm != "sha256" {
            return Err(invalid("unsupported evidence-pack hash algorithm"));
        }
        let evidence_content = pack_content(&pack);
        validate_content(&evidence_content)?;
        if content_hash(&evidence_content, &path)? != pack.content_hash {
            return Err(invalid("evidence-pack content hash does not match"));
        }
        OffsetDateTime::parse(&pack.imported_at, &Rfc3339)
            .map_err(|_| invalid("evidence import timestamp must use RFC 3339"))?;
        Ok(pack)
    }
}
