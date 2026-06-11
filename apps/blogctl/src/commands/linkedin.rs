//! `blogctl linkedin import` — refresh post metrics from LinkedIn's
//! daily `Content_*.xlsx` analytics exports.
//!
//! Parses every export in `--xlsx-dir`, matches each post by the
//! activity id embedded in its `targets[].url`, and records a per-day
//! `(impressions, engagements)` sample on the matching LinkedIn target.
//! Idempotent on `(urn, snapshot_date)`: re-running overlapping exports
//! adds no duplicate data points. URNs with no matching post are
//! reported, not created — post creation is #860's job.
//!
//! This is the metrics half of the `linkedin import` surface (#707); the
//! HTML-fetch / published-stub step lands in #860.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::linkedin::{self, activity_id, PostSnapshot};
use crate::storage::{PostHandle, Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};
use crate::target::{MetricSample, Target};

/// Export directory relative to the workdir when `--xlsx-dir` is unset.
const DEFAULT_XLSX_DIR: &str = "linkedin-exports";

#[derive(Debug)]
pub struct ImportArgs {
    pub workdir: PathBuf,
    /// Directory of `Content_*.xlsx` exports. Defaults to
    /// `linkedin-exports/` under the workdir.
    pub xlsx_dir: Option<PathBuf>,
    pub dry_run: bool,
    pub no_sync: bool,
}

/// What an import run did: how many daily samples were newly recorded,
/// how many were already present (idempotent skips), and which export
/// URNs matched no post in the workdir.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub added: usize,
    pub skipped: usize,
    pub unmatched: Vec<String>,
}

pub fn run(jj: &dyn Jj, args: ImportArgs) -> Result<ImportSummary> {
    let xlsx_dir = args
        .xlsx_dir
        .unwrap_or_else(|| args.workdir.join(DEFAULT_XLSX_DIR));
    let snapshots = linkedin::parse_dir(&xlsx_dir)?;

    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let handles = repo.list()?;
    let (matched, unmatched) = match_snapshots(&handles, &snapshots);

    let mut added = 0usize;
    let mut skipped = 0usize;
    let mut writes: Vec<(PathBuf, String)> = Vec::new();
    for (slug, samples) in &matched {
        let (handle, mut post) = repo.load_raw(slug)?;
        let Some(idx) = post
            .metadata
            .targets
            .iter()
            .position(|t| t.name == Target::Linkedin)
        else {
            continue;
        };
        let mut changed = false;
        for sample in samples {
            if post.metadata.targets[idx].record_sample(sample.clone()) {
                added += 1;
                changed = true;
            } else {
                skipped += 1;
            }
        }
        if changed && !args.dry_run {
            post.metadata.updated_at = OffsetDateTime::now_utc();
            let rendered = post.render()?;
            writes.push((handle.path.clone(), rendered));
        }
    }

    let summary = ImportSummary {
        added,
        skipped,
        unmatched: unmatched.into_iter().collect(),
    };
    print_summary(&summary, args.dry_run);

    if !args.dry_run && !writes.is_empty() {
        let config = repo.read_config()?;
        let opts = SyncOptions::from_config(&config.sync, args.no_sync);
        let post_count = writes.len();
        let message =
            format!("chore: linkedin metrics — {added} sample(s) across {post_count} post(s)");
        sync::commit_and_push(jj, &args.workdir, &opts, &message, || {
            for (path, rendered) in &writes {
                fs::write(path, rendered).map_err(|e| Error::io(path, e))?;
            }
            Ok(())
        })?;
    }

    Ok(summary)
}

/// Match parsed snapshots to posts by activity id. Returns the daily
/// samples to record per matched post slug, plus the export URNs that
/// matched no post. Pure: no IO, so the matching logic is unit-tested
/// without a workdir on disk.
fn match_snapshots(
    handles: &[PostHandle],
    snapshots: &[PostSnapshot],
) -> (BTreeMap<String, Vec<MetricSample>>, BTreeSet<String>) {
    // Index every LinkedIn target URL by its activity id → post slug.
    let mut id_to_slug: HashMap<String, String> = HashMap::new();
    for handle in handles {
        for target in &handle.metadata.targets {
            if target.name != Target::Linkedin {
                continue;
            }
            if let Some(id) = target.url.as_deref().and_then(activity_id) {
                id_to_slug
                    .entry(id.to_string())
                    .or_insert_with(|| handle.metadata.slug.clone());
            }
        }
    }

    let mut matched: BTreeMap<String, Vec<MetricSample>> = BTreeMap::new();
    let mut unmatched: BTreeSet<String> = BTreeSet::new();
    for snap in snapshots {
        match activity_id(&snap.urn).and_then(|id| id_to_slug.get(id)) {
            Some(slug) => matched.entry(slug.clone()).or_default().push(MetricSample {
                date: snap.snapshot_date,
                impressions: snap.impressions,
                engagements: snap.engagements,
            }),
            None => {
                unmatched.insert(snap.urn.clone());
            }
        }
    }
    (matched, unmatched)
}

fn print_summary(summary: &ImportSummary, dry_run: bool) {
    let tag = if dry_run { " [dry-run]" } else { "" };
    println!(
        "linkedin import: {} sample(s) added, {} already present, {} unmatched URN(s){tag}",
        summary.added,
        summary.skipped,
        summary.unmatched.len(),
    );
    if !summary.unmatched.is_empty() {
        println!("  unmatched URNs (no post in the workdir):");
        for urn in &summary.unmatched {
            println!("    {urn}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::Kind;
    use crate::post::PostMetadata;
    use crate::stage::Stage;
    use crate::target::{TargetEntry, TargetStatus};
    use time::macros::{date, datetime};

    fn linkedin_post(slug: &str, url: &str) -> PostHandle {
        PostHandle {
            stage: Stage::Published,
            path: format!("/x/{slug}.md").into(),
            metadata: PostMetadata {
                title: slug.into(),
                slug: slug.into(),
                kind: Kind::Post,
                theme: "standard".into(),
                status: Stage::Published,
                created_at: datetime!(2026-05-01 00:00:00 UTC),
                updated_at: datetime!(2026-05-01 00:00:00 UTC),
                tags: vec![],
                todoist_task_id: None,
                history_checked: false,
                targets: vec![TargetEntry {
                    name: Target::Linkedin,
                    status: TargetStatus::Published,
                    url: Some(url.into()),
                    published_at: None,
                    metrics: None,
                    samples: Vec::new(),
                }],
                classifications: Default::default(),
                ai: None,
            },
        }
    }

    fn snap(id: &str, day: time::Date, imp: Option<u64>, eng: Option<u64>) -> PostSnapshot {
        PostSnapshot {
            urn: format!("urn:li:activity:{id}"),
            url: format!("https://www.linkedin.com/feed/update/urn:li:activity:{id}"),
            publish_date: date!(2026 - 01 - 01),
            impressions: imp,
            engagements: eng,
            snapshot_date: day,
        }
    }

    #[test]
    fn matches_export_urn_to_share_url_by_activity_id() {
        // The post stores the `/posts/...-<id>-<code>` share URL; the
        // export carries `urn:li:activity:<id>`. Both must resolve to
        // the same id.
        let handles = vec![linkedin_post(
            "beaver",
            "https://www.linkedin.com/posts/x_the-beaver-share-7456442827909005312-8FKm/?utm=1",
        )];
        let snaps = vec![snap(
            "7456442827909005312",
            date!(2026 - 05 - 30),
            Some(74),
            Some(3),
        )];
        let (matched, unmatched) = match_snapshots(&handles, &snaps);
        assert!(unmatched.is_empty());
        assert_eq!(matched.get("beaver").map(Vec::len), Some(1));
        assert_eq!(matched["beaver"][0].impressions, Some(74));
        assert_eq!(matched["beaver"][0].engagements, Some(3));
    }

    #[test]
    fn reports_unmatched_urns() {
        let handles = vec![linkedin_post(
            "known",
            "https://www.linkedin.com/posts/x-7400000000000000001-Aa/",
        )];
        let snaps = vec![
            snap("7400000000000000001", date!(2026 - 05 - 01), Some(1), None),
            snap("7400000000000000999", date!(2026 - 05 - 01), Some(2), None),
        ];
        let (matched, unmatched) = match_snapshots(&handles, &snaps);
        assert!(matched.contains_key("known"));
        assert_eq!(
            unmatched.into_iter().collect::<Vec<_>>(),
            vec!["urn:li:activity:7400000000000000999".to_string()],
        );
    }
}
