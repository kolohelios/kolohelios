//! Explicit private working copies, immutable editorial checkpoints, and diffs.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use similar::TextDiff;
use tempfile::NamedTempFile;

use crate::writing::{self, ArtifactReference, CandidateContext, GenerationMetadata, Store};

const SIDECAR_SUFFIX: &str = ".blogctl-revision.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum OriginKind {
    Generated,
    Imported,
    HumanRevision,
    AssistedRevision,
    MechanicalCleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum RevisionKind {
    Substantive,
    Mechanical,
    Transcription,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorialRevision {
    pub origin_kind: OriginKind,
    pub revision_kind: RevisionKind,
    pub contributors: Vec<String>,
    pub notes: String,
}

impl EditorialRevision {
    pub(crate) fn validate(&self) -> writing::Result<()> {
        if self.contributors.is_empty() {
            return Err(invalid(
                "editorial revision requires at least one contributor",
            ));
        }
        for contributor in &self.contributors {
            if contributor.trim().is_empty() {
                return Err(invalid("editorial contributors must not be empty"));
            }
            if contributor.chars().any(char::is_control) {
                return Err(invalid(
                    "editorial contributors must not contain control characters",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct RevisionPreparation {
    pub working_file: PathBuf,
    pub sidecar: PathBuf,
    pub parent: ArtifactReference,
    pub candidate_hash: String,
    pub body_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionSidecar {
    version: u32,
    working_file: PathBuf,
    parent: ArtifactReference,
    candidate_hash: String,
    body_hash: String,
}

#[derive(Debug, Serialize)]
pub struct RevisionDiff {
    pub from: ArtifactReference,
    pub to: ArtifactReference,
    pub from_body_hash: String,
    pub to_body_hash: String,
    pub changed: bool,
    pub unified_diff: String,
}

fn invalid(reason: impl Into<String>) -> writing::Error {
    writing::Error::Invalid {
        reason: reason.into(),
    }
}

fn absolute_path(path: &Path) -> writing::Result<PathBuf> {
    if path.as_os_str().is_empty() || path.file_name().is_none() {
        return Err(invalid("revision working path must name a file"));
    }
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(invalid(
            "revision working path must not contain parent traversal",
        ));
    }
    let mut absolute = if path.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir().map_err(|source| writing::Error::Io {
            path: PathBuf::from("."),
            source,
        })?
    };
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                absolute.push(component.as_os_str());
            }
            Component::ParentDir => unreachable!("parent traversal rejected above"),
        }
    }
    Ok(absolute)
}

fn safe_working_path(store: &Store, path: &Path, must_exist: bool) -> writing::Result<PathBuf> {
    let path = absolute_path(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("revision working path has no parent directory"))?;
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(|source| writing::Error::Io {
            path: current.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(writing::Error::UnsafePath { path: current });
        }
    }
    let parent = parent.canonicalize().map_err(|source| writing::Error::Io {
        path: parent.into(),
        source,
    })?;
    if parent.starts_with("/nix/store") {
        return Err(writing::Error::UnsafePath { path: parent });
    }
    let path = parent.join(
        path.file_name()
            .ok_or_else(|| invalid("revision working path must name a file"))?,
    );
    if path.starts_with(store.root().join(".writing")) {
        return Err(writing::Error::UnsafePath { path });
    }
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(writing::Error::UnsafePath { path });
            }
            if !must_exist {
                return Err(invalid("revision working path already exists"));
            }
            if metadata.permissions().mode() & 0o777 != 0o600 {
                return Err(invalid("revision working files must have mode 0600"));
            }
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            if must_exist {
                return Err(writing::Error::Io { path, source });
            }
        }
        Err(source) => return Err(writing::Error::Io { path, source }),
    }
    Ok(path)
}

fn sidecar_path(working_file: &Path) -> writing::Result<PathBuf> {
    let name = working_file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("revision working filename must be UTF-8"))?;
    Ok(working_file.with_file_name(format!("{name}{SIDECAR_SUFFIX}")))
}

fn write_new(path: &Path, bytes: &[u8]) -> writing::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("revision working path has no parent directory"))?;
    let mut pending = NamedTempFile::new_in(parent).map_err(|source| writing::Error::Io {
        path: parent.into(),
        source,
    })?;
    pending
        .write_all(bytes)
        .map_err(|source| writing::Error::Io {
            path: path.into(),
            source,
        })?;
    writing::publish(pending, path, 0o600)
}

fn read_private_text(path: &Path) -> writing::Result<String> {
    let metadata = fs::symlink_metadata(path).map_err(|source| writing::Error::Io {
        path: path.into(),
        source,
    })?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(writing::Error::UnsafePath { path: path.into() });
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|source| writing::Error::Io {
            path: path.into(),
            source,
        })?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut file, &mut text).map_err(|source| writing::Error::Io {
        path: path.into(),
        source,
    })?;
    Ok(text)
}

impl Store {
    pub fn prepare_revision(
        &self,
        experiment: &str,
        candidate: &str,
        output: &Path,
    ) -> writing::Result<RevisionPreparation> {
        let working_file = safe_working_path(self, output, false)?;
        let sidecar = sidecar_path(&working_file)?;
        safe_working_path(self, &sidecar, false)?;
        let candidate_bytes = self.read_candidate(experiment, candidate)?;
        let document = self.candidate(experiment, candidate)?;
        let parent = ArtifactReference {
            experiment_id: experiment.into(),
            candidate_id: candidate.into(),
        };
        let record = RevisionSidecar {
            version: 1,
            working_file: working_file.clone(),
            parent: parent.clone(),
            candidate_hash: writing::hash(&candidate_bytes),
            body_hash: writing::hash(&document.body),
        };
        let sidecar_bytes =
            serde_json::to_vec_pretty(&record).map_err(|source| writing::Error::Json {
                path: sidecar.clone(),
                source,
            })?;
        write_new(&working_file, document.body.as_bytes())?;
        write_new(&sidecar, &sidecar_bytes)?;
        Ok(RevisionPreparation {
            working_file,
            sidecar,
            parent,
            candidate_hash: record.candidate_hash,
            body_hash: record.body_hash,
        })
    }

    pub fn import_revision(
        &self,
        working_file: &Path,
        revision: EditorialRevision,
    ) -> writing::Result<String> {
        revision.validate()?;
        let working_file = safe_working_path(self, working_file, true)?;
        let sidecar = safe_working_path(self, &sidecar_path(&working_file)?, true)?;
        let record: RevisionSidecar =
            serde_json::from_str(&read_private_text(&sidecar)?).map_err(|source| {
                writing::Error::Json {
                    path: sidecar.clone(),
                    source,
                }
            })?;
        if record.version != 1 || record.working_file != working_file {
            return Err(invalid("revision sidecar does not match this working file"));
        }
        let parent_bytes =
            self.read_candidate(&record.parent.experiment_id, &record.parent.candidate_id)?;
        if writing::hash(&parent_bytes) != record.candidate_hash {
            return Err(invalid("revision parent changed after preparation"));
        }
        let parent = self.candidate(&record.parent.experiment_id, &record.parent.candidate_id)?;
        if writing::hash(&parent.body) != record.body_hash {
            return Err(invalid("revision parent body changed after preparation"));
        }
        let body = read_private_text(&working_file)?;
        if body.trim().is_empty() {
            return Err(invalid("revision body must not be empty"));
        }
        for id in self.candidates(&record.parent.experiment_id)? {
            let existing = self.candidate(&record.parent.experiment_id, &id)?;
            if existing.body == body
                && existing.provenance.parent.as_ref() == Some(&record.parent)
                && existing.provenance.editorial_revision.as_ref() == Some(&revision)
            {
                return Ok(id);
            }
        }
        let generation = GenerationMetadata {
            provider: "editorial-checkpoint".into(),
            model: "not-applicable".into(),
            ai_provenance: "Caller-asserted editorial checkpoint; inspect editorial_revision for contribution provenance.".into(),
            parameters: BTreeMap::new(),
        };
        let context = CandidateContext {
            artifact_kind: parent.provenance.artifact_kind.clone(),
            parent: Some(record.parent),
            editorial_revision: Some(revision),
        };
        self.record_candidate_with_context(&parent.request_snapshot, &generation, &body, &context)
    }

    pub fn revision_diff(
        &self,
        from: &ArtifactReference,
        to: &ArtifactReference,
    ) -> writing::Result<RevisionDiff> {
        let from_document = self.candidate(&from.experiment_id, &from.candidate_id)?;
        let to_document = self.candidate(&to.experiment_id, &to.candidate_id)?;
        let diff = TextDiff::from_lines(&from_document.body, &to_document.body)
            .unified_diff()
            .header(
                &format!("{}/{}", from.experiment_id, from.candidate_id),
                &format!("{}/{}", to.experiment_id, to.candidate_id),
            )
            .to_string();
        Ok(RevisionDiff {
            from: from.clone(),
            to: to.clone(),
            from_body_hash: writing::hash(&from_document.body),
            to_body_hash: writing::hash(&to_document.body),
            changed: from_document.body != to_document.body,
            unified_diff: diff,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_paths_are_normalized_without_traversal() {
        let relative = absolute_path(Path::new("revision.md")).unwrap();
        assert_eq!(
            relative,
            std::env::current_dir().unwrap().join("revision.md")
        );
        assert!(absolute_path(Path::new("")).is_err());
        assert!(absolute_path(Path::new("nested/../revision.md")).is_err());
    }
}
