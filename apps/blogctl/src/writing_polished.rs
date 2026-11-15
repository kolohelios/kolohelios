//! Human polished decisions and explicit, no-clobber manuscript export.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::writing::{self, Store};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum PolishedDecision {
    Approve,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolishedRecord {
    pub polished_id: String,
    pub experiment_id: String,
    pub candidate_id: String,
    pub candidate_hash: String,
    pub body_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_id: Option<String>,
    pub decision: PolishedDecision,
    pub approver: String,
    pub timestamp: String,
    pub notes: String,
}

#[derive(Debug, Serialize)]
pub struct ExportOutcome {
    pub destination: PathBuf,
    pub approval_id: String,
    pub body_hash: String,
    pub created: bool,
}

#[derive(Serialize)]
struct DecisionIdentity<'a> {
    experiment_id: &'a str,
    candidate_id: &'a str,
    candidate_hash: &'a str,
    body_hash: &'a str,
    previous_id: Option<&'a str>,
    decision: PolishedDecision,
    approver: &'a str,
    notes: &'a str,
}

fn invalid(reason: impl Into<String>) -> writing::Error {
    writing::Error::Invalid {
        reason: reason.into(),
    }
}

fn validate_approver(approver: &str) -> writing::Result<()> {
    if approver.trim().is_empty() {
        return Err(invalid("polished approver must not be empty"));
    }
    if approver.chars().any(char::is_control) {
        return Err(invalid(
            "polished approver must not contain control characters",
        ));
    }
    Ok(())
}

fn identity(record: &PolishedRecord) -> DecisionIdentity<'_> {
    DecisionIdentity {
        experiment_id: &record.experiment_id,
        candidate_id: &record.candidate_id,
        candidate_hash: &record.candidate_hash,
        body_hash: &record.body_hash,
        previous_id: record.previous_id.as_deref(),
        decision: record.decision,
        approver: &record.approver,
        notes: &record.notes,
    }
}

fn identity_hash(identity: &DecisionIdentity<'_>, path: &Path) -> writing::Result<String> {
    let bytes = serde_json::to_vec(identity).map_err(|source| writing::Error::Json {
        path: path.into(),
        source,
    })?;
    Ok(writing::hash_bytes(&bytes))
}

fn absolute_destination(destination: &Path) -> writing::Result<PathBuf> {
    if destination.as_os_str().is_empty() || destination.file_name().is_none() {
        return Err(invalid("export destination must name a file"));
    }
    if destination
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(invalid(
            "export destination must not contain parent traversal",
        ));
    }
    let base = if destination.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir().map_err(|source| writing::Error::Io {
            path: PathBuf::from("."),
            source,
        })?
    };
    let mut absolute = base;
    for component in destination.components() {
        match component {
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                absolute.push(component.as_os_str());
            }
            Component::ParentDir => unreachable!("rejected above"),
        }
    }
    Ok(absolute)
}

fn safe_destination(store: &Store, destination: &Path) -> writing::Result<PathBuf> {
    let destination = absolute_destination(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("export destination has no parent directory"))?;
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
    let destination = parent.join(
        destination
            .file_name()
            .ok_or_else(|| invalid("export destination must name a file"))?,
    );
    let managed = store.root().join(".writing");
    if destination.starts_with(&managed) {
        return Err(writing::Error::UnsafePath { path: destination });
    }
    Ok(destination)
}

impl Store {
    pub fn record_polished_decision(
        &self,
        experiment: &str,
        candidate: &str,
        expected_candidate_hash: &str,
        decision: PolishedDecision,
        approver: String,
        notes: String,
    ) -> writing::Result<String> {
        validate_approver(&approver)?;
        let bytes = self.read_candidate(experiment, candidate)?;
        if writing::hash(&bytes) != expected_candidate_hash {
            return Err(invalid("candidate changed since polished review began"));
        }
        let document = self.candidate(experiment, candidate)?;
        if document.provenance.artifact_kind.as_deref() != Some("final-draft") {
            return Err(invalid(
                "polished decisions require a final-draft candidate",
            ));
        }
        let body_hash = writing::hash(&document.body);
        let existing_decisions = self.polished_decisions(experiment, candidate)?;
        if let Some(latest) = existing_decisions.last() {
            if latest.decision == decision
                && latest.approver == approver
                && latest.notes == notes
                && latest.candidate_hash == expected_candidate_hash
                && latest.body_hash == body_hash
            {
                return Ok(latest.polished_id.clone());
            }
        }
        let previous_id = existing_decisions
            .last()
            .map(|record| record.polished_id.as_str());
        let decision_identity = DecisionIdentity {
            experiment_id: experiment,
            candidate_id: candidate,
            candidate_hash: expected_candidate_hash,
            body_hash: &body_hash,
            previous_id,
            decision,
            approver: &approver,
            notes: &notes,
        };
        let digest = identity_hash(&decision_identity, Path::new("polished decision"))?;
        let id = format!("polished-{}", &digest[..20]);
        writing::validate_identifier(&id)?;
        let directory = self
            .directory(&["polished", experiment, candidate], true)?
            .ok_or_else(|| invalid("missing polished decision directory"))?;
        let path = directory.join(format!("{id}.json"));
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(writing::Error::UnsafePath { path });
                }
                let existing: PolishedRecord = writing::read_json(&path)?;
                self.validate_polished_record(&existing, experiment, candidate)?;
                if identity_hash(&identity(&existing), &path)? == digest {
                    return Ok(id);
                }
                return Err(invalid("polished decision identity collision"));
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(writing::Error::Io { path, source }),
        }
        let record = PolishedRecord {
            polished_id: id.clone(),
            experiment_id: experiment.into(),
            candidate_id: candidate.into(),
            candidate_hash: expected_candidate_hash.into(),
            body_hash,
            previous_id: previous_id.map(str::to_owned),
            decision,
            approver,
            timestamp: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .map_err(|source| writing::Error::Timestamp { source })?,
            notes,
        };
        let bytes = serde_json::to_vec_pretty(&record).map_err(|source| writing::Error::Json {
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
                let existing: PolishedRecord = writing::read_json(&path)?;
                self.validate_polished_record(&existing, experiment, candidate)?;
                if identity_hash(&identity(&existing), &path)? == digest {
                    Ok(id)
                } else {
                    Err(invalid("polished decision identity collision"))
                }
            }
            Err(error) => Err(error),
        }
    }

    pub fn polished_decisions(
        &self,
        experiment: &str,
        candidate: &str,
    ) -> writing::Result<Vec<PolishedRecord>> {
        writing::validate_identifier(experiment)?;
        writing::artifact_identifier(candidate)?;
        self.candidate(experiment, candidate)?;
        let directory = self.directory(&["polished", experiment, candidate], false)?;
        let ids = writing::list(directory.clone(), "json")?;
        let mut records = Vec::new();
        if let Some(directory) = directory {
            for id in ids {
                let record: PolishedRecord =
                    writing::read_json(&directory.join(format!("{id}.json")))?;
                if record.polished_id != id {
                    return Err(invalid("polished decision ID disagrees with its filename"));
                }
                self.validate_polished_record(&record, experiment, candidate)?;
                records.push(record);
            }
        }
        records.sort_by_cached_key(|record| {
            (
                OffsetDateTime::parse(&record.timestamp, &Rfc3339).ok(),
                record.polished_id.clone(),
            )
        });
        for (index, record) in records.iter().enumerate() {
            let expected = index
                .checked_sub(1)
                .and_then(|previous| records.get(previous))
                .map(|previous| previous.polished_id.as_str());
            if record.previous_id.as_deref() != expected {
                return Err(invalid("polished decision history is not a valid chain"));
            }
        }
        Ok(records)
    }

    fn validate_polished_record(
        &self,
        record: &PolishedRecord,
        experiment: &str,
        candidate: &str,
    ) -> writing::Result<()> {
        writing::validate_identifier(&record.polished_id)?;
        validate_approver(&record.approver)?;
        let document = self.candidate(experiment, candidate)?;
        let candidate_hash = writing::hash(&self.read_candidate(experiment, candidate)?);
        if record.experiment_id != experiment
            || record.candidate_id != candidate
            || record.candidate_hash != candidate_hash
            || record.body_hash != writing::hash(&document.body)
        {
            return Err(invalid(
                "polished decision does not match this candidate's current bytes or identity",
            ));
        }
        let digest = identity_hash(&identity(record), Path::new("polished decision"))?;
        if record.polished_id != format!("polished-{}", &digest[..20]) {
            return Err(invalid("polished decision ID does not match its content"));
        }
        OffsetDateTime::parse(&record.timestamp, &Rfc3339)
            .map_err(|_| invalid("polished decision timestamp must use RFC 3339"))?;
        Ok(())
    }

    pub fn export_polished(
        &self,
        experiment: &str,
        candidate: &str,
        approval_id: &str,
        destination: &Path,
    ) -> writing::Result<ExportOutcome> {
        writing::validate_identifier(approval_id)?;
        let decisions = self.polished_decisions(experiment, candidate)?;
        let latest = decisions
            .last()
            .ok_or_else(|| invalid("polished approval not found"))?;
        if latest.polished_id != approval_id {
            return Err(invalid(
                "export requires the latest polished decision to be the selected approval",
            ));
        }
        let approval = decisions
            .into_iter()
            .find(|record| record.polished_id == approval_id)
            .ok_or_else(|| invalid("polished approval not found"))?;
        if approval.decision != PolishedDecision::Approve {
            return Err(invalid("a rejection cannot authorize export"));
        }
        let document = self.candidate(experiment, candidate)?;
        let destination = safe_destination(self, destination)?;
        let body = document.body.as_bytes();
        let created = match fs::symlink_metadata(&destination) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(writing::Error::UnsafePath { path: destination });
                }
                let existing = writing::read_text(&destination)?;
                if existing.as_bytes() != body {
                    return Err(invalid(
                        "export destination already exists with different bytes",
                    ));
                }
                false
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let parent = destination
                    .parent()
                    .ok_or_else(|| invalid("export destination has no parent directory"))?;
                let mut pending =
                    NamedTempFile::new_in(parent).map_err(|source| writing::Error::Io {
                        path: parent.into(),
                        source,
                    })?;
                pending
                    .write_all(body)
                    .map_err(|source| writing::Error::Io {
                        path: destination.clone(),
                        source,
                    })?;
                match writing::publish(pending, &destination, 0o600) {
                    Ok(()) => true,
                    Err(writing::Error::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::AlreadyExists =>
                    {
                        let existing = writing::read_text(&destination)?;
                        if existing.as_bytes() != body {
                            return Err(invalid(
                                "export destination already exists with different bytes",
                            ));
                        }
                        false
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(source) => {
                return Err(writing::Error::Io {
                    path: destination,
                    source,
                });
            }
        };
        Ok(ExportOutcome {
            destination,
            approval_id: approval.polished_id,
            body_hash: approval.body_hash,
            created,
        })
    }
}
