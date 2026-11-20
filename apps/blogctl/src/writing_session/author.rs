//! Durable review focus, interrupted working copies, and explicit policy forks.

use super::*;
use crate::writing_revision::{EditorialRevision, RevisionPreparation};
use crate::writing_workflow::GuardChange;
use std::os::unix::fs::DirBuilderExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBinding {
    pub config_root: PathBuf,
    pub config: PathBuf,
    pub model: String,
    pub workflow_json: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contributor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkingRevision {
    pub file: PathBuf,
    pub parent: ArtifactReference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyOrigin {
    pub session: String,
    pub archive_hash: String,
    pub changes: Vec<GuardChange>,
}

impl Session {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn binding(&self) -> Option<&SessionBinding> {
        self.binding.as_ref()
    }
    pub fn contributor(&self) -> Option<&str> {
        self.binding
            .as_ref()
            .and_then(|binding| binding.contributor.as_deref())
    }
    pub fn refresh(&mut self) -> writing::Result<()> {
        self.save()
    }
    pub fn set_model(&mut self, model: &str) -> writing::Result<()> {
        if model.trim().is_empty() || model.chars().any(char::is_control) {
            return Err(invalid("an explicit model is required"));
        }
        self.binding
            .as_mut()
            .ok_or_else(|| invalid("register a legacy session before setting its model"))?
            .model = model.into();
        self.save()
    }
    pub fn set_contributor(&mut self, contributor: &str) -> writing::Result<()> {
        if contributor.trim().is_empty() || contributor.chars().any(char::is_control) {
            return Err(invalid("invalid contributor label"));
        }
        self.binding
            .as_mut()
            .ok_or_else(|| invalid("register a legacy session before saving an editor label"))?
            .contributor = Some(contributor.into());
        self.save()
    }
    pub fn working_revision(&self) -> Option<&WorkingRevision> {
        self.working_revision.as_ref()
    }

    /// Legacy focus comes only from an exact active-pass journal entry, never
    /// directory ordering. Focus is not selection or an accepted review.
    pub fn review_target(&self) -> Option<&ArtifactReference> {
        self.review_target
            .as_ref()
            .or(self.selected.as_ref())
            .or_else(|| {
                self.attempts.iter().rev().find_map(|attempt| {
                    attempt.candidate.as_ref().filter(|candidate| {
                        attempt.pass == self.pass
                            && self.experiment.as_ref() == Some(&candidate.experiment_id)
                    })
                })
            })
    }

    /// Read-only discovery/status; does not refresh JSON or the derived handoff.
    pub fn read(store: &Store, name: &str) -> writing::Result<Self> {
        writing::validate_identifier(name)?;
        let directory = store
            .session_directory(false)?
            .ok_or_else(|| invalid("no writing sessions"))?;
        let path = directory.join(format!("{name}.json"));
        check_target(&path)?;
        let mode = fs::metadata(&path)
            .map_err(|source| writing::Error::Io {
                path: path.clone(),
                source,
            })?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o600 {
            return Err(invalid("session JSON must have mode 0600"));
        }
        let mut session: Self = writing::read_json(&path)?;
        session.path = path;
        if session.version != 1 || session.name != name {
            return Err(invalid("unsupported or mismatched writing session"));
        }
        session.validate_author_state(store)?;
        Ok(session)
    }

    pub fn names(store: &Store) -> writing::Result<Vec<String>> {
        let Some(directory) = store.session_directory(false)? else {
            return Ok(Vec::new());
        };
        let mut names = Vec::new();
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
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let name = path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| invalid("invalid session filename"))?;
                writing::validate_identifier(name)?;
                check_target(&path)?;
                names.push(name.into());
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn bind(
        &mut self,
        store: &Store,
        root: &Path,
        config: &Path,
        model: &str,
    ) -> writing::Result<()> {
        writing_workflow::validate_store_boundary(store, root)?;
        let definition = writing_workflow::definition(root, config)?;
        if definition.fingerprint() != self.workflow_hash {
            return Err(invalid(
                "configuration does not match this frozen session policy",
            ));
        }
        definition.pass(&self.pass)?;
        if model.trim().is_empty() || model.chars().any(char::is_control) {
            return Err(invalid("an explicit model is required"));
        }
        // Preserve exact pre-registration bytes, including legacy formatting.
        check_target(&self.path)?;
        archive(
            store,
            &fs::read(&self.path).map_err(|source| writing::Error::Io {
                path: self.path.clone(),
                source,
            })?,
        )?;
        self.binding = Some(SessionBinding {
            config_root: root.canonicalize().map_err(|source| writing::Error::Io {
                path: root.into(),
                source,
            })?,
            config: config.into(),
            model: model.into(),
            workflow_json: definition.snapshot().into(),
            contributor: self
                .binding
                .as_ref()
                .and_then(|binding| binding.contributor.clone()),
        });
        self.save()
    }

    /// Explicitly relocate the permitted context root, without replacing the
    /// frozen policy with a consumer file (important for private policy forks).
    pub fn relocate(&mut self, store: &Store, root: &Path) -> writing::Result<()> {
        self.bound_definition()?;
        writing_workflow::validate_store_boundary(store, root)?;
        let root = root.canonicalize().map_err(|source| writing::Error::Io {
            path: root.into(),
            source,
        })?;
        check_target(&self.path)?;
        archive(
            store,
            &fs::read(&self.path).map_err(|source| writing::Error::Io {
                path: self.path.clone(),
                source,
            })?,
        )?;
        self.binding
            .as_mut()
            .ok_or_else(|| invalid("missing session binding"))?
            .config_root = root;
        self.save()
    }

    pub fn bound_definition(&self) -> writing::Result<WorkflowDefinition> {
        let binding = self.binding.as_ref().ok_or_else(|| invalid("legacy session needs explicit --config-root, --config and --model registration once"))?;
        let definition = writing_workflow::from_snapshot(&binding.workflow_json)?;
        if definition.fingerprint() != self.workflow_hash {
            return Err(invalid("bound session policy hash mismatch"));
        }
        definition.pass(&self.pass)?;
        Ok(definition)
    }

    pub(super) fn validate_author_state(&self, store: &Store) -> writing::Result<()> {
        writing::validate_identifier(&self.pass)?;
        for candidate in [
            self.input.as_ref(),
            self.selected.as_ref(),
            self.review_target.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            store.candidate(&candidate.experiment_id, &candidate.candidate_id)?;
        }
        for attempt in &self.attempts {
            writing::validate_identifier(&attempt.pass)?;
            if let Some(candidate) = &attempt.candidate {
                store.candidate(&candidate.experiment_id, &candidate.candidate_id)?;
            }
        }
        for id in &self.evidence {
            store.evidence_pack(id)?;
        }
        if let Some(binding) = &self.binding {
            if !binding.config_root.is_absolute()
                || binding.config_root.starts_with("/nix/store")
                || binding
                    .config_root
                    .components()
                    .any(|part| part == std::path::Component::ParentDir)
                || store.root().starts_with(&binding.config_root)
                || binding.model.trim().is_empty()
                || binding.model.chars().any(char::is_control)
            {
                return Err(invalid("invalid session binding"));
            }
            self.bound_definition()?;
        }
        if let Some(working) = &self.working_revision {
            if self.review_target() != Some(&working.parent)
                || !working
                    .file
                    .starts_with(store.root().join("review-working"))
                || working
                    .file
                    .components()
                    .any(|part| part == std::path::Component::ParentDir)
            {
                return Err(invalid(
                    "working revision is not bound to this review target and private store",
                ));
            }
        }
        if let Some(origin) = &self.policy_origin {
            writing::validate_identifier(&origin.session)?;
            if origin.archive_hash.len() != 64
                || !origin
                    .archive_hash
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(invalid("invalid session archive hash"));
            }
            let directory = store
                .directory(&["session-history"], false)?
                .ok_or_else(|| invalid("missing policy-fork history"))?;
            let path = directory.join(format!("{}.json", origin.archive_hash));
            check_target(&path)?;
            if writing::hash(&writing::read_text(&path)?) != origin.archive_hash {
                return Err(invalid("policy-fork history hash mismatch"));
            }
        }
        Ok(())
    }

    pub(super) fn require_no_working_revision(&self) -> writing::Result<()> {
        if self.working_revision.is_some() {
            return Err(invalid(
                "finish or abandon the pending working copy before changing review state",
            ));
        }
        Ok(())
    }

    /// Explicitly inspect an existing current-pass output or a direct editorial
    /// child of the current target. This never accepts that output.
    pub fn focus(
        &mut self,
        store: &Store,
        definition: &WorkflowDefinition,
        candidate: ArtifactReference,
    ) -> writing::Result<crate::writing_assessment::Assessment> {
        self.require_no_working_revision()?;
        let document = store.candidate(&candidate.experiment_id, &candidate.candidate_id)?;
        let current_output = self.experiment.as_ref() == Some(&candidate.experiment_id);
        let revision = document.provenance.editorial_revision.is_some()
            && document
                .provenance
                .parent
                .as_ref()
                .is_some_and(|parent| Some(parent) == self.review_target());
        if !current_output && !revision {
            return Err(invalid("review focus must be a current experiment output or a direct revision of the current target"));
        }
        let assessment = crate::writing_assessment::assess(self.pass(definition)?, &document);
        self.review_target = Some(candidate.clone());
        self.attempts.push(PassAttempt {
            pass: self.pass.clone(),
            action: "focus".into(),
            candidate: Some(candidate),
            assessment: Some(assessment.clone()),
            failure: None,
            workflow_hash: Some(self.workflow_hash.clone()),
        });
        self.save()?;
        Ok(assessment)
    }

    pub fn begin_revision(&mut self, store: &Store) -> writing::Result<RevisionPreparation> {
        if let Some(working) = &self.working_revision {
            let prepared = store.inspect_revision(&working.file)?;
            if prepared.parent != working.parent {
                return Err(invalid(
                    "working copy parent no longer matches this session",
                ));
            }
            return Ok(prepared);
        }
        let parent = self.review_target().cloned().ok_or_else(|| {
            invalid("no output under review; generate, import or focus an explicit output first")
        })?;
        let directory = store.root().join("review-working");
        match fs::symlink_metadata(&directory) {
            Ok(metadata)
                if metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && metadata.permissions().mode() & 0o777 == 0o700 => {}
            Ok(_) => return Err(writing::Error::UnsafePath { path: directory }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::DirBuilder::new()
                .mode(0o700)
                .create(&directory)
                .map_err(|source| writing::Error::Io {
                    path: directory.clone(),
                    source,
                })?,
            Err(source) => {
                return Err(writing::Error::Io {
                    path: directory,
                    source,
                })
            }
        }
        let file = directory.join(format!(
            "{}-{}-{}.md",
            self.name,
            self.history.len(),
            self.attempts.len()
        ));
        if fs::symlink_metadata(&file).is_ok() {
            return Err(invalid(
                "working-copy destination already exists; no file was overwritten",
            ));
        }
        self.review_target = Some(parent.clone());
        self.working_revision = Some(WorkingRevision {
            file: file.clone(),
            parent: parent.clone(),
        });
        // Persist intent first. A crash between the two file publications is
        // visible on restart, not permission to overwrite or discard an edit.
        self.save()?;
        store.prepare_revision(&parent.experiment_id, &parent.candidate_id, &file)
    }

    /// Close an unchanged editor visit without inventing a revision or identity.
    pub fn close_unchanged_revision(&mut self, store: &Store) -> writing::Result<bool> {
        let working = self
            .working_revision
            .clone()
            .ok_or_else(|| invalid("no pending working copy"))?;
        if store.inspect_revision(&working.file)?.parent != working.parent
            || self.review_target() != Some(&working.parent)
        {
            return Err(invalid("working copy parent mismatch"));
        }
        let parent =
            store.candidate(&working.parent.experiment_id, &working.parent.candidate_id)?;
        if store.revision_body(&working.file)? != parent.body {
            return Ok(false);
        }
        self.working_revision = None;
        self.history.push(HistoryEntry {
            pass: self.pass.clone(),
            action: "working-copy-unchanged".into(),
            candidate: working.parent,
        });
        self.save()?;
        Ok(true)
    }

    pub fn finish_revision(
        &mut self,
        store: &Store,
        definition: &WorkflowDefinition,
        revision: EditorialRevision,
    ) -> writing::Result<ArtifactReference> {
        self.pass(definition)?;
        let working = self
            .working_revision
            .clone()
            .ok_or_else(|| invalid("no pending working copy"))?;
        if store.inspect_revision(&working.file)?.parent != working.parent
            || self.review_target() != Some(&working.parent)
        {
            return Err(invalid(
                "working copy parent no longer matches the review target",
            ));
        }
        revision.validate()?;
        if self.close_unchanged_revision(store)? {
            return Ok(working.parent);
        }
        let id = store.import_revision(&working.file, revision)?;
        let candidate = ArtifactReference {
            experiment_id: working.parent.experiment_id,
            candidate_id: id,
        };
        let document = store.candidate(&candidate.experiment_id, &candidate.candidate_id)?;
        let assessment = crate::writing_assessment::assess(self.pass(definition)?, &document);
        self.review_target = Some(candidate.clone());
        self.working_revision = None;
        self.attempts.push(PassAttempt {
            pass: self.pass.clone(),
            action: "revision".into(),
            candidate: Some(candidate.clone()),
            assessment: Some(assessment),
            failure: None,
            workflow_hash: Some(self.workflow_hash.clone()),
        });
        self.save().map_err(|error| {
            invalid(format!(
                "revision {}/{} was saved but session persistence failed: {error}",
                candidate.experiment_id, candidate.candidate_id
            ))
        })?;
        Ok(candidate)
    }

    /// Abandon only the tracked intent; preserve both files for recovery.
    pub fn abandon_revision(&mut self) -> writing::Result<()> {
        let working = self
            .working_revision
            .take()
            .ok_or_else(|| invalid("no pending working copy"))?;
        self.history.push(HistoryEntry {
            pass: self.pass.clone(),
            action: "working-copy-abandoned-files-preserved".into(),
            candidate: working.parent,
        });
        self.save()
    }

    pub fn accept_target(&mut self, store: &Store) -> writing::Result<()> {
        self.require_no_working_revision()?;
        let candidate = self
            .review_target()
            .cloned()
            .ok_or_else(|| invalid("no output under review"))?;
        store.candidate(&candidate.experiment_id, &candidate.candidate_id)?;
        self.history.push(HistoryEntry {
            pass: self.pass.clone(),
            action: "selected".into(),
            candidate: candidate.clone(),
        });
        self.selected = Some(candidate);
        self.save()
    }

    /// Fork rather than changing an existing frozen policy or its assessments.
    pub fn fork_guards(
        &self,
        store: &Store,
        name: &str,
        changes: Vec<GuardChange>,
    ) -> writing::Result<Self> {
        self.require_no_working_revision()?;
        writing::validate_identifier(name)?;
        let definition = self.bound_definition()?;
        let target = self
            .review_target()
            .ok_or_else(|| invalid("a review draft is required for a protection decision"))?;
        let body = store
            .candidate(&target.experiment_id, &target.candidate_id)?
            .body;
        let document = store.candidate(&target.experiment_id, &target.candidate_id)?;
        if document.provenance.editorial_revision.is_none() {
            return Err(invalid(
                "protection changes require an explicit editorial checkpoint",
            ));
        }
        let updated = definition.change_guards(&changes, &body)?;
        let mut fork = self.clone();
        fork.name = name.into();
        fork.path = self.path.with_file_name(format!("{name}.json"));
        for path in [&fork.path, &fork.path.with_extension("handoff.md")] {
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    return Err(invalid(
                        "policy fork destination already exists or is unsafe",
                    ))
                }
            }
        }
        check_target(&self.path)?;
        let original = fs::read(&self.path).map_err(|source| writing::Error::Io {
            path: self.path.clone(),
            source,
        })?;
        let archive_hash = archive(store, &original)?;
        for attempt in &mut fork.attempts {
            if attempt.workflow_hash.is_none() {
                attempt.workflow_hash = Some(self.workflow_hash.clone());
            }
        }
        fork.binding
            .as_mut()
            .ok_or_else(|| invalid("missing session binding"))?
            .workflow_json = updated.snapshot().into();
        fork.policy_origin = Some(PolicyOrigin {
            session: self.name.clone(),
            archive_hash,
            changes,
        });
        fork.workflow_hash = updated.fingerprint().into();
        fork.experiment = None;
        fork.selected = None;
        fork.review_target = Some(target.clone());
        // Publish the new JSON without clobbering another session.
        check_target(&fork.path.with_extension("handoff.md"))?;
        let bytes = serde_json::to_vec_pretty(&fork).map_err(|source| writing::Error::Json {
            path: fork.path.clone(),
            source,
        })?;
        publish_private_new(&fork.path, &bytes, 0o600)?;
        write_private(
            &fork.path.with_extension("handoff.md"),
            fork.handoff().as_bytes(),
        )?;
        Ok(fork)
    }
}

fn publish_private_new(path: &Path, bytes: &[u8], mode: u32) -> writing::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| invalid("missing private parent"))?;
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
    writing::publish(pending, path, mode)
}

fn archive(store: &Store, bytes: &[u8]) -> writing::Result<String> {
    let hash = writing::hash_bytes(bytes);
    let directory = store
        .directory(&["session-history"], true)?
        .ok_or_else(|| invalid("missing session history directory"))?;
    let path = directory.join(format!("{hash}.json"));
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if fs::read(&path).map_err(|source| writing::Error::Io {
                path: path.clone(),
                source,
            })? != bytes
            {
                return Err(invalid("session archive integrity mismatch"));
            }
        }
        Ok(_) => return Err(writing::Error::UnsafePath { path }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            publish_private_new(&path, bytes, 0o400)?
        }
        Err(source) => return Err(writing::Error::Io { path, source }),
    }
    Ok(hash)
}
