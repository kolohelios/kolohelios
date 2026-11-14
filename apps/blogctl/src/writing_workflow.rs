//! Deterministic preparation of consumer-configured writing passes.
//!
//! This module reads explicit local configuration and context files, then
//! prepares an immutable writing experiment. It never resolves credentials,
//! calls a provider, selects a candidate, or advances a workflow.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

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
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowPass {
    name: String,
    requires_input: bool,
    approach: String,
    #[serde(default)]
    context: Vec<String>,
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
    }
    Ok(())
}

pub fn prepare(
    store: &Store,
    config_root: &Path,
    config: &Path,
    pass_name: &str,
    input: Option<&ArtifactReference>,
) -> writing::Result<String> {
    let root = ConfigRoot::open(config_root)?;
    if store.root().starts_with(&root.path) {
        return Err(writing::Error::UnsafePath {
            path: store.root().into(),
        });
    }
    let (config_path, config_text) = root.read(config)?;
    let definition: WorkflowDefinition =
        serde_json::from_str(&config_text).map_err(|source| writing::Error::Json {
            path: config_path,
            source,
        })?;
    validate(&definition)?;
    writing::validate_identifier(pass_name)?;
    let pass = definition
        .passes
        .iter()
        .find(|pass| pass.name == pass_name)
        .ok_or_else(|| invalid("unknown writing workflow pass"))?;
    if pass.requires_input && input.is_none() {
        return Err(invalid(
            "writing workflow pass requires an explicit input candidate",
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
    experiment.instructions = sections.join("\n\n");
    experiment.variants = BTreeMap::from([(pass.name.clone(), pass.approach.clone())]);
    let digest_source = serde_json::to_vec(&experiment).map_err(|source| writing::Error::Json {
        path: config.to_owned(),
        source,
    })?;
    let digest = writing::hash_bytes(&digest_source);
    experiment.id = format!("{}-{}-{}", experiment.id, pass.name, &digest[..12]);
    store.ensure_experiment(experiment)
}
