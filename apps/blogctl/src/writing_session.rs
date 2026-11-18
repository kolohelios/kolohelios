//! Explicit, resumable state for consumer-configured writing workflows.

use std::fs::{self, Permissions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::writing::{self, ArtifactReference, Store};
use crate::writing_workflow::{self, WorkflowDefinition, WorkflowPass};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryEntry {
    pass: String,
    action: String,
    #[serde(flatten)]
    candidate: ArtifactReference,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PassAttempt {
    pass: String,
    action: String,
    candidate: Option<ArtifactReference>,
    assessment: Option<crate::writing_assessment::Assessment>,
    failure: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    version: u32,
    name: String,
    workflow_hash: String,
    pass: String,
    input: Option<ArtifactReference>,
    #[serde(default)]
    evidence: Vec<String>,
    experiment: Option<String>,
    selected: Option<ArtifactReference>,
    history: Vec<HistoryEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    attempts: Vec<PassAttempt>,
    #[serde(skip)]
    path: PathBuf,
}

fn invalid(reason: impl Into<String>) -> writing::Error {
    writing::Error::Invalid {
        reason: reason.into(),
    }
}

impl Session {
    pub fn open(
        store: &Store,
        name: &str,
        definition: &WorkflowDefinition,
    ) -> writing::Result<Self> {
        writing::validate_identifier(name)?;
        let directory = store
            .session_directory(true)?
            .ok_or_else(|| invalid("missing session directory"))?;
        let path = directory.join(format!("{name}.json"));
        let session = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(writing::Error::UnsafePath { path });
                }
                let mut session: Session = writing::read_json(&path)?;
                session.path = path;
                session
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Session {
                version: 1,
                name: name.into(),
                workflow_hash: definition.fingerprint().into(),
                pass: definition
                    .passes()
                    .first()
                    .ok_or_else(|| invalid("writing workflow needs at least one pass"))?
                    .name()
                    .into(),
                input: None,
                evidence: Vec::new(),
                experiment: None,
                selected: None,
                history: Vec::new(),
                attempts: Vec::new(),
                path,
            },
            Err(source) => return Err(writing::Error::Io { path, source }),
        };
        if session.version != 1
            || session.name != name
            || session.workflow_hash != definition.fingerprint()
        {
            return Err(invalid("unsupported or mismatched writing session"));
        }
        definition.pass(&session.pass)?;
        for reference in [session.input.as_ref(), session.selected.as_ref()]
            .into_iter()
            .flatten()
        {
            store.candidate(&reference.experiment_id, &reference.candidate_id)?;
        }
        for attempt in &session.attempts {
            definition.pass(&attempt.pass)?;
            if let Some(reference) = &attempt.candidate {
                store.candidate(&reference.experiment_id, &reference.candidate_id)?;
            }
        }
        for id in &session.evidence {
            store.evidence_pack(id)?;
        }
        session.save()?;
        Ok(session)
    }

    pub fn pass<'a>(
        &self,
        definition: &'a WorkflowDefinition,
    ) -> writing::Result<&'a WorkflowPass> {
        definition.pass(&self.pass)
    }

    pub fn pass_name(&self) -> &str {
        &self.pass
    }

    pub fn input(&self) -> Option<&ArtifactReference> {
        self.input.as_ref()
    }

    pub fn evidence(&self) -> &[String] {
        &self.evidence
    }

    pub fn experiment(&self) -> Option<&str> {
        self.experiment.as_deref()
    }

    pub fn selected(&self) -> Option<&ArtifactReference> {
        self.selected.as_ref()
    }

    pub fn prepare(
        &mut self,
        store: &Store,
        config_root: &Path,
        config: &Path,
    ) -> writing::Result<String> {
        let experiment = writing_workflow::prepare_with_evidence(
            store,
            config_root,
            config,
            &self.pass,
            self.input.as_ref(),
            &self.evidence,
        )?;
        self.experiment = Some(experiment.clone());
        self.save()?;
        Ok(experiment)
    }

    pub fn set_evidence(&mut self, store: &Store, ids: Vec<String>) -> writing::Result<()> {
        let mut unique = std::collections::BTreeSet::new();
        for id in &ids {
            if !unique.insert(id) {
                return Err(invalid(
                    "evidence-pack selections must not contain duplicates",
                ));
            }
            store.evidence_pack(id)?;
        }
        self.evidence = ids;
        self.experiment = None;
        self.selected = None;
        self.save()
    }

    pub fn select(&mut self, store: &Store, candidate_id: &str) -> writing::Result<()> {
        let experiment = self
            .experiment
            .as_ref()
            .ok_or_else(|| invalid("prepare the current pass before selecting a candidate"))?;
        store.candidate(experiment, candidate_id)?;
        let candidate = ArtifactReference {
            experiment_id: experiment.clone(),
            candidate_id: candidate_id.into(),
        };
        self.history.push(HistoryEntry {
            pass: self.pass.clone(),
            action: "selected".into(),
            candidate: candidate.clone(),
        });
        self.selected = Some(candidate);
        self.save()
    }

    pub fn record_output(
        &mut self,
        store: &Store,
        definition: &WorkflowDefinition,
        candidate_id: &str,
        action: &str,
    ) -> writing::Result<crate::writing_assessment::Assessment> {
        let experiment = self
            .experiment
            .as_ref()
            .ok_or_else(|| invalid("current pass has no prepared experiment"))?;
        let document = store.candidate(experiment, candidate_id)?;
        let assessment = crate::writing_assessment::assess(self.pass(definition)?, &document);
        self.attempts.push(PassAttempt {
            pass: self.pass.clone(),
            action: action.into(),
            candidate: Some(ArtifactReference {
                experiment_id: experiment.clone(),
                candidate_id: candidate_id.into(),
            }),
            assessment: Some(assessment.clone()),
            failure: None,
        });
        self.save().map_err(|error| invalid(format!("candidate {experiment}/{candidate_id} was saved but session/handoff persistence failed: {error}; no provider retry is authorized")))?;
        Ok(assessment)
    }

    /// Store only sanitized engine diagnostics, never provider bodies or credentials.
    pub fn record_failure(&mut self, failure: String) -> writing::Result<()> {
        self.attempts.push(PassAttempt {
            pass: self.pass.clone(),
            action: "generate".into(),
            candidate: None,
            assessment: None,
            failure: Some(failure),
        });
        self.save()
    }

    pub fn record_review(&mut self, action: &str) -> writing::Result<()> {
        let candidate = self
            .selected
            .clone()
            .ok_or_else(|| invalid("no candidate is selected"))?;
        self.history.push(HistoryEntry {
            pass: self.pass.clone(),
            action: action.into(),
            candidate,
        });
        self.save()
    }

    pub fn advance(&mut self, definition: &WorkflowDefinition) -> writing::Result<()> {
        let selected = self
            .selected
            .clone()
            .ok_or_else(|| invalid("select a candidate before advancing"))?;
        let index = definition
            .passes()
            .iter()
            .position(|pass| pass.name() == self.pass)
            .ok_or_else(|| invalid("session refers to an unknown pass"))?;
        let next = definition
            .passes()
            .get(index + 1)
            .ok_or_else(|| invalid("final pass reached; approval and promotion are separate"))?;
        self.history.push(HistoryEntry {
            pass: self.pass.clone(),
            action: "accepted-for-next-pass".into(),
            candidate: selected.clone(),
        });
        self.pass = next.name().into();
        self.input = Some(selected);
        self.evidence.clear();
        self.experiment = None;
        self.selected = None;
        self.save()
    }

    pub fn choose_pass(
        &mut self,
        definition: &WorkflowDefinition,
        pass: &str,
        input: Option<ArtifactReference>,
        store: &Store,
    ) -> writing::Result<()> {
        let target = definition.pass(pass)?;
        if let Some(reference) = &input {
            store.candidate(&reference.experiment_id, &reference.candidate_id)?;
        }
        if target.requires_input() && input.is_none() {
            return Err(invalid(
                "selected writing pass requires an explicit input candidate",
            ));
        }
        self.pass = pass.into();
        self.input = input;
        self.evidence.clear();
        self.experiment = None;
        self.selected = None;
        self.save()
    }

    fn handoff(&self) -> String {
        let reference = |value: Option<&ArtifactReference>| {
            value
                .map(|value| format!("{}/{}", value.experiment_id, value.candidate_id))
                .unwrap_or_else(|| "none".into())
        };
        let mut text = format!("# Writing session: {}\n\nWorkflow SHA-256: {}\n\nCurrent pass: {}\n\nExplicit input: {}\n\nSelected output: {}\n\nEvidence packs: {}\n\nPrepared experiment: {}\n\n", self.name, self.workflow_hash, self.pass, reference(self.input.as_ref()), reference(self.selected.as_ref()), self.evidence.join(", "), self.experiment.as_deref().unwrap_or("none"));
        text.push_str("## Recorded attempts\n\n");
        for (index, attempt) in self.attempts.iter().enumerate() {
            text.push_str(&format!(
                "{}. {} / {} / output: {}\n",
                index + 1,
                attempt.pass,
                attempt.action,
                reference(attempt.candidate.as_ref())
            ));
            if let Some(assessment) = &attempt.assessment {
                text.push_str(&format!("   Mechanical status: {:?}; words: {}. Editorial approval is not established.\n", assessment.status, assessment.words));
                for finding in &assessment.findings {
                    text.push_str(&format!("   - {}: {}\n", finding.code, finding.detail));
                }
            }
            if let Some(failure) = &attempt.failure {
                text.push_str(&format!("   Failure: {failure}\n"));
            }
        }
        text.push_str("\n## Boundary\n\nThis handoff is derived from the session JSON, which remains authoritative. No recorded output is selected automatically. Review the explicit output and findings before selection. No retry, advancement, polished approval, or export is authorized by this handoff.\n");
        text
    }

    fn save(&self) -> writing::Result<()> {
        let handoff = self.path.with_extension("handoff.md");
        check_target(&handoff)?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|source| writing::Error::Json {
            path: self.path.clone(),
            source,
        })?;
        write_private(&self.path, &bytes)?;
        write_private(&handoff, self.handoff().as_bytes()).map_err(|_| invalid("session JSON was saved but the private handoff could not be refreshed; no provider retry is authorized"))
    }
}

fn check_target(path: &Path) -> writing::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            Err(writing::Error::UnsafePath { path: path.into() })
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(writing::Error::Io {
            path: path.into(),
            source,
        }),
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> writing::Result<()> {
    check_target(path)?;
    let directory = path
        .parent()
        .ok_or_else(|| invalid("session path has no parent"))?;
    let mut pending = NamedTempFile::new_in(directory).map_err(|source| writing::Error::Io {
        path: directory.into(),
        source,
    })?;
    pending
        .write_all(bytes)
        .map_err(|source| writing::Error::Io {
            path: path.into(),
            source,
        })?;
    pending
        .as_file()
        .set_permissions(Permissions::from_mode(0o600))
        .map_err(|source| writing::Error::Io {
            path: path.into(),
            source,
        })?;
    pending
        .as_file()
        .sync_all()
        .map_err(|source| writing::Error::Io {
            path: path.into(),
            source,
        })?;
    check_target(path)?;
    fs::rename(pending.path(), path).map_err(|source| writing::Error::Io {
        path: path.into(),
        source,
    })?;
    fs::set_permissions(path, Permissions::from_mode(0o600)).map_err(|source| {
        writing::Error::Io {
            path: path.into(),
            source,
        }
    })?;
    fs::File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| writing::Error::Io {
            path: directory.into(),
            source,
        })
}
