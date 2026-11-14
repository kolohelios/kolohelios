//! Local writing experiments, independent of the post stage pipeline.
//!
//! Candidate publication never updates a draft, promotes a stage, or invokes
//! sync. The caller supplies generation metadata; this module makes no remote
//! requests and does not resolve credentials.

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};
use tempfile::NamedTempFile;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub mod migration;

/// Errors are typed locally so experiments do not expand the post error model.
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("writing file operation failed at {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("invalid writing JSON at {}: {source}", path.display()))]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[snafu(display("invalid writing data: {reason}"))]
    Invalid { reason: String },
    #[snafu(display("unsafe writing path: {}", path.display()))]
    UnsafePath { path: PathBuf },
    #[snafu(display("could not serialize candidate provenance: {source}"))]
    Yaml { source: serde_yaml_ng::Error },
    #[snafu(display("could not format candidate timestamp: {source}"))]
    Timestamp { source: time::error::Format },
}

/// Result for the private writing store.
pub type Result<T> = std::result::Result<T, Error>;

/// A logical reference, not a new hierarchy or a post-stage requirement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WritingUnit {
    Post { slug: String },
    Article { slug: String },
    Chapter { book: String, chapter: String },
}

/// Private, user-editable experiment definition. Seeds are preserved verbatim.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    pub id: String,
    pub target: WritingUnit,
    pub seed: String,
    pub instructions: String,
    pub fiction: bool,
    pub variants: BTreeMap<String, String>,
}

/// A validated prompt snapshot, held across generation so later experiment edits
/// cannot change the provenance of an in-flight request.
#[derive(Debug, Clone, Serialize)]
pub struct PreparedExperiment {
    prompt_format_version: u32,
    experiment: Experiment,
    variant: String,
    prompt: String,
}

impl PreparedExperiment {
    pub fn prompt(&self) -> &str {
        &self.prompt
    }
}

/// Caller-asserted provenance for externally obtained text; never credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationMetadata {
    pub provider: String,
    pub model: String,
    pub ai_provenance: String,
    pub parameters: BTreeMap<String, serde_json::Value>,
}

/// Initial review state is not configurable by the importing caller.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewStatus {
    Unreviewed,
}

/// YAML frontmatter on each immutable candidate.
#[derive(Debug, Serialize, Deserialize)]
pub struct CandidateProvenance {
    pub candidate_id: String,
    pub provider: String,
    pub model: String,
    pub timestamp: String,
    pub experiment_id: String,
    pub variant: String,
    pub prompt_hash: String,
    pub original_seed_hash: String,
    pub hash_algorithm: String,
    pub review_status: ReviewStatus,
    pub fiction: bool,
    pub ai_provenance: String,
    pub parameters: BTreeMap<String, serde_json::Value>,
}

/// Human decisions are append-only sidecars, never candidate mutations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewDecision {
    Accept,
    Reject,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRecord {
    pub review_id: String,
    pub experiment_id: String,
    pub candidate_id: String,
    pub candidate_hash: String,
    pub decision: ReviewDecision,
    pub reviewer: String,
    pub timestamp: String,
    pub notes: String,
}

fn invalid(reason: &str) -> Error {
    Error::Invalid {
        reason: reason.into(),
    }
}

fn nonempty(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(invalid("required text must not be empty"));
    }
    Ok(())
}

/// Identifiers are single short path components, never paths or extensions.
fn artifact_identifier(value: &str) -> Result<()> {
    if let Some((stamp, random)) = value.split_once('-') {
        if stamp.len() == 22
            && stamp.as_bytes()[8] == b'T'
            && stamp.as_bytes()[21] == b'Z'
            && stamp.as_bytes()[..8].iter().all(u8::is_ascii_digit)
            && stamp.as_bytes()[9..21].iter().all(u8::is_ascii_digit)
            && random.len() == 32
            && random.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Ok(());
        }
    }
    identifier(value)
}

fn identifier(value: &str) -> Result<()> {
    if value.len() > 120 || crate::slug::validate(value).is_err() {
        return Err(invalid(
            "IDs and variant names must be kebab-case, at most 120 bytes",
        ));
    }
    Ok(())
}

impl Experiment {
    pub fn validate(&self) -> Result<()> {
        identifier(&self.id)?;
        nonempty(&self.seed)?;
        nonempty(&self.instructions)?;
        match &self.target {
            WritingUnit::Post { slug } | WritingUnit::Article { slug } => identifier(slug)?,
            WritingUnit::Chapter { book, chapter } => {
                nonempty(book)?;
                nonempty(chapter)?;
            }
        }
        if self.variants.is_empty() {
            return Err(invalid("an experiment needs at least one variant"));
        }
        for (name, approach) in &self.variants {
            identifier(name)?;
            nonempty(approach)?;
        }
        Ok(())
    }

    pub fn prepare(&self, variant: &str) -> Result<PreparedExperiment> {
        self.validate()?;
        let approach = self
            .variants
            .get(variant)
            .ok_or_else(|| invalid("unknown variant"))?;
        let target = match &self.target {
            WritingUnit::Post { slug } => format!("Post: {slug}"),
            WritingUnit::Article { slug } => format!("Article: {slug}"),
            WritingUnit::Chapter { book, chapter } => format!("Book: {book}\nChapter: {chapter}"),
        };
        let boundary = if self.fiction {
            "This is a fictional exploration, not autobiography. Do not invent incidents from the author's life."
        } else {
            "Do not invent facts or firsthand experiences. Flag gaps for the author rather than filling them."
        };
        let prompt = format!(
            "{boundary}\n\n{}\n\n{target}\nOriginal seed:\n{}\n\nApproach ({variant}):\n{approach}\n\nCandidate:\n",
            self.instructions, self.seed
        );
        Ok(PreparedExperiment {
            prompt_format_version: 1,
            experiment: self.clone(),
            variant: variant.into(),
            prompt,
        })
    }
}

/// SHA-256 of the exact UTF-8 bytes, with no whitespace normalization.
pub fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Read a regular file, refusing a symlink leaf or non-file (e.g. FIFO).
pub fn read_text(path: &Path) -> Result<String> {
    let meta = fs::symlink_metadata(path).context(IoSnafu { path })?;
    if !meta.is_file() {
        return Err(Error::UnsafePath { path: path.into() });
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .context(IoSnafu { path })?;
    let mut text = String::new();
    file.read_to_string(&mut text).context(IoSnafu { path })?;
    Ok(text)
}

/// Load JSON without shelling out or evaluating templates/code.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_str(&read_text(path)?).context(JsonSnafu { path })
}

/// Writing storage is explicitly rooted in an existing private workdir. It can
/// coexist with a blog workdir or stand alone; no post initialization required.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open(workdir: &Path) -> Result<Self> {
        let root = workdir.canonicalize().context(IoSnafu { path: workdir })?;
        if root.starts_with("/nix/store") || !root.is_dir() {
            return Err(Error::UnsafePath { path: root });
        }
        Ok(Self { root })
    }

    // Check every managed directory, not just the leaf. Symlinked workdir roots
    // are resolved by open; symlinks beneath that root are never followed.
    fn directory(&self, components: &[&str], create: bool) -> Result<Option<PathBuf>> {
        let mut path = self.root.clone();
        for name in std::iter::once(&".writing").chain(components.iter()) {
            path.push(name);
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(Error::UnsafePath { path }),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    if !create {
                        return Ok(None);
                    }
                    if let Err(source) = DirBuilder::new().mode(0o700).create(&path) {
                        if source.kind() != std::io::ErrorKind::AlreadyExists {
                            return Err(Error::Io { path, source });
                        }
                        // Another local writer may have created the directory.
                        // Still refuse a symlink or non-directory at that path.
                        if !fs::symlink_metadata(&path)
                            .context(IoSnafu { path: &path })?
                            .is_dir()
                        {
                            return Err(Error::UnsafePath { path });
                        }
                    }
                }
                Err(source) => return Err(Error::Io { path, source }),
            }
            if create {
                fs::set_permissions(&path, Permissions::from_mode(0o700))
                    .context(IoSnafu { path: &path })?;
            }
        }
        Ok(Some(path))
    }

    pub fn import_experiment(&self, source: &Path) -> Result<String> {
        let exp: Experiment = read_json(source)?;
        exp.validate()?;
        let dir = self
            .directory(&["experiments"], true)?
            .ok_or_else(|| invalid("missing experiment directory"))?;
        let path = dir.join(format!("{}.json", exp.id));
        let bytes = serde_json::to_vec_pretty(&exp).context(JsonSnafu { path: &path })?;
        let mut pending = NamedTempFile::new_in(&dir).context(IoSnafu { path: &dir })?;
        pending.write_all(&bytes).context(IoSnafu { path: &path })?;
        publish(pending, &path, 0o600)?;
        Ok(exp.id)
    }

    pub fn experiment(&self, id: &str) -> Result<Experiment> {
        identifier(id)?;
        let dir = self
            .directory(&["experiments"], false)?
            .ok_or_else(|| invalid("experiment not found"))?;
        let exp: Experiment = read_json(&dir.join(format!("{id}.json")))?;
        exp.validate()?;
        if exp.id != id {
            return Err(invalid("experiment ID disagrees with its filename"));
        }
        Ok(exp)
    }

    pub fn experiments(&self) -> Result<Vec<String>> {
        list(self.directory(&["experiments"], false)?, "json")
    }

    pub fn candidates(&self, experiment: &str) -> Result<Vec<String>> {
        identifier(experiment)?;
        list(self.directory(&["candidates", experiment], false)?, "md")
    }

    pub fn read_candidate(&self, experiment: &str, id: &str) -> Result<String> {
        identifier(experiment)?;
        artifact_identifier(id)?;
        let dir = self
            .directory(&["candidates", experiment], false)?
            .ok_or_else(|| invalid("candidate not found"))?;
        read_text(&dir.join(format!("{id}.md")))
    }

    /// Append a decision bound to the exact artifact read by the caller. A
    /// concurrent edit invalidates the decision rather than approving new bytes.
    pub fn record_review(
        &self,
        experiment: &str,
        candidate: &str,
        expected_hash: &str,
        decision: ReviewDecision,
        reviewer: String,
        notes: String,
    ) -> Result<String> {
        nonempty(&reviewer)?;
        if reviewer.chars().any(char::is_control) {
            return Err(invalid("reviewer must not contain control characters"));
        }
        if hash(&self.read_candidate(experiment, candidate)?) != expected_hash {
            return Err(invalid("candidate changed since review began"));
        }
        let dir = self
            .directory(&["reviews", experiment, candidate], true)?
            .ok_or_else(|| invalid("missing review directory"))?;
        let mut pending = tempfile::Builder::new()
            .prefix(".pending-")
            .rand_bytes(20)
            .tempfile_in(&dir)
            .context(IoSnafu { path: &dir })?;
        let id = artifact_id(&pending)?;
        let record = ReviewRecord {
            review_id: id.clone(),
            experiment_id: experiment.into(),
            candidate_id: candidate.into(),
            candidate_hash: expected_hash.into(),
            decision,
            reviewer,
            timestamp: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .context(TimestampSnafu)?,
            notes,
        };
        let path = dir.join(format!("{id}.json"));
        let bytes = serde_json::to_vec_pretty(&record).context(JsonSnafu { path: &path })?;
        pending.write_all(&bytes).context(IoSnafu { path: &path })?;
        publish(pending, &path, 0o400)?;
        Ok(id)
    }

    pub fn reviews(&self, experiment: &str, candidate: &str) -> Result<Vec<ReviewRecord>> {
        let current_hash = hash(&self.read_candidate(experiment, candidate)?);
        let dir = self.directory(&["reviews", experiment, candidate], false)?;
        let ids = list(dir.clone(), "json")?;
        let mut records = Vec::new();
        if let Some(dir) = dir {
            for id in ids {
                let record: ReviewRecord = read_json(&dir.join(format!("{id}.json")))?;
                if record.review_id != id
                    || record.experiment_id != experiment
                    || record.candidate_id != candidate
                    || record.candidate_hash != current_hash
                {
                    return Err(invalid(
                        "review does not match this candidate's current bytes or identity",
                    ));
                }
                records.push(record);
            }
        }
        Ok(records)
    }

    /// Probe the managed directory and a private temporary file before a paid
    /// request. A later write can still fail; no candidate is published here.
    pub fn check_candidate_storage(&self, prepared: &PreparedExperiment) -> Result<()> {
        let dir = self
            .directory(&["candidates", &prepared.experiment.id], true)?
            .ok_or_else(|| invalid("missing candidate directory"))?;
        let probe = NamedTempFile::new_in(&dir).context(IoSnafu { path: &dir })?;
        probe.as_file().sync_all().context(IoSnafu { path: &dir })?;
        Ok(())
    }

    /// Store one candidate from an already-prepared snapshot. No network, stage
    /// transition, review decision, draft write, or implicit VCS operation.
    pub fn record_candidate(
        &self,
        prepared: &PreparedExperiment,
        generation: &GenerationMetadata,
        body: &str,
    ) -> Result<String> {
        nonempty(body)?;
        for value in [
            &generation.provider,
            &generation.model,
            &generation.ai_provenance,
        ] {
            nonempty(value)?;
        }
        let dir = self
            .directory(&["candidates", &prepared.experiment.id], true)?
            .ok_or_else(|| invalid("missing candidate directory"))?;
        // tempfile supplies randomness; the timestamp makes IDs easy to order.
        // The pending file is hidden and never listed as a complete candidate.
        let mut pending = tempfile::Builder::new()
            .prefix(".pending-")
            .rand_bytes(20)
            .tempfile_in(&dir)
            .context(IoSnafu { path: &dir })?;
        let now = OffsetDateTime::now_utc();
        let id = artifact_id(&pending)?;
        let meta = CandidateProvenance {
            candidate_id: id.clone(),
            provider: generation.provider.clone(),
            model: generation.model.clone(),
            timestamp: now.format(&Rfc3339).context(TimestampSnafu)?,
            experiment_id: prepared.experiment.id.clone(),
            variant: prepared.variant.clone(),
            prompt_hash: hash(&prepared.prompt),
            original_seed_hash: hash(&prepared.experiment.seed),
            hash_algorithm: "sha256".into(),
            review_status: ReviewStatus::Unreviewed,
            fiction: prepared.experiment.fiction,
            ai_provenance: generation.ai_provenance.clone(),
            parameters: generation.parameters.clone(),
        };
        let yaml = serde_yaml_ng::to_string(&meta).context(YamlSnafu)?;
        let snapshot = serde_json::to_string_pretty(prepared).context(JsonSnafu { path: &dir })?;
        // Longer fences than any run in the snapshot prevent user text from
        // escaping its JSON block when the artifact is rendered as Markdown.
        let fence = "`".repeat(
            snapshot
                .split(|c| c != '`')
                .map(str::len)
                .max()
                .unwrap_or(0)
                .max(2)
                + 1,
        );
        let text = format!("---\n{yaml}---\n\n{body}\n\n## Request snapshot (not manuscript)\n\n{fence}json\n{snapshot}\n{fence}\n");
        let path = dir.join(format!("{id}.md"));
        pending
            .write_all(text.as_bytes())
            .context(IoSnafu { path: &path })?;
        publish(pending, &path, 0o400)?;
        Ok(id)
    }
}

fn artifact_id(pending: &NamedTempFile) -> Result<String> {
    let suffix = pending
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix(".pending-"))
        .ok_or_else(|| invalid("invalid temporary filename"))?
        .to_ascii_lowercase();
    let id = format!(
        "{}-{suffix}",
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    );
    identifier(&id)?;
    Ok(id)
}

fn list(directory: Option<PathBuf>, extension: &str) -> Result<Vec<String>> {
    let Some(dir) = directory else {
        return Ok(Vec::new());
    };
    let mut ids = Vec::new();
    for entry in fs::read_dir(&dir).context(IoSnafu { path: &dir })? {
        let path = entry.context(IoSnafu { path: &dir })?.path();
        if path.extension().and_then(|s| s.to_str()) != Some(extension) {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| invalid("non-UTF-8 artifact filename"))?;
        if extension == "md" {
            artifact_identifier(id)?;
        } else {
            identifier(id)?;
        }
        if !fs::symlink_metadata(&path)
            .context(IoSnafu { path: &path })?
            .is_file()
        {
            return Err(Error::UnsafePath { path });
        }
        ids.push(id.into());
    }
    ids.sort();
    Ok(ids)
}

/// Hard-link publication refuses existing destinations (including symlinks).
/// Contents and mode are finalized before the public name becomes visible.
fn publish(pending: NamedTempFile, path: &Path, mode: u32) -> Result<()> {
    pending
        .as_file()
        .set_permissions(Permissions::from_mode(mode))
        .context(IoSnafu { path })?;
    pending.as_file().sync_all().context(IoSnafu { path })?;
    pending.persist_noclobber(path).map_err(|err| Error::Io {
        path: path.into(),
        source: err.error,
    })?;
    if let Some(parent) = path.parent() {
        File::open(parent)
            .and_then(|dir| dir.sync_all())
            .context(IoSnafu { path: parent })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_publish_refuses_existing_file_and_cleans_pending() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("candidate.md");
        fs::write(&path, "original").unwrap();
        let mut pending = NamedTempFile::new_in(dir.path()).unwrap();
        let pending_path = pending.path().to_owned();
        pending.write_all(b"replacement").unwrap();
        assert!(publish(pending, &path, 0o400).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
        assert!(!pending_path.exists());
    }
}
