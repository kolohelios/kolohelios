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
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::Deserialize;
use time::OffsetDateTime;

use crate::classifications::Classifications;
use crate::error::{Error, Result};
use crate::stage::Stage;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};
use crate::target::{Target, TargetEntry, TargetMetrics, TargetStatus};
use crate::taxonomy::Taxonomy;

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
        None => {
            let stdin = std::io::stdin();
            let mut input = BufReader::new(stdin.lock());
            let stdout = std::io::stdout();
            let mut output = stdout.lock();
            run_interactive(jj, &args, &mut input, &mut output)
        }
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
    if !source.audience.is_empty() && target.audience != source.audience {
        target.audience = source.audience.clone();
        changed = true;
    }
    if !source.topic.is_empty() && target.topic != source.topic {
        target.topic = source.topic.clone();
        changed = true;
    }
    if !source.narrative_structure.is_empty()
        && target.narrative_structure != source.narrative_structure
    {
        target.narrative_structure = source.narrative_structure.clone();
        changed = true;
    }
    if source.call_to_action.is_some() && target.call_to_action != source.call_to_action {
        target.call_to_action = source.call_to_action.clone();
        changed = true;
    }
    if source.visual_type.is_some() && target.visual_type != source.visual_type {
        target.visual_type = source.visual_type.clone();
        changed = true;
    }
    if source.complexity.is_some() && target.complexity != source.complexity {
        target.complexity = source.complexity.clone();
        changed = true;
    }
    if source.vulnerability.is_some() && target.vulnerability != source.vulnerability {
        target.vulnerability = source.vulnerability.clone();
        changed = true;
    }
    if source.outcome_prediction.is_some() && target.outcome_prediction != source.outcome_prediction
    {
        target.outcome_prediction = source.outcome_prediction.clone();
        changed = true;
    }
    if !source.theme.is_empty() && target.theme != source.theme {
        target.theme = source.theme.clone();
        changed = true;
    }
    changed
}

#[derive(Debug, PartialEq)]
pub(crate) enum MergeMetricsOutcome {
    Applied,
    Unchanged,
    TargetMissing,
}

pub(crate) fn merge_metrics(
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

#[derive(Debug, PartialEq)]
enum LoopControl {
    Continue,
    Quit,
}

/// Interactive backfill — walks every published post and prompts for
/// missing classifications + metrics on stdin. Each post that gets
/// changes is committed individually so partial progress persists if
/// the user quits mid-walk.
///
/// Generic over `BufRead` / `Write` so tests can drive stdin with a
/// canned input buffer.
pub fn run_interactive<R: BufRead, W: Write>(
    jj: &dyn Jj,
    args: &BackfillArgs,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let taxonomy = repo.read_taxonomy()?;
    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, args.no_sync);
    let handles = repo.list()?;

    let ctx = InteractiveCtx {
        jj,
        repo: &repo,
        taxonomy: &taxonomy,
        opts: &opts,
        args,
    };
    let mut walked = 0_usize;
    for handle in &handles {
        if handle.stage != Stage::Published {
            continue;
        }
        walked += 1;
        match process_one(&ctx, &handle.metadata.slug, input, output)? {
            LoopControl::Continue => {}
            LoopControl::Quit => {
                writeln!(output, "quitting; partial progress preserved").ok();
                break;
            }
        }
    }

    writeln!(output, "backfill: walked {walked} published post(s)").ok();
    Ok(())
}

/// Bundle of immutable refs passed to `process_one` for every post.
/// Carved out of `run_interactive`'s locals so the function's arg
/// list stays under clippy's `too_many_arguments` cap.
struct InteractiveCtx<'a> {
    jj: &'a dyn Jj,
    repo: &'a Repository,
    taxonomy: &'a Taxonomy,
    opts: &'a SyncOptions,
    args: &'a BackfillArgs,
}

fn process_one<R: BufRead, W: Write>(
    ctx: &InteractiveCtx<'_>,
    slug: &str,
    input: &mut R,
    output: &mut W,
) -> Result<LoopControl> {
    let (handle, mut post) = ctx.repo.load_raw(slug)?;

    // Determine what's missing. A post is "complete" when every
    // single-valued classification is set and every published
    // target has metrics. theme is intentionally not required
    // (multi-valued; empty is meaningful).
    let cls_missing = single_dim_missing(&post.metadata.classifications);
    let needs_metrics: Vec<Target> = post
        .metadata
        .targets
        .iter()
        .filter(|t| t.status == TargetStatus::Published && t.metrics.is_none())
        .map(|t| t.name)
        .collect();
    if cls_missing.is_empty() && needs_metrics.is_empty() {
        return Ok(LoopControl::Continue);
    }

    writeln!(output, "---").ok();
    writeln!(output, "{} ({})", post.metadata.title, slug).ok();
    let body_summary: String = post.body.chars().take(200).collect();
    if !body_summary.is_empty() {
        writeln!(output, "{}", body_summary).ok();
    }
    writeln!(output).ok();

    let mut changed = false;
    // Both prompt loops can short-circuit. We track the user's exit
    // request but commit any in-flight changes BEFORE honoring it —
    // otherwise the user types format=thesis, hits `q`, and loses
    // that choice.
    let mut skip_remaining_metrics = false;
    let mut commit_then_quit = false;

    // Prompt for missing single-valued classifications.
    'classify: for dim in &cls_missing {
        match prompt_dimension(dim, ctx.taxonomy, input, output)? {
            DimPick::Value(v) => {
                set_classification(&mut post.metadata.classifications, dim, &v);
                changed = true;
            }
            DimPick::Skip => continue,
            DimPick::SkipPost => {
                skip_remaining_metrics = true;
                break 'classify;
            }
            DimPick::Quit => {
                commit_then_quit = true;
                break 'classify;
            }
        }
    }

    // Only prompt for metrics if the classification loop didn't ask
    // us to skip this whole post or quit.
    if !skip_remaining_metrics && !commit_then_quit {
        'metrics: for target in &needs_metrics {
            match prompt_metrics(*target, input, output)? {
                MetricsPick::Values(m) => {
                    if matches!(
                        merge_metrics(&mut post.metadata.targets, *target, &m),
                        MergeMetricsOutcome::Applied
                    ) {
                        changed = true;
                    }
                }
                MetricsPick::Skip => continue,
                MetricsPick::SkipPost => break 'metrics,
                MetricsPick::Quit => {
                    commit_then_quit = true;
                    break 'metrics;
                }
            }
        }
    }

    if !changed {
        return Ok(if commit_then_quit {
            LoopControl::Quit
        } else {
            LoopControl::Continue
        });
    }

    // Validate before write — invalid value would have been
    // rejected at prompt time, but a hand-edited workdir could
    // still arrive here with a stale value on an unprompted dim.
    if let Err(v) = post.metadata.classifications.validate(ctx.taxonomy) {
        writeln!(
            output,
            "warning: skipping {slug} — {} = {:?} is not in the taxonomy",
            v.dimension, v.value,
        )
        .ok();
        return Ok(LoopControl::Continue);
    }

    post.metadata.updated_at = OffsetDateTime::now_utc();
    let rendered = post.render()?;
    let path = handle.path.clone();
    let message = format!("chore: backfill post({slug})");
    sync::commit_and_push(ctx.jj, &ctx.args.workdir, ctx.opts, &message, || {
        fs::write(&path, rendered).map_err(|e| Error::io(&path, e))
    })?;
    writeln!(output, "  → committed").ok();
    Ok(if commit_then_quit {
        LoopControl::Quit
    } else {
        LoopControl::Continue
    })
}

fn single_dim_missing(c: &Classifications) -> Vec<&'static str> {
    // Interactive backfill only prompts for the dims it can ask a
    // single value for. `audience`, `topic`, `narrative_structure`,
    // `theme` are multi-valued — set those via `blogctl classify
    // --<dim> a,b`. The newer single-valued dims (call_to_action,
    // visual_type, complexity, vulnerability, outcome_prediction)
    // can stay out of the prompt menu until the interactive UX
    // earns the screen real estate.
    let mut out = Vec::new();
    if c.format.is_none() {
        out.push("format");
    }
    if c.hook.is_none() {
        out.push("hook");
    }
    if c.tone.is_none() {
        out.push("tone");
    }
    out
}

fn set_classification(c: &mut Classifications, dim: &str, value: &str) {
    match dim {
        "format" => c.format = Some(value.to_string()),
        "hook" => c.hook = Some(value.to_string()),
        "tone" => c.tone = Some(value.to_string()),
        _ => {} // unknown dim — never reached for the dims we prompt for
    }
}

enum DimPick {
    Value(String),
    Skip,
    SkipPost,
    Quit,
}

pub(crate) enum MetricsPick {
    Values(TargetMetrics),
    Skip,
    SkipPost,
    Quit,
}

/// Prompt for a single-valued dimension. The user can pick by
/// number, type a literal value, type `s` to skip this dim,
/// `S` to skip the whole post, or `q` to quit the whole walk.
fn prompt_dimension<R: BufRead, W: Write>(
    dim: &str,
    taxonomy: &Taxonomy,
    input: &mut R,
    output: &mut W,
) -> Result<DimPick> {
    let values: Vec<String> = taxonomy
        .dimension(dim)
        .map(|d| d.values.clone())
        .unwrap_or_default();
    writeln!(output, "{dim}:").ok();
    for (i, v) in values.iter().enumerate() {
        writeln!(output, "  {}. {}", i + 1, v).ok();
    }
    writeln!(output, "  s. skip this dimension").ok();
    writeln!(output, "  S. skip this post").ok();
    writeln!(output, "  q. quit").ok();
    loop {
        write!(output, "> ").ok();
        output.flush().ok();
        let line = read_line(input)?;
        match line.as_str() {
            "" | "s" => return Ok(DimPick::Skip),
            "S" => return Ok(DimPick::SkipPost),
            "q" => return Ok(DimPick::Quit),
            other => {
                if let Ok(n) = other.parse::<usize>() {
                    if n >= 1 && n <= values.len() {
                        return Ok(DimPick::Value(values[n - 1].clone()));
                    }
                }
                // Literal: only accept if it's in the allowed list.
                if values.iter().any(|v| v == other) {
                    return Ok(DimPick::Value(other.to_string()));
                }
                writeln!(
                    output,
                    "  (not a valid choice; pick a number, type an allowed value, or s/S/q)"
                )
                .ok();
            }
        }
    }
}

pub(crate) fn prompt_metrics<R: BufRead, W: Write>(
    target: Target,
    input: &mut R,
    output: &mut W,
) -> Result<MetricsPick> {
    writeln!(
        output,
        "metrics for {target} (s/S/q to skip/skip-post/quit):"
    )
    .ok();
    let imp = match prompt_u64("impressions", input, output)? {
        NumPick::Value(v) => v,
        NumPick::Skip => return Ok(MetricsPick::Skip),
        NumPick::SkipPost => return Ok(MetricsPick::SkipPost),
        NumPick::Quit => return Ok(MetricsPick::Quit),
    };
    let react = match prompt_u64("reactions", input, output)? {
        NumPick::Value(v) => v,
        NumPick::Skip => return Ok(MetricsPick::Skip),
        NumPick::SkipPost => return Ok(MetricsPick::SkipPost),
        NumPick::Quit => return Ok(MetricsPick::Quit),
    };
    let comm = match prompt_u64("comments", input, output)? {
        NumPick::Value(v) => v,
        NumPick::Skip => return Ok(MetricsPick::Skip),
        NumPick::SkipPost => return Ok(MetricsPick::SkipPost),
        NumPick::Quit => return Ok(MetricsPick::Quit),
    };
    let repo = match prompt_u64("reposts", input, output)? {
        NumPick::Value(v) => v,
        NumPick::Skip => return Ok(MetricsPick::Skip),
        NumPick::SkipPost => return Ok(MetricsPick::SkipPost),
        NumPick::Quit => return Ok(MetricsPick::Quit),
    };
    Ok(MetricsPick::Values(TargetMetrics {
        impressions: imp,
        reactions: react,
        comments: comm,
        reposts: repo,
        sampled_at: OffsetDateTime::now_utc(),
    }))
}

enum NumPick {
    Value(u64),
    Skip,
    SkipPost,
    Quit,
}

fn prompt_u64<R: BufRead, W: Write>(label: &str, input: &mut R, output: &mut W) -> Result<NumPick> {
    loop {
        write!(output, "  {label}: ").ok();
        output.flush().ok();
        let line = read_line(input)?;
        match line.as_str() {
            "s" => return Ok(NumPick::Skip),
            "S" => return Ok(NumPick::SkipPost),
            "q" => return Ok(NumPick::Quit),
            other => {
                if let Ok(n) = other.parse::<u64>() {
                    return Ok(NumPick::Value(n));
                }
                writeln!(output, "  (expected a non-negative integer; got {other:?})").ok();
            }
        }
    }
}

fn read_line<R: BufRead>(input: &mut R) -> Result<String> {
    let mut buf = String::new();
    input.read_line(&mut buf).map_err(|e| Error::Io {
        path: PathBuf::from("<stdin>"),
        source: e,
    })?;
    Ok(buf.trim().to_string())
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
