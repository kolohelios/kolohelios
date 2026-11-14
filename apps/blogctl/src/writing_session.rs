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
pub struct Session {
    version: u32,
    name: String,
    workflow_hash: String,
    pass: String,
    input: Option<ArtifactReference>,
    experiment: Option<String>,
    selected: Option<ArtifactReference>,
    history: Vec<HistoryEntry>,
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
                experiment: None,
                selected: None,
                history: Vec::new(),
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
        let experiment =
            writing_workflow::prepare(store, config_root, config, &self.pass, self.input.as_ref())?;
        self.experiment = Some(experiment.clone());
        self.save()?;
        Ok(experiment)
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
        self.experiment = None;
        self.selected = None;
        self.save()
    }

    fn save(&self) -> writing::Result<()> {
        let directory = self
            .path
            .parent()
            .ok_or_else(|| invalid("session path has no parent"))?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|source| writing::Error::Json {
            path: self.path.clone(),
            source,
        })?;
        let mut pending =
            NamedTempFile::new_in(directory).map_err(|source| writing::Error::Io {
                path: directory.into(),
                source,
            })?;
        pending
            .write_all(&bytes)
            .map_err(|source| writing::Error::Io {
                path: self.path.clone(),
                source,
            })?;
        pending
            .as_file()
            .set_permissions(Permissions::from_mode(0o600))
            .map_err(|source| writing::Error::Io {
                path: self.path.clone(),
                source,
            })?;
        pending
            .as_file()
            .sync_all()
            .map_err(|source| writing::Error::Io {
                path: self.path.clone(),
                source,
            })?;
        if let Ok(metadata) = fs::symlink_metadata(&self.path) {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(writing::Error::UnsafePath {
                    path: self.path.clone(),
                });
            }
        }
        fs::rename(pending.path(), &self.path).map_err(|source| writing::Error::Io {
            path: self.path.clone(),
            source,
        })?;
        fs::set_permissions(&self.path, Permissions::from_mode(0o600)).map_err(|source| {
            writing::Error::Io {
                path: self.path.clone(),
                source,
            }
        })
    }
}
