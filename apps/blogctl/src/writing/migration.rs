//! Explicit, copy-only migration of the provisional Python lab's private files.
//! Original sources, run bytes, names, hashes, and review state are preserved.

use super::*;

#[derive(Deserialize)]
struct LegacyExperiment {
    id: String,
    book: String,
    chapter: String,
    seed: String,
}

#[derive(Debug, Serialize)]
pub struct MigrationReport {
    pub experiment_id: String,
    pub copied_runs: usize,
    pub existing_runs: usize,
}

/// The prototype's fixed creative directions, not new general-engine defaults.
fn convert(old: &LegacyExperiment) -> Experiment {
    Experiment {
        id: old.id.clone(),
        target: WritingUnit::Chapter { book: old.book.clone(), chapter: old.chapter.clone() },
        seed: old.seed.clone(),
        fiction: true,
        instructions: "Write a fictional exploration, not autobiography. Never invent incidents from the author's professional life. Be warm, thoughtful, slightly wry, allegorical where useful, and intellectually honest. Avoid corporate language, generic AI prose, and a tidy moral. Reveal the tension through concrete choices rather than explaining it. Supply one provisional narrative opening, not a finished chapter.".into(),
        variants: BTreeMap::from([
            ("workshop".into(), "A craftsperson, an apprentice, and a remarkably capable machine.".into()),
            ("woodland".into(), "A whimsical animal parable revealing the cost of acquiring understanding.".into()),
            ("ordinary".into(), "An accessible, grounded ordinary-life narrative without a fantasy setting.".into()),
        ]),
    }
}

#[derive(Deserialize)]
struct LegacyProvenance {
    experiment_id: String,
    prompt_hash: String,
    original_seed_hash: String,
    review_status: String,
    provider: String,
    model: String,
    timestamp: String,
}

fn validate_run(text: &str, old: &LegacyExperiment) -> Result<()> {
    let after_open = text
        .strip_prefix("---\n")
        .ok_or_else(|| invalid("legacy run has no frontmatter"))?;
    let (yaml, rest) = after_open
        .split_once("\n---\n")
        .ok_or_else(|| invalid("legacy run has incomplete frontmatter"))?;
    let meta: LegacyProvenance = serde_yaml_ng::from_str(yaml).context(YamlSnafu)?;
    let snapshot = rest
        .rsplit_once("\n## Request snapshot (not manuscript)\n\n```json\n")
        .and_then(|(_, snapshot)| snapshot.strip_suffix("\n```\n"))
        .ok_or_else(|| invalid("legacy run has no complete request snapshot"))?;
    let snapshot: serde_json::Value = serde_json::from_str(snapshot)
        .map_err(|_| invalid("legacy request snapshot is invalid"))?;
    let prompt = snapshot["request"]["prompt"]
        .as_str()
        .ok_or_else(|| invalid("legacy prompt missing"))?;
    if meta.experiment_id != old.id
        || meta.original_seed_hash != hash(&old.seed)
        || meta.prompt_hash != hash(prompt)
        || meta.review_status != "unreviewed"
        || snapshot["experiment"]["seed"].as_str() != Some(old.seed.as_str())
        || snapshot["experiment"]["id"].as_str() != Some(old.id.as_str())
    {
        return Err(invalid(
            "legacy run identity, hashes, or review state do not match the source experiment",
        ));
    }
    nonempty(&meta.provider)?;
    nonempty(&meta.model)?;
    OffsetDateTime::parse(&meta.timestamp, &Rfc3339)
        .map_err(|_| invalid("invalid legacy timestamp"))?;
    Ok(())
}

/// Same-byte destinations are idempotent; any differing destination aborts
/// before file publication. A concurrent writer can still cause a safe failure;
/// a rerun resumes without clobbering any file. Sources are never removed.
pub fn migrate(store: &Store, experiment_file: &Path, runs_dir: &Path) -> Result<MigrationReport> {
    let source = read_text(experiment_file)?;
    let old: LegacyExperiment = serde_json::from_str(&source).context(JsonSnafu {
        path: experiment_file,
    })?;
    let experiment = convert(&old);
    experiment.validate()?;
    if !fs::symlink_metadata(runs_dir)
        .context(IoSnafu { path: runs_dir })?
        .is_dir()
    {
        return Err(Error::UnsafePath {
            path: runs_dir.into(),
        });
    }
    let mut runs = Vec::new();
    for entry in fs::read_dir(runs_dir).context(IoSnafu { path: runs_dir })? {
        let path = entry.context(IoSnafu { path: runs_dir })?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| invalid("invalid legacy filename"))?;
        artifact_identifier(stem)?;
        let text = read_text(&path)?;
        validate_run(&text, &old)?;
        runs.push((format!("{stem}.md"), text));
    }
    let definitions = store
        .directory(&["experiments"], true)?
        .ok_or_else(|| invalid("missing experiments directory"))?;
    let archive = store
        .directory(&["legacy", "novelai-lab"], true)?
        .ok_or_else(|| invalid("missing archive directory"))?;
    let candidates = store
        .directory(&["candidates", &old.id], true)?
        .ok_or_else(|| invalid("missing candidates directory"))?;
    let converted = serde_json::to_string_pretty(&experiment).context(JsonSnafu {
        path: experiment_file,
    })?;
    let mut plan = vec![
        (
            definitions.join(format!("{}.json", old.id)),
            converted,
            0o600,
        ),
        (archive.join("experiment.json"), source, 0o400),
    ];
    for (name, text) in &runs {
        plan.push((candidates.join(name), text.clone(), 0o400));
    }
    let mut pending = Vec::new();
    let mut copied_runs = 0;
    for (path, text, mode) in plan {
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                if read_text(&path)? != text {
                    return Err(invalid(
                        "migration destination already exists with different bytes",
                    ));
                }
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    copied_runs += 1;
                }
                pending.push((path, text, mode));
            }
            Err(source) => return Err(Error::Io { path, source }),
        }
    }
    for (path, text, mode) in pending {
        let dir = path
            .parent()
            .ok_or_else(|| invalid("invalid migration destination"))?;
        let mut file = NamedTempFile::new_in(dir).context(IoSnafu { path: dir })?;
        file.write_all(text.as_bytes())
            .context(IoSnafu { path: &path })?;
        publish(file, &path, mode)?;
    }
    Ok(MigrationReport {
        experiment_id: old.id,
        copied_runs,
        existing_runs: runs.len() - copied_runs,
    })
}
