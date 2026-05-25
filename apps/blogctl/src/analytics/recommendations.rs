#![forbid(unsafe_code)]

//! Heuristic observations across the analytics corpus, with the
//! hedge language enforced by the renderer.
//!
//! Three constraints make this module unusual:
//!
//! 1. **Honesty is the load-bearing feature.** Every observation
//!    carries `n`; every quantitative claim gets an "Early signal:"
//!    prefix; low-n cases get "Insufficient data:". The renderer is
//!    the language gate — never call out to ad-hoc formatting.
//! 2. **No JSON output.** Recommendations are human-shaped prose
//!    that exists to *be hedged*. A JSON pathway would tempt
//!    consumers to strip the hedges and treat the claims as facts.
//! 3. **No statistical tests.** The corpus is too small (tens of
//!    posts). Pretending otherwise would be the exact dishonesty
//!    this module is preventing.

use time::OffsetDateTime;

use crate::analytics::compare;
use crate::analytics::percentile::percentiles_f64;
use crate::analytics::summary;
use crate::analytics::DerivedMetrics;
use crate::post::Post;
use crate::target::Target;

/// A cell / value must beat its baseline by this much (relative) to
/// register as a lift. 1.5 = 50% relative lift, matching #441's spec.
pub const LIFT_THRESHOLD: f64 = 1.5;

/// Targets with metrics older than this trigger the stale-data
/// observation. The cap is per the spec; not configurable.
pub const STALE_METRICS_THRESHOLD_DAYS: i64 = 30;

/// Cap on the number of *inference* observations we emit (dimension
/// lift + interaction lift + underrepresented). Stale-data
/// observations are informational and don't count against the cap.
pub const MAX_INFERENCE_OBSERVATIONS: usize = 5;

/// Words that must NEVER appear in rendered observations. The
/// language-gate test greps for these to catch regressions.
pub const FORBIDDEN_WORDS: &[&str] = &["proves", "shows", "best", "winning"];

/// The result of one `analytics recommendations` run.
#[derive(Debug, Clone, PartialEq)]
pub struct Recommendations {
    pub target_filter: Option<Target>,
    /// Number of distinct posts with at least one (filtered) target
    /// that has metrics. Drives the header line.
    pub n_total: usize,
    pub observations: Vec<Observation>,
}

/// One heuristic observation. The hedge prefix lives in the
/// renderer — `Observation` itself is structured data with the
/// minimum context each heuristic produces.
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    /// A dimension value's median engagement_rate ≥ LIFT_THRESHOLD×
    /// the corpus median, AND n ≥ min_n.
    DimensionLift {
        dimension: String,
        value: String,
        n: usize,
        value_median_er: f64,
        corpus_median_er: f64,
    },
    /// A `(dim_a, dim_b)` cell's median engagement_rate beats at
    /// least one of its marginals by ≥ LIFT_THRESHOLD relative, AND
    /// cell_n ≥ min_n.
    InteractionLift {
        dim_a: String,
        value_a: String,
        dim_b: String,
        value_b: String,
        n: usize,
        cell_median_er: f64,
        marginal_a_median_er: f64,
        marginal_b_median_er: f64,
    },
    /// A value with above-corpus engagement_rate but n < min_n.
    /// "Worth more samples" rather than "this is real."
    Underrepresented {
        dimension: String,
        value: String,
        n: usize,
        value_median_er: f64,
        corpus_median_er: f64,
        min_n: usize,
    },
    /// Published targets whose metrics haven't been refreshed in
    /// >STALE_METRICS_THRESHOLD_DAYS days.
    StaleMetrics {
        slugs: Vec<String>,
        threshold_days: i64,
    },
}

impl Observation {
    /// Render the hedged prose. Each variant's template bakes in
    /// the required prefix and avoids the FORBIDDEN_WORDS list;
    /// callers concatenate these into the full output.
    pub fn render(&self) -> String {
        match self {
            Self::DimensionLift {
                dimension,
                value,
                n,
                value_median_er,
                corpus_median_er,
            } => format!(
                "Early signal: {dimension}={value} correlates with higher engagement \
                 (median {} vs corpus {}; n={n}).",
                fmt_er(*value_median_er),
                fmt_er(*corpus_median_er),
            ),
            Self::InteractionLift {
                dim_a,
                value_a,
                dim_b,
                value_b,
                n,
                cell_median_er,
                marginal_a_median_er,
                marginal_b_median_er,
            } => format!(
                "Early signal: {dim_a}={value_a} + {dim_b}={value_b} outperformed both marginals \
                 in this sample (median {}; marginals {} / {}; n={n}). Treat with caution at this n.",
                fmt_er(*cell_median_er),
                fmt_er(*marginal_a_median_er),
                fmt_er(*marginal_b_median_er),
            ),
            Self::Underrepresented {
                dimension,
                value,
                n,
                value_median_er,
                corpus_median_er,
                min_n,
            } => format!(
                "Insufficient data: {dimension}={value} has {} posts (below min_n={min_n}) \
                 but its median engagement ({}) is above corpus ({}). Consider more samples.",
                n,
                fmt_er(*value_median_er),
                fmt_er(*corpus_median_er),
            ),
            Self::StaleMetrics {
                slugs,
                threshold_days,
            } => format!(
                "Stale data: {} published post(s) have metrics older than {threshold_days} days \
                 [{}]. Run `blogctl metrics update <slug>` to refresh.",
                slugs.len(),
                slugs.join(", "),
            ),
        }
    }
}

/// Closing reminder printed unconditionally so a stray copy-paste
/// of one observation still lands with the hedge attached.
pub const CLOSING_REMINDER: &str =
    "Reminder: these are correlations on a small sample. Treat as priors for what to try next, \
     not as conclusions.";

/// Run every heuristic and emit a `Recommendations` bundle.
pub fn compute(
    posts: &[Post],
    target_filter: Option<Target>,
    min_n: usize,
    now: OffsetDateTime,
) -> Recommendations {
    let n_total = count_distinct_posts_with_metrics(posts, target_filter);
    let corpus_median_er = corpus_median_engagement_rate(posts, target_filter, now);
    let summary_all = summary::compute(posts, target_filter, None, now);
    let compare_fmt_hook = compare::compute(posts, "format", "hook", target_filter, min_n, now);

    let mut inference = Vec::new();
    if let Some(corpus) = corpus_median_er {
        inference.extend(dimension_lift(&summary_all, corpus, min_n));
        inference.extend(interaction_lift(&compare_fmt_hook, min_n));
        inference.extend(underrepresented(&summary_all, corpus, min_n));
    }
    // Cap inference observations only; stale-data is informational.
    inference.truncate(MAX_INFERENCE_OBSERVATIONS);

    let stale = stale_metrics(posts, target_filter, STALE_METRICS_THRESHOLD_DAYS, now);

    let mut observations = inference;
    observations.extend(stale);
    Recommendations {
        target_filter,
        n_total,
        observations,
    }
}

fn dimension_lift(s: &summary::Summary, corpus_median_er: f64, min_n: usize) -> Vec<Observation> {
    let mut out = Vec::new();
    for dim in &s.dimensions {
        for value in &dim.values {
            let Some(er) = value.engagement_rate else {
                continue;
            };
            if value.n >= min_n && er.p50 >= corpus_median_er * LIFT_THRESHOLD {
                out.push(Observation::DimensionLift {
                    dimension: dim.name.clone(),
                    value: value.value.clone(),
                    n: value.n,
                    value_median_er: er.p50,
                    corpus_median_er,
                });
            }
        }
    }
    out
}

fn interaction_lift(c: &compare::Comparison, min_n: usize) -> Vec<Observation> {
    let mut out = Vec::new();
    for cell in &c.cells {
        let Some(cell_er) = cell.engagement_rate_p50 else {
            continue;
        };
        if cell.n < min_n {
            continue;
        }
        let marginal_a = c
            .marginals_a
            .iter()
            .find(|m| m.value == cell.value_a)
            .and_then(|m| m.engagement_rate_p50);
        let marginal_b = c
            .marginals_b
            .iter()
            .find(|m| m.value == cell.value_b)
            .and_then(|m| m.engagement_rate_p50);
        let (Some(a_er), Some(b_er)) = (marginal_a, marginal_b) else {
            continue;
        };
        let beats_a = cell_er >= a_er * LIFT_THRESHOLD;
        let beats_b = cell_er >= b_er * LIFT_THRESHOLD;
        if beats_a || beats_b {
            out.push(Observation::InteractionLift {
                dim_a: c.dim_a.clone(),
                value_a: cell.value_a.clone(),
                dim_b: c.dim_b.clone(),
                value_b: cell.value_b.clone(),
                n: cell.n,
                cell_median_er: cell_er,
                marginal_a_median_er: a_er,
                marginal_b_median_er: b_er,
            });
        }
    }
    out
}

fn underrepresented(s: &summary::Summary, corpus_median_er: f64, min_n: usize) -> Vec<Observation> {
    let mut out = Vec::new();
    for dim in &s.dimensions {
        for value in &dim.values {
            let Some(er) = value.engagement_rate else {
                continue;
            };
            if value.n < min_n && er.p50 > corpus_median_er {
                out.push(Observation::Underrepresented {
                    dimension: dim.name.clone(),
                    value: value.value.clone(),
                    n: value.n,
                    value_median_er: er.p50,
                    corpus_median_er,
                    min_n,
                });
            }
        }
    }
    out
}

fn stale_metrics(
    posts: &[Post],
    target_filter: Option<Target>,
    threshold_days: i64,
    now: OffsetDateTime,
) -> Vec<Observation> {
    let mut stale_slugs: Vec<String> = Vec::new();
    for post in posts {
        for target in &post.metadata.targets {
            if let Some(filter) = target_filter {
                if target.name != filter {
                    continue;
                }
            }
            let derived = DerivedMetrics::from_target(target, now);
            if let Some(days) = derived.staleness_days {
                if days > threshold_days {
                    if !stale_slugs.contains(&post.metadata.slug) {
                        stale_slugs.push(post.metadata.slug.clone());
                    }
                    // Avoid emitting once per stale-target on the same post.
                    break;
                }
            }
        }
    }
    if stale_slugs.is_empty() {
        Vec::new()
    } else {
        vec![Observation::StaleMetrics {
            slugs: stale_slugs,
            threshold_days,
        }]
    }
}

fn count_distinct_posts_with_metrics(posts: &[Post], target_filter: Option<Target>) -> usize {
    posts
        .iter()
        .filter(|p| {
            p.metadata.targets.iter().any(|t| {
                if let Some(filter) = target_filter {
                    if t.name != filter {
                        return false;
                    }
                }
                t.metrics.is_some()
            })
        })
        .count()
}

fn corpus_median_engagement_rate(
    posts: &[Post],
    target_filter: Option<Target>,
    now: OffsetDateTime,
) -> Option<f64> {
    let mut rates = Vec::new();
    for post in posts {
        for target in &post.metadata.targets {
            if let Some(filter) = target_filter {
                if target.name != filter {
                    continue;
                }
            }
            if target.metrics.is_some() {
                let derived = DerivedMetrics::from_target(target, now);
                if let Some(er) = derived.engagement_rate {
                    rates.push(er);
                }
            }
        }
    }
    percentiles_f64(&rates).map(|p| p.p50)
}

fn fmt_er(rate: f64) -> String {
    format!("{:.1}%", rate * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    use crate::classifications::Classifications;
    use crate::kind::Kind;
    use crate::post::PostMetadata;
    use crate::stage::Stage;
    use crate::target::{TargetEntry, TargetMetrics, TargetStatus};

    fn now() -> OffsetDateTime {
        datetime!(2026-05-17 00:00:00 UTC)
    }

    fn post_with(
        slug: &str,
        format: Option<&str>,
        hook: Option<&str>,
        impressions: u64,
        reactions: u64,
        sampled_at: OffsetDateTime,
    ) -> Post {
        Post::new(
            PostMetadata {
                title: slug.into(),
                slug: slug.into(),
                kind: Kind::Post,
                theme: "standard".into(),
                status: Stage::Published,
                created_at: datetime!(2026-05-01 00:00:00 UTC),
                updated_at: datetime!(2026-05-08 00:00:00 UTC),
                tags: vec![],
                todoist_task_id: None,
                history_checked: false,
                targets: vec![TargetEntry {
                    name: Target::Linkedin,
                    status: TargetStatus::Published,
                    url: Some("https://example.invalid".into()),
                    published_at: Some(datetime!(2026-05-08 14:32:00 UTC)),
                    metrics: Some(TargetMetrics {
                        impressions,
                        reactions,
                        comments: 0,
                        reposts: 0,
                        sampled_at,
                    }),
                }],
                classifications: Classifications {
                    format: format.map(|s| s.into()),
                    hook: hook.map(|s| s.into()),
                    ..Default::default()
                },
                ai: None,
            },
            "body\n",
        )
    }

    fn no_metrics_post(slug: &str) -> Post {
        let mut p = post_with(
            slug,
            Some("thesis"),
            None,
            1000,
            50,
            datetime!(2026-05-14 00:00:00 UTC),
        );
        p.metadata.targets[0].metrics = None;
        p
    }

    #[test]
    fn dimension_lift_fires_when_one_format_dominates_by_5x() {
        // 5 thesis posts at 10% engagement, 5 parable at 2% — corpus
        // median is somewhere in between; thesis sits well above
        // the 1.5x lift threshold.
        let posts: Vec<_> = (0..5)
            .map(|i| {
                post_with(
                    &format!("t{i}"),
                    Some("thesis"),
                    None,
                    1000,
                    100, // 10%
                    datetime!(2026-05-14 00:00:00 UTC),
                )
            })
            .chain((0..5).map(|i| {
                post_with(
                    &format!("p{i}"),
                    Some("parable"),
                    None,
                    1000,
                    20, // 2%
                    datetime!(2026-05-14 00:00:00 UTC),
                )
            }))
            .collect();
        let r = compute(&posts, Some(Target::Linkedin), 5, now());
        // dimension_lift should fire for thesis.
        let lifts: Vec<&Observation> = r
            .observations
            .iter()
            .filter(|o| matches!(o, Observation::DimensionLift { .. }))
            .collect();
        assert!(
            lifts.iter().any(
                |o| matches!(o, Observation::DimensionLift { value, .. } if value == "thesis")
            ),
            "expected DimensionLift for thesis: {:?}",
            r.observations,
        );
    }

    #[test]
    fn no_metrics_in_corpus_yields_only_stale_or_nothing() {
        // Every post is metric-less → corpus_median_er is None →
        // no inference observations fire. (Stale only fires on
        // posts with metrics, so this fixture produces an empty
        // observation list.)
        let posts: Vec<_> = (0..3).map(|i| no_metrics_post(&format!("p{i}"))).collect();
        let r = compute(&posts, Some(Target::Linkedin), 5, now());
        assert!(
            r.observations.is_empty(),
            "expected no observations; got {:?}",
            r.observations
        );
        assert_eq!(r.n_total, 0);
    }

    #[test]
    fn underrepresented_fires_for_high_er_low_n() {
        // 5 parables at 2% engagement and 1 thesis at 8% — thesis's
        // engagement is well above corpus but n=1 < min_n=3.
        let mut posts: Vec<_> = (0..5)
            .map(|i| {
                post_with(
                    &format!("p{i}"),
                    Some("parable"),
                    None,
                    1000,
                    20,
                    datetime!(2026-05-14 00:00:00 UTC),
                )
            })
            .collect();
        posts.push(post_with(
            "t1",
            Some("thesis"),
            None,
            1000,
            80, // 8%
            datetime!(2026-05-14 00:00:00 UTC),
        ));
        let r = compute(&posts, Some(Target::Linkedin), 3, now());
        assert!(
            r.observations.iter().any(
                |o| matches!(o, Observation::Underrepresented { value, .. } if value == "thesis")
            ),
            "expected Underrepresented for thesis: {:?}",
            r.observations,
        );
    }

    #[test]
    fn stale_metrics_fires_for_posts_sampled_more_than_30_days_ago() {
        // Sampled 60 days before `now` → staleness > 30 → fires.
        let p = post_with(
            "old",
            Some("thesis"),
            None,
            1000,
            50,
            datetime!(2026-03-18 00:00:00 UTC),
        );
        let r = compute(&[p], Some(Target::Linkedin), 3, now());
        assert!(
            r.observations
                .iter()
                .any(|o| matches!(o, Observation::StaleMetrics { slugs, .. } if slugs.contains(&"old".to_string()))),
            "expected StaleMetrics for old: {:?}",
            r.observations,
        );
    }

    #[test]
    fn fresh_metrics_do_not_trigger_stale_observation() {
        // Sampled 3 days before `now` → staleness < 30 → silent.
        let p = post_with(
            "fresh",
            Some("thesis"),
            None,
            1000,
            50,
            datetime!(2026-05-14 00:00:00 UTC),
        );
        let r = compute(&[p], Some(Target::Linkedin), 3, now());
        assert!(
            !r.observations
                .iter()
                .any(|o| matches!(o, Observation::StaleMetrics { .. })),
            "fresh metrics should not produce stale observation: {:?}",
            r.observations,
        );
    }

    #[test]
    fn interaction_lift_fires_when_cell_beats_at_least_one_marginal() {
        // Construct a corpus where thesis+contradiction (n=3) is at
        // 10% while thesis-marginal is ~4% and contradiction-marginal
        // is ~4%. The cell beats both marginals by >1.5x.
        let mut posts = Vec::new();
        for i in 0..3 {
            posts.push(post_with(
                &format!("tc{i}"),
                Some("thesis"),
                Some("contradiction"),
                1000,
                100, // 10%
                datetime!(2026-05-14 00:00:00 UTC),
            ));
        }
        // Background thesis posts (with other hooks) bring the
        // format=thesis marginal down.
        for i in 0..5 {
            posts.push(post_with(
                &format!("td{i}"),
                Some("thesis"),
                Some("direct-claim"),
                1000,
                30, // 3%
                datetime!(2026-05-14 00:00:00 UTC),
            ));
        }
        // Background contradiction posts (other formats).
        for i in 0..5 {
            posts.push(post_with(
                &format!("pc{i}"),
                Some("parable"),
                Some("contradiction"),
                1000,
                30, // 3%
                datetime!(2026-05-14 00:00:00 UTC),
            ));
        }
        let r = compute(&posts, Some(Target::Linkedin), 3, now());
        assert!(
            r.observations.iter().any(
                |o| matches!(o, Observation::InteractionLift { value_a, value_b, .. }
                    if value_a == "thesis" && value_b == "contradiction")
            ),
            "expected InteractionLift for thesis+contradiction: {:?}",
            r.observations,
        );
    }

    #[test]
    fn rendered_observations_never_contain_forbidden_words() {
        // Build a fixture that triggers every heuristic, render the
        // observations, and grep for forbidden words.
        let mut posts = Vec::new();
        for i in 0..5 {
            posts.push(post_with(
                &format!("t{i}"),
                Some("thesis"),
                Some("contradiction"),
                1000,
                100,
                datetime!(2026-05-14 00:00:00 UTC),
            ));
        }
        for i in 0..5 {
            posts.push(post_with(
                &format!("p{i}"),
                Some("parable"),
                None,
                1000,
                20,
                datetime!(2026-05-14 00:00:00 UTC),
            ));
        }
        // A stale post.
        posts.push(post_with(
            "old",
            Some("thesis"),
            None,
            1000,
            50,
            datetime!(2026-03-18 00:00:00 UTC),
        ));
        let r = compute(&posts, Some(Target::Linkedin), 3, now());
        assert!(
            !r.observations.is_empty(),
            "fixture should produce some observations"
        );
        for obs in &r.observations {
            let rendered = obs.render();
            for word in FORBIDDEN_WORDS {
                assert!(
                    !rendered.to_lowercase().contains(word),
                    "observation {:?} rendered with forbidden word {:?}: {}",
                    obs,
                    word,
                    rendered,
                );
            }
        }
        // The closing reminder is also subject to the language gate.
        for word in FORBIDDEN_WORDS {
            assert!(
                !CLOSING_REMINDER.to_lowercase().contains(word),
                "CLOSING_REMINDER contains forbidden word {word:?}",
            );
        }
    }

    #[test]
    fn rendered_observations_always_include_n() {
        // Every inference observation must surface `n=` so the user
        // can size their confidence. Stale observations don't have a
        // single n (they're a list); they get a count instead.
        let posts: Vec<_> = (0..5)
            .map(|i| {
                post_with(
                    &format!("t{i}"),
                    Some("thesis"),
                    None,
                    1000,
                    100,
                    datetime!(2026-05-14 00:00:00 UTC),
                )
            })
            .collect();
        let r = compute(&posts, Some(Target::Linkedin), 5, now());
        for obs in &r.observations {
            let rendered = obs.render();
            match obs {
                Observation::StaleMetrics { .. } => {
                    // Stale renders `<N> published post(s)` — N is a count.
                }
                _ => {
                    assert!(
                        rendered.contains("n="),
                        "inference observation should include `n=`: {rendered}",
                    );
                }
            }
        }
    }

    #[test]
    fn inference_cap_applies_but_stale_is_unbounded() {
        // Concoct many lift opportunities. The inference cap should
        // limit them to MAX_INFERENCE_OBSERVATIONS even though more
        // are technically eligible. Stale metrics on top of that
        // still appear.
        let mut posts = Vec::new();
        // Many different formats all beating the corpus median.
        for fmt in [
            "thesis",
            "essay",
            "framework",
            "observation",
            "parable",
            "personal-reflection",
        ] {
            for i in 0..5 {
                posts.push(post_with(
                    &format!("{fmt}-{i}"),
                    Some(fmt),
                    None,
                    1000,
                    100,
                    datetime!(2026-05-14 00:00:00 UTC),
                ));
            }
        }
        // One stale post tacked on.
        posts.push(post_with(
            "old",
            Some("thesis"),
            None,
            1000,
            50,
            datetime!(2026-03-18 00:00:00 UTC),
        ));
        let r = compute(&posts, Some(Target::Linkedin), 3, now());
        let inference_count = r
            .observations
            .iter()
            .filter(|o| !matches!(o, Observation::StaleMetrics { .. }))
            .count();
        assert!(
            inference_count <= MAX_INFERENCE_OBSERVATIONS,
            "inference cap violated: {inference_count}",
        );
        // And stale-data still made it through.
        assert!(r
            .observations
            .iter()
            .any(|o| matches!(o, Observation::StaleMetrics { .. })));
    }
}
