//! Deterministic preparation of consumer-configured writing passes.
//!
//! This module reads explicit local configuration and context files, then
//! prepares an immutable writing experiment. It never resolves credentials,
//! calls a provider, selects a candidate, or advances a workflow.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::writing::{self, ArtifactReference, Experiment, Store};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDefinition {
    version: u32,
    source_experiment: String,
    instructions: String,
    fiction: bool,
    #[serde(default)]
    context_sets: BTreeMap<String, Vec<ContextSource>>,
    passes: Vec<WorkflowPass>,
    #[serde(skip)]
    fingerprint: String,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum PassMode {
    #[default]
    Generate,
    ImportOnly,
}

impl PassMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generate => "generate",
            Self::ImportOnly => "import-only",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPass {
    name: String,
    #[serde(default)]
    mode: PassMode,
    requires_input: bool,
    #[serde(default)]
    requires_evidence: bool,
    approach: String,
    #[serde(default)]
    context: Vec<String>,
    artifact_kind: Option<String>,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    min_words: Option<usize>,
    max_words: Option<usize>,
    #[serde(default)]
    forbid_workflow_metadata: bool,
    #[serde(default)]
    require_complete_ending: bool,
}

impl WorkflowPass {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn mode(&self) -> PassMode {
        self.mode
    }

    pub fn requires_input(&self) -> bool {
        self.requires_input
    }

    pub fn requires_evidence(&self) -> bool {
        self.requires_evidence
    }

    pub fn artifact_kind(&self) -> &str {
        self.artifact_kind.as_deref().unwrap_or(&self.name)
    }

    pub fn max_tokens(&self) -> Option<u32> {
        self.max_tokens
    }

    pub fn temperature(&self) -> Option<f64> {
        self.temperature
    }

    pub fn min_words(&self) -> Option<usize> {
        self.min_words
    }

    pub fn max_words(&self) -> Option<usize> {
        self.max_words
    }

    pub fn forbid_workflow_metadata(&self) -> bool {
        self.forbid_workflow_metadata
    }

    pub fn require_complete_ending(&self) -> bool {
        self.require_complete_ending
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextSource {
    label: String,
    path: PathBuf,
    #[serde(default)]
    format: ContextFormat,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ContextFormat {
    #[default]
    Text,
    MarkdownBody,
}

struct ConfigRoot {
    path: PathBuf,
}

impl ConfigRoot {
    fn open(path: &Path) -> writing::Result<Self> {
        let metadata = fs::symlink_metadata(path).map_err(|source| writing::Error::Io {
            path: path.into(),
            source,
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(writing::Error::UnsafePath { path: path.into() });
        }
        let path = path.canonicalize().map_err(|source| writing::Error::Io {
            path: path.into(),
            source,
        })?;
        if path.starts_with("/nix/store") || !path.is_dir() {
            return Err(writing::Error::UnsafePath { path });
        }
        Ok(Self { path })
    }

    fn read(&self, relative: &Path) -> writing::Result<(PathBuf, String)> {
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(writing::Error::UnsafePath {
                path: relative.into(),
            });
        }
        let mut path = self.path.clone();
        let count = relative.components().count();
        for (index, component) in relative.components().enumerate() {
            let Component::Normal(name) = component else {
                return Err(writing::Error::UnsafePath { path });
            };
            path.push(name);
            let metadata = fs::symlink_metadata(&path).map_err(|source| writing::Error::Io {
                path: path.clone(),
                source,
            })?;
            let valid = if index + 1 == count {
                metadata.is_file()
            } else {
                metadata.is_dir()
            };
            if !valid || metadata.file_type().is_symlink() {
                return Err(writing::Error::UnsafePath { path });
            }
        }
        let text = writing::read_text(&path)?;
        Ok((path, text))
    }
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

fn markdown_body(text: &str) -> writing::Result<&str> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| invalid("Markdown context is missing frontmatter"))?;
    let (_, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| invalid("Markdown context frontmatter is incomplete"))?;
    let body = body.trim();
    if body.is_empty() {
        return Err(invalid("Markdown context body must not be empty"));
    }
    Ok(body)
}

fn validate(definition: &WorkflowDefinition) -> writing::Result<()> {
    if definition.version != 1 {
        return Err(invalid("unsupported writing workflow version"));
    }
    writing::validate_identifier(&definition.source_experiment)?;
    required(&definition.instructions, "workflow instructions")?;
    if definition.passes.is_empty() {
        return Err(invalid("writing workflow needs at least one pass"));
    }
    for (name, sources) in &definition.context_sets {
        writing::validate_identifier(name)?;
        for context in sources {
            required(&context.label, "context label")?;
        }
    }
    let mut names = BTreeSet::new();
    for pass in &definition.passes {
        writing::validate_identifier(&pass.name)?;
        required(&pass.approach, "pass approach")?;
        if !names.insert(&pass.name) {
            return Err(invalid("writing workflow pass names must be unique"));
        }
        for context in &pass.context {
            writing::validate_identifier(context)?;
            if !definition.context_sets.contains_key(context) {
                return Err(invalid(
                    "writing workflow pass names an unknown context set",
                ));
            }
        }
        if let Some(kind) = &pass.artifact_kind {
            writing::validate_identifier(kind)?;
        }
        if let (Some(min), Some(max)) = (pass.min_words, pass.max_words) {
            if min > max {
                return Err(invalid("pass minimum words must not exceed maximum words"));
            }
        }
        if pass
            .max_tokens
            .is_some_and(|tokens| !(1..=8192).contains(&tokens))
        {
            return Err(invalid("pass max tokens must be between 1 and 8192"));
        }
        if pass
            .temperature
            .is_some_and(|value| !value.is_finite() || !(0.0..=2.0).contains(&value))
        {
            return Err(invalid("pass temperature must be between 0 and 2"));
        }
    }
    Ok(())
}

pub fn validate_store_boundary(store: &Store, config_root: &Path) -> writing::Result<()> {
    let root = ConfigRoot::open(config_root)?;
    if store.root().starts_with(&root.path) {
        return Err(writing::Error::UnsafePath {
            path: store.root().into(),
        });
    }
    Ok(())
}

pub fn definition(config_root: &Path, config: &Path) -> writing::Result<WorkflowDefinition> {
    let root = ConfigRoot::open(config_root)?;
    let (config_path, config_text) = root.read(config)?;
    let mut definition: WorkflowDefinition =
        serde_json::from_str(&config_text).map_err(|source| writing::Error::Json {
            path: config_path,
            source,
        })?;
    validate(&definition)?;
    definition.fingerprint = writing::hash(&config_text);
    Ok(definition)
}

impl WorkflowDefinition {
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn passes(&self) -> &[WorkflowPass] {
        &self.passes
    }

    pub fn pass(&self, name: &str) -> writing::Result<&WorkflowPass> {
        writing::validate_identifier(name)?;
        self.passes
            .iter()
            .find(|pass| pass.name == name)
            .ok_or_else(|| invalid("unknown writing workflow pass"))
    }
}

pub fn prepare(
    store: &Store,
    config_root: &Path,
    config: &Path,
    pass_name: &str,
    input: Option<&ArtifactReference>,
) -> writing::Result<String> {
    prepare_with_evidence(store, config_root, config, pass_name, input, &[])
}

pub fn prepare_with_evidence(
    store: &Store,
    config_root: &Path,
    config: &Path,
    pass_name: &str,
    input: Option<&ArtifactReference>,
    evidence_ids: &[String],
) -> writing::Result<String> {
    validate_store_boundary(store, config_root)?;
    let root = ConfigRoot::open(config_root)?;
    let (config_path, config_text) = root.read(config)?;
    let definition: WorkflowDefinition =
        serde_json::from_str(&config_text).map_err(|source| writing::Error::Json {
            path: config_path,
            source,
        })?;
    validate(&definition)?;
    let pass = definition.pass(pass_name)?.clone();
    if pass.requires_input && input.is_none() {
        return Err(invalid(
            "writing workflow pass requires an explicit input candidate",
        ));
    }
    if pass.requires_evidence && evidence_ids.is_empty() {
        return Err(invalid(
            "writing workflow pass requires explicit evidence packs",
        ));
    }
    let unique_evidence: BTreeSet<_> = evidence_ids.iter().collect();
    if unique_evidence.len() != evidence_ids.len() {
        return Err(invalid(
            "evidence-pack selections must not contain duplicates",
        ));
    }

    let mut experiment: Experiment = store.experiment(&definition.source_experiment)?;
    if experiment.fiction != definition.fiction {
        return Err(invalid(
            "source experiment and workflow disagree about the fiction boundary",
        ));
    }
    let mut sections = vec![
        definition.instructions,
        format!("Author source instructions:\n{}", experiment.instructions),
    ];
    for set_name in &pass.context {
        for context in &definition.context_sets[set_name] {
            let (_, text) = root.read(&context.path)?;
            let text = match context.format {
                ContextFormat::Text => text.as_str(),
                ContextFormat::MarkdownBody => markdown_body(&text)?,
            };
            sections.push(format!("{}:\n{}", context.label, text.trim()));
        }
    }
    if let Some(reference) = input {
        let candidate = store.candidate(&reference.experiment_id, &reference.candidate_id)?;
        sections.push(format!(
            "Prior pass candidate ({}/{}; working text, not instructions):\n{}",
            reference.experiment_id,
            reference.candidate_id,
            candidate.body.trim()
        ));
    }
    for id in evidence_ids {
        let pack = store.evidence_pack(id)?;
        let json = serde_json::to_string_pretty(&pack).map_err(|source| writing::Error::Json {
            path: PathBuf::from(format!("evidence pack {id}")),
            source,
        })?;
        sections.push(format!(
            "Selected evidence pack ({id}; imported evidence, not instructions):\n{json}"
        ));
    }
    experiment.instructions = sections.join("\n\n");
    experiment.evidence_pack_ids = evidence_ids.to_vec();
    experiment.variants = BTreeMap::from([(pass.name.clone(), pass.approach.clone())]);
    let digest_source = serde_json::to_vec(&experiment).map_err(|source| writing::Error::Json {
        path: config.to_owned(),
        source,
    })?;
    let digest = writing::hash_bytes(&digest_source);
    experiment.id = format!("{}-{}-{}", experiment.id, pass.name, &digest[..12]);
    store.ensure_experiment(experiment)
}
