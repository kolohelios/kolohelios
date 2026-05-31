//! `blogctl metrics update <slug>` / `blogctl metrics show <slug>` /
//! `blogctl metrics collect` — per-target performance numbers.
//!
//! Update overwrites; there's no history. `sampled_at` defaults to
//! `now_utc()` and acts as the freshness signal — `collect` uses it
//! to decide which posts in the eligibility set are due for a
//! refresh, oldest first.

use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::commands::backfill::{merge_metrics, prompt_metrics, MergeMetricsOutcome, MetricsPick};
use crate::datetime::{self, Flag};
use crate::error::{Error, Result};
use crate::stage::Stage;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};
use crate::target::{Target, TargetMetrics, TargetStatus};

#[derive(Debug)]
pub struct UpdateArgs {
    pub slug: String,
    pub workdir: PathBuf,
    pub target: Target,
    pub impressions: u64,
    pub reactions: u64,
    pub comments: u64,
    pub reposts: u64,
    /// RFC 3339 timestamp; parsed by the command body. None means
    /// "use now()".
    pub sampled_at: Option<String>,
    pub no_sync: bool,
}

#[derive(Debug)]
pub struct ShowArgs {
    pub slug: String,
    pub workdir: PathBuf,
}

pub fn update(jj: &dyn Jj, args: UpdateArgs) -> Result<()> {
    let sampled_at = parse_sampled_at(args.sampled_at.as_deref())?;
    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let (handle, mut post) = repo.load_raw(&args.slug)?;

    // Find the named target. Absent → hard fail before any write or
    // sync; the user must add the target via the existing targets[]
    // editing flow first (out of scope here per #436).
    let target_idx = post
        .metadata
        .targets
        .iter()
        .position(|t| t.name == args.target)
        .ok_or_else(|| Error::TargetNotInPost {
            slug: args.slug.clone(),
            target: args.target,
        })?;

    let new_metrics = TargetMetrics {
        impressions: args.impressions,
        reactions: args.reactions,
        comments: args.comments,
        reposts: args.reposts,
        sampled_at,
    };
    let interactions = new_metrics.reactions + new_metrics.comments + new_metrics.reposts;
    post.metadata.targets[target_idx].metrics = Some(new_metrics);

    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, args.no_sync);
    let message = format!(
        "post({}): metrics {} imp={} int={}",
        args.slug, args.target, args.impressions, interactions,
    );
    let path = handle.path.clone();
    let target_name = args.target;
    let impressions = args.impressions;

    sync::commit_and_push(jj, &args.workdir, &opts, &message, || {
        post.metadata.updated_at = OffsetDateTime::now_utc();
        let rendered = post.render()?;
        fs::write(&path, rendered).map_err(|e| Error::io(&path, e))?;
        Ok(())
    })?;
    println!(
        "{}: metrics {target_name} set (imp={impressions} int={interactions})",
        args.slug,
    );
    Ok(())
}

pub fn show(args: ShowArgs) -> Result<()> {
    let repo = Repository::open(Workdir::new(&args.workdir))?;
    // load_raw, not load — show should work on posts with
    // currently-invalid classifications too. Showing metrics
    // shouldn't be blocked by an unrelated taxonomy typo.
    let (_handle, post) = repo.load_raw(&args.slug)?;

    if post.metadata.targets.is_empty() {
        println!("{}: no targets", args.slug);
        return Ok(());
    }
    println!("{} ({}):", args.slug, post.metadata.title);
    for t in &post.metadata.targets {
        match &t.metrics {
            None => println!("  {} [{}]: no metrics", t.name, t.status),
            Some(m) => {
                let sampled = m.sampled_at.format(&Rfc3339).unwrap_or_else(|_| "?".into());
                println!(
                    "  {} [{}]: imp={} react={} comm={} reposts={} (sampled {sampled})",
                    t.name, t.status, m.impressions, m.reactions, m.comments, m.reposts,
                );
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct CollectArgs {
    pub workdir: PathBuf,
    pub target: Target,
    /// Posts whose `sampled_at` is older than this become eligible.
    /// Parsed via `humantime` (`7d`, `1w`, `24h`, …).
    pub stale_after: String,
    /// Hard cap on how many posts a single run walks. The rotation
    /// falls out of "oldest first" — capping at 10/day rotates through
    /// the backlog without forcing the whole thing in one sitting.
    pub batch_size: usize,
    pub no_sync: bool,
}

/// Walk a daily batch of posts whose chosen-target metrics are stale
/// or missing. Prompts interactively, accumulates writes, commits the
/// whole batch in one go. Quit mid-walk preserves accumulated work.
pub fn collect<R: BufRead, W: Write>(
    jj: &dyn Jj,
    args: CollectArgs,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    let stale_after = humantime::parse_duration(&args.stale_after).map_err(|source| {
        Error::InvalidStaleAfter {
            value: args.stale_after.clone(),
            source,
        }
    })?;
    let cutoff = OffsetDateTime::now_utc() - stale_after;

    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, args.no_sync);
    let handles = repo.list()?;

    // Build the eligible set: published posts whose chosen target is
    // itself in `published` state, and whose metrics are either
    // absent or older than the cutoff. `None` sorts before any
    // `Some` so never-sampled posts naturally lead the rotation.
    let mut eligible: Vec<(String, Option<OffsetDateTime>)> = Vec::new();
    for handle in &handles {
        if handle.stage != Stage::Published {
            continue;
        }
        let Some(entry) = handle
            .metadata
            .targets
            .iter()
            .find(|t| t.name == args.target)
        else {
            continue;
        };
        if entry.status != TargetStatus::Published {
            continue;
        }
        let sampled = entry.metrics.as_ref().map(|m| m.sampled_at);
        let stale = match sampled {
            None => true,
            Some(s) => s < cutoff,
        };
        if stale {
            eligible.push((handle.metadata.slug.clone(), sampled));
        }
    }
    eligible.sort_by_key(|(_, sampled)| *sampled);

    if eligible.is_empty() {
        writeln!(output, "metrics collect: nothing to refresh").ok();
        return Ok(());
    }

    let pool = eligible.len();
    let batch_slugs: Vec<String> = eligible
        .into_iter()
        .take(args.batch_size)
        .map(|(slug, _)| slug)
        .collect();
    writeln!(
        output,
        "metrics collect({}): walking {} of {} eligible post(s)",
        args.target,
        batch_slugs.len(),
        pool,
    )
    .ok();

    let mut writes: Vec<(PathBuf, String)> = Vec::new();
    let mut quit_early = false;

    for slug in &batch_slugs {
        let (h, mut post) = repo.load_raw(slug)?;
        print_preview(output, &post, slug, args.target);

        match prompt_metrics(args.target, input, output)? {
            MetricsPick::Values(m) => {
                if matches!(
                    merge_metrics(&mut post.metadata.targets, args.target, &m),
                    MergeMetricsOutcome::Applied
                ) {
                    post.metadata.updated_at = OffsetDateTime::now_utc();
                    let rendered = post.render()?;
                    writes.push((h.path.clone(), rendered));
                }
            }
            // `Skip` and `SkipPost` are equivalent here — we only
            // prompt one target per post — but accept both so the
            // shared prompt UX stays consistent with `backfill`.
            MetricsPick::Skip | MetricsPick::SkipPost => continue,
            MetricsPick::Quit => {
                quit_early = true;
                break;
            }
        }
    }

    let n = writes.len();
    if n == 0 {
        writeln!(output, "metrics collect: no changes recorded").ok();
        return Ok(());
    }

    let message = format!("chore: collect metrics({}) — {n} post(s)", args.target);
    sync::commit_and_push(jj, &args.workdir, &opts, &message, || {
        for (path, rendered) in &writes {
            fs::write(path, rendered).map_err(|e| Error::io(path, e))?;
        }
        Ok(())
    })?;
    writeln!(output, "metrics collect: updated {n} post(s)").ok();
    if quit_early {
        writeln!(
            output,
            "  (quit early; remaining batch deferred to next run)",
        )
        .ok();
    }
    Ok(())
}

fn print_preview<W: Write>(output: &mut W, post: &crate::post::Post, slug: &str, target: Target) {
    let entry = post.metadata.targets.iter().find(|t| t.name == target);
    let url = entry
        .and_then(|t| t.url.as_deref())
        .unwrap_or("(no url recorded)");
    writeln!(output, "---").ok();
    writeln!(output, "{} ({slug})", post.metadata.title).ok();
    writeln!(output, "{target}: {url}").ok();
    match entry.and_then(|t| t.metrics.as_ref()) {
        None => {
            writeln!(output, "  last sampled: never").ok();
        }
        Some(m) => {
            let when = m.sampled_at.format(&Rfc3339).unwrap_or_else(|_| "?".into());
            writeln!(
                output,
                "  last sampled: {when} (impressions: {}, reactions: {}, comments: {}, reposts: {})",
                m.impressions, m.reactions, m.comments, m.reposts,
            )
            .ok();
        }
    }
}

fn parse_sampled_at(raw: Option<&str>) -> Result<OffsetDateTime> {
    match raw {
        None => Ok(OffsetDateTime::now_utc()),
        Some(s) => datetime::parse_timestamp(s, Flag::SampledAt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn parse_sampled_at_defaults_to_now_when_none() {
        let parsed = parse_sampled_at(None).unwrap();
        // Just check it returned something in the right ballpark.
        // Real now() is non-deterministic so we can't assert exact.
        let drift = (OffsetDateTime::now_utc() - parsed).abs();
        assert!(drift < time::Duration::seconds(2));
    }

    #[test]
    fn parse_sampled_at_accepts_rfc3339() {
        let parsed = parse_sampled_at(Some("2026-05-14T00:00:00Z")).unwrap();
        assert_eq!(parsed, datetime!(2026-05-14 00:00:00 UTC));
    }

    #[test]
    fn parse_sampled_at_rejects_garbage_with_useful_error() {
        let err = parse_sampled_at(Some("yesterday")).unwrap_err();
        assert!(
            matches!(err, Error::InvalidSampledAt { ref value, .. } if value == "yesterday"),
            "got: {err:?}"
        );
    }
}
