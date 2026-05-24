//! `blogctl metrics update <slug>` / `blogctl metrics show <slug>` —
//! per-target performance numbers.
//!
//! Update overwrites; there's no history. `sampled_at` defaults to
//! `now_utc()` and acts as the freshness signal — analytics will
//! grow a "stale" notion on top, but the storage model stays a
//! single point-in-time per target.

use std::fs;
use std::path::PathBuf;

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};
use crate::target::{Target, TargetMetrics};

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

fn parse_sampled_at(raw: Option<&str>) -> Result<OffsetDateTime> {
    match raw {
        None => Ok(OffsetDateTime::now_utc()),
        Some(s) => OffsetDateTime::parse(s, &Rfc3339).map_err(|source| Error::InvalidSampledAt {
            value: s.to_string(),
            source,
        }),
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
