//! `blogctl backfill` — fill in missing classifications + metrics
//! across the published backlog.
//!
//! Two modes:
//!
//! - `--import <file.json>` (batch): merge a JSON file of per-slug
//!   entries into the matching posts. One commit covers the whole
//!   batch; unknown slugs / unknown targets / invalid taxonomy
//!   values warn to stderr and produce a non-zero exit but partial
//!   progress is preserved.
//! - (no `--import`, interactive): walks every published post and
//!   prompts for missing dimensions/metrics on stdin. Lands in a
//!   follow-up commit on this branch.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use time::OffsetDateTime;

use crate::classifications::Classifications;
use crate::error::{Error, Result};
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};
use crate::target::{Target, TargetEntry, TargetMetrics};

#[derive(Debug)]
pub struct BackfillArgs {
    pub workdir: PathBuf,
    /// Path to a JSON file of `{ slug → classifications/metrics }`
    /// entries. When set, runs in batch mode (no prompts).
    pub import: Option<PathBuf>,
    pub no_sync: bool,
}

/// One entry in the backfill JSON. Mirrors the schema in #435.
#[derive(Debug, Deserialize)]
struct BackfillEntry {
    slug: String,
    #[serde(default)]
    classifications: Option<Classifications>,
    /// Map from target name (kebab-case string — `"linkedin"`,
    /// `"blog"`) to the metrics to apply. Keys are parsed into
    /// `Target` at merge time so the deserializer doesn't reject
    /// future venues we haven't taught the enum about yet (we just
    /// warn and skip them).
    #[serde(default)]
    metrics: Option<HashMap<String, TargetMetrics>>,
}

pub fn run(jj: &dyn Jj, args: BackfillArgs) -> Result<()> {
    match &args.import {
        Some(path) => run_import(jj, &args, path.clone()),
        None => Err(Error::Unimplemented("backfill interactive mode")),
    }
}

fn run_import(jj: &dyn Jj, args: &BackfillArgs, path: PathBuf) -> Result<()> {
    let raw = fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
    let entries: Vec<BackfillEntry> =
        serde_json::from_str(&raw).map_err(Error::BackfillImportParse)?;

    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let taxonomy = repo.read_taxonomy()?;

    // Process each entry. Track changed post paths + which posts
    // actually need writing; warnings accumulate but don't abort.
    let mut warnings: Vec<String> = Vec::new();
    let mut writes: Vec<(PathBuf, String)> = Vec::new(); // (path, rendered)

    for entry in &entries {
        match apply_entry(&repo, &taxonomy, entry) {
            Ok(None) => {} // no-op — fully idempotent
            Ok(Some(write)) => writes.push(write),
            Err(msg) => warnings.push(msg),
        }
    }

    // Print warnings to stderr; they don't fail the command but do
    // flip the exit code at the end.
    for w in &warnings {
        eprintln!("warning: {w}");
    }

    if writes.is_empty() {
        if warnings.is_empty() {
            println!("backfill: every post already up-to-date");
        }
        return if warnings.is_empty() {
            Ok(())
        } else {
            Err(Error::BackfillPartialFailure(warnings.len()))
        };
    }

    let n = writes.len();
    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, args.no_sync);
    let message = format!("chore: backfill {n} posts");

    sync::commit_and_push(jj, &args.workdir, &opts, &message, || {
        for (path, rendered) in &writes {
            fs::write(path, rendered).map_err(|e| Error::io(path, e))?;
        }
        Ok(())
    })?;
    println!("backfill: updated {n} post(s)");

    if warnings.is_empty() {
        Ok(())
    } else {
        Err(Error::BackfillPartialFailure(warnings.len()))
    }
}

/// Apply one entry. Returns:
/// - `Ok(None)` when the post already has the requested state (no
///   write needed — idempotent).
/// - `Ok(Some((path, rendered)))` when the entry produced changes
///   that should be written to disk.
/// - `Err(msg)` when the entry can't be applied at all — caller
///   accumulates `msg` as a warning.
fn apply_entry(
    repo: &Repository,
    taxonomy: &crate::taxonomy::Taxonomy,
    entry: &BackfillEntry,
) -> std::result::Result<Option<(PathBuf, String)>, String> {
    let (handle, mut post) = match repo.load_raw(&entry.slug) {
        Ok(p) => p,
        Err(_) => return Err(format!("post not found: {}", entry.slug)),
    };
    let mut changed = false;

    if let Some(source) = &entry.classifications {
        if merge_classifications(&mut post.metadata.classifications, source) {
            changed = true;
        }
    }

    if let Some(metrics_map) = &entry.metrics {
        for (target_name, metrics) in metrics_map {
            let target: Target = match target_name.parse() {
                Ok(t) => t,
                Err(_) => {
                    return Err(format!(
                        "unknown target {target_name:?} on post {}",
                        entry.slug
                    ));
                }
            };
            match merge_metrics(&mut post.metadata.targets, target, metrics) {
                MergeMetricsOutcome::Applied => changed = true,
                MergeMetricsOutcome::Unchanged => {}
                MergeMetricsOutcome::TargetMissing => {
                    return Err(format!(
                        "target {target} is not on post {} — add the target first",
                        entry.slug
                    ));
                }
            }
        }
    }

    // Pre-write taxonomy check — fails fast with a helpful error
    // listing the dimension's allowed values.
    if let Err(v) = post.metadata.classifications.validate(taxonomy) {
        return Err(format!(
            "invalid classification on {}: {}={:?} (allowed: {})",
            entry.slug,
            v.dimension,
            v.value,
            v.allowed.join(", ")
        ));
    }

    if !changed {
        return Ok(None);
    }

    post.metadata.updated_at = OffsetDateTime::now_utc();
    let rendered = post.render().map_err(|e| format!("{}: {e}", entry.slug))?;
    Ok(Some((handle.path.clone(), rendered)))
}

/// Merge `source` into `target`. Single-valued dims are overwritten
/// when set in `source`; `theme` is replaced when non-empty.
/// Returns `true` when anything actually changed.
fn merge_classifications(target: &mut Classifications, source: &Classifications) -> bool {
    let mut changed = false;
    if source.format.is_some() && target.format != source.format {
        target.format = source.format.clone();
        changed = true;
    }
    if source.hook.is_some() && target.hook != source.hook {
        target.hook = source.hook.clone();
        changed = true;
    }
    if source.tone.is_some() && target.tone != source.tone {
        target.tone = source.tone.clone();
        changed = true;
    }
    if source.audience.is_some() && target.audience != source.audience {
        target.audience = source.audience.clone();
        changed = true;
    }
    if source.strategic_role.is_some() && target.strategic_role != source.strategic_role {
        target.strategic_role = source.strategic_role.clone();
        changed = true;
    }
    if !source.theme.is_empty() && target.theme != source.theme {
        target.theme = source.theme.clone();
        changed = true;
    }
    changed
}

#[derive(Debug, PartialEq)]
enum MergeMetricsOutcome {
    Applied,
    Unchanged,
    TargetMissing,
}

fn merge_metrics(
    targets: &mut [TargetEntry],
    target: Target,
    source: &TargetMetrics,
) -> MergeMetricsOutcome {
    let Some(entry) = targets.iter_mut().find(|t| t.name == target) else {
        return MergeMetricsOutcome::TargetMissing;
    };
    if entry.metrics.as_ref() == Some(source) {
        return MergeMetricsOutcome::Unchanged;
    }
    entry.metrics = Some(source.clone());
    MergeMetricsOutcome::Applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn sample_metrics() -> TargetMetrics {
        TargetMetrics {
            impressions: 100,
            reactions: 10,
            comments: 2,
            reposts: 1,
            sampled_at: datetime!(2026-05-14 00:00:00 UTC),
        }
    }

    #[test]
    fn merge_classifications_sets_unset_dimensions() {
        let mut t = Classifications::default();
        let s = Classifications {
            format: Some("thesis".into()),
            ..Default::default()
        };
        assert!(merge_classifications(&mut t, &s));
        assert_eq!(t.format.as_deref(), Some("thesis"));
    }

    #[test]
    fn merge_classifications_overrides_existing_values() {
        let mut t = Classifications {
            tone: Some("gentle".into()),
            ..Default::default()
        };
        let s = Classifications {
            tone: Some("sharp".into()),
            ..Default::default()
        };
        assert!(merge_classifications(&mut t, &s));
        assert_eq!(t.tone.as_deref(), Some("sharp"));
    }

    #[test]
    fn merge_classifications_is_idempotent_on_equal_values() {
        let mut t = Classifications {
            format: Some("thesis".into()),
            ..Default::default()
        };
        let s = Classifications {
            format: Some("thesis".into()),
            ..Default::default()
        };
        assert!(!merge_classifications(&mut t, &s));
    }

    #[test]
    fn merge_classifications_skips_none_source_fields() {
        // Source has format=None, hook=Some. Target's format must
        // stay as-is.
        let mut t = Classifications {
            format: Some("thesis".into()),
            ..Default::default()
        };
        let s = Classifications {
            hook: Some("contradiction".into()),
            ..Default::default()
        };
        merge_classifications(&mut t, &s);
        assert_eq!(t.format.as_deref(), Some("thesis"));
        assert_eq!(t.hook.as_deref(), Some("contradiction"));
    }

    #[test]
    fn merge_classifications_replaces_theme_list() {
        let mut t = Classifications {
            theme: vec!["ambiguity".into()],
            ..Default::default()
        };
        let s = Classifications {
            theme: vec!["delivery".into(), "interfaces".into()],
            ..Default::default()
        };
        assert!(merge_classifications(&mut t, &s));
        assert_eq!(t.theme, vec!["delivery", "interfaces"]);
    }

    #[test]
    fn merge_metrics_applies_to_existing_target() {
        let mut targets = vec![TargetEntry {
            name: Target::Linkedin,
            status: crate::target::TargetStatus::Published,
            url: Some("https://example.invalid".into()),
            published_at: Some(datetime!(2026-05-08 14:32:00 UTC)),
            metrics: None,
        }];
        let outcome = merge_metrics(&mut targets, Target::Linkedin, &sample_metrics());
        assert_eq!(outcome, MergeMetricsOutcome::Applied);
        assert_eq!(targets[0].metrics.as_ref(), Some(&sample_metrics()));
    }

    #[test]
    fn merge_metrics_target_missing_signals_caller() {
        let mut targets: Vec<TargetEntry> = vec![];
        let outcome = merge_metrics(&mut targets, Target::Linkedin, &sample_metrics());
        assert_eq!(outcome, MergeMetricsOutcome::TargetMissing);
    }

    #[test]
    fn merge_metrics_idempotent_on_equal_value() {
        let mut targets = vec![TargetEntry {
            name: Target::Linkedin,
            status: crate::target::TargetStatus::Published,
            url: Some("https://example.invalid".into()),
            published_at: Some(datetime!(2026-05-08 14:32:00 UTC)),
            metrics: Some(sample_metrics()),
        }];
        let outcome = merge_metrics(&mut targets, Target::Linkedin, &sample_metrics());
        assert_eq!(outcome, MergeMetricsOutcome::Unchanged);
    }

    #[test]
    fn backfill_entry_round_trips_through_json() {
        let raw = r#"[
            {
                "slug": "the-only-way-out-is-through",
                "classifications": {
                    "format": "thesis",
                    "hook": "contradiction",
                    "theme": ["ambiguity"]
                },
                "metrics": {
                    "linkedin": {
                        "impressions": 1234,
                        "reactions": 45,
                        "comments": 12,
                        "reposts": 3,
                        "sampled_at": "2026-05-14T00:00:00Z"
                    }
                }
            }
        ]"#;
        let entries: Vec<BackfillEntry> = serde_json::from_str(raw).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.slug, "the-only-way-out-is-through");
        let cls = e.classifications.as_ref().unwrap();
        assert_eq!(cls.format.as_deref(), Some("thesis"));
        assert_eq!(cls.theme, vec!["ambiguity"]);
        let m = e.metrics.as_ref().unwrap();
        assert_eq!(m.get("linkedin").unwrap().impressions, 1234);
    }

    #[test]
    fn empty_classifications_and_metrics_parses_as_no_changes() {
        // A skeleton entry with only the slug — used by hand-edited
        // JSON files that want to leave a placeholder.
        let raw = r#"[{ "slug": "post-x" }]"#;
        let entries: Vec<BackfillEntry> = serde_json::from_str(raw).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].classifications.is_none());
        assert!(entries[0].metrics.is_none());
    }
}
