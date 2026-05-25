#![forbid(unsafe_code)]

//! Per-dimension summary: counts and percentiles for every
//! classification value across the workdir.
//!
//! Pure data — no I/O, no rendering, no clap. Commands consume the
//! `Summary` struct and decide between text and JSON output.

use std::collections::BTreeMap;

use serde::Serialize;
use time::OffsetDateTime;

use crate::analytics::percentile::{percentiles_f64, percentiles_u64, Percentiles};
use crate::analytics::DerivedMetrics;
use crate::classifications::Classifications;
use crate::post::Post;
use crate::target::Target;

/// Rows with fewer than this many samples get a `(low n)` text
/// suffix / `"low_n": true` JSON flag. The threshold is fixed at v1
/// per the issue spec; analytics::compare uses a separate
/// configurable `--min-n` for cell suppression — different concept,
/// different knob.
pub const LOW_N_THRESHOLD: usize = 3;

/// Top-level summary. Serializes directly to the documented
/// `{ "dimensions": [...] }` JSON shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub dimensions: Vec<DimensionSummary>,
}

/// One dimension and its sorted values. `name` is the
/// `Classifications` field name (`format`, `hook`, …) — matches
/// what the `[classifications.<name>]` table in `.blog-os.toml`
/// declares.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DimensionSummary {
    pub name: String,
    pub values: Vec<ValueSummary>,
}

/// One value within a dimension. `engagement_rate` is `None` when
/// every sample's engagement_rate was `None` (e.g. all samples had
/// zero impressions). `impressions` and `interactions` are always
/// present when the row exists (the row exists iff n ≥ 1).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValueSummary {
    pub value: String,
    pub n: usize,
    pub impressions: Percentiles<u64>,
    pub interactions: Percentiles<u64>,
    pub engagement_rate: Option<Percentiles<f64>>,
    pub low_n: bool,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    impressions: u64,
    interactions: u64,
    engagement_rate: Option<f64>,
}

/// Walk `posts`, filter by `target_filter` and `dimension_filter`,
/// and emit a `Summary`. A post contributes one sample to a
/// `(dimension, value)` bucket for each of its targets that:
///
/// 1. Matches `target_filter` (or every target, if `None`).
/// 2. Has `metrics` set — targets without metrics are excluded
///    entirely per the issue's "Posts missing metrics for the
///    relevant target are excluded (not counted as zero)" rule.
///
/// `now` is injected so tests stay deterministic (passed to
/// `DerivedMetrics::from_target` for the engagement_rate
/// computation; nothing else uses it).
pub fn compute(
    posts: &[Post],
    target_filter: Option<Target>,
    dimension_filter: Option<&str>,
    now: OffsetDateTime,
) -> Summary {
    let mut buckets: BTreeMap<String, BTreeMap<String, Vec<Sample>>> = BTreeMap::new();

    for post in posts {
        for target in &post.metadata.targets {
            if let Some(filter) = target_filter {
                if target.name != filter {
                    continue;
                }
            }
            let Some(metrics) = &target.metrics else {
                continue;
            };
            let derived = DerivedMetrics::from_target(target, now);
            let sample = Sample {
                impressions: metrics.impressions,
                interactions: derived.interactions,
                engagement_rate: derived.engagement_rate,
            };
            for (dim, value) in classification_entries(&post.metadata.classifications) {
                if let Some(filter) = dimension_filter {
                    if dim != filter {
                        continue;
                    }
                }
                buckets
                    .entry(dim.to_string())
                    .or_default()
                    .entry(value.to_string())
                    .or_default()
                    .push(sample);
            }
        }
    }

    let mut dimensions: Vec<DimensionSummary> = buckets
        .into_iter()
        .map(|(name, values_map)| {
            let mut values: Vec<ValueSummary> = values_map
                .into_iter()
                .map(|(value, samples)| value_summary(value, &samples))
                .collect();
            // Sort by median engagement_rate descending; None medians
            // sort to the end (low-information signals get demoted).
            values.sort_by(|a, b| {
                er_median(b)
                    .partial_cmp(&er_median(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            DimensionSummary { name, values }
        })
        .collect();
    // Stable iteration order on the top-level dimension list — BTreeMap
    // already does this alphabetically; preserve it.
    dimensions.sort_by(|a, b| a.name.cmp(&b.name));
    Summary { dimensions }
}

fn er_median(v: &ValueSummary) -> Option<f64> {
    v.engagement_rate.map(|p| p.p50)
}

fn value_summary(value: String, samples: &[Sample]) -> ValueSummary {
    let impressions_vec: Vec<u64> = samples.iter().map(|s| s.impressions).collect();
    let interactions_vec: Vec<u64> = samples.iter().map(|s| s.interactions).collect();
    let er_vec: Vec<f64> = samples.iter().filter_map(|s| s.engagement_rate).collect();
    ValueSummary {
        value,
        n: samples.len(),
        impressions: percentiles_u64(&impressions_vec).expect("samples.len() >= 1 by construction"),
        interactions: percentiles_u64(&interactions_vec)
            .expect("samples.len() >= 1 by construction"),
        engagement_rate: percentiles_f64(&er_vec),
        low_n: samples.len() < LOW_N_THRESHOLD,
    }
}

/// Yield every `(dimension_name, value)` pair set on a
/// `Classifications`. Single-valued fields yield 0 or 1 entries;
/// the multi-valued `theme` yields 0 or N. Field names match the
/// `Classifications` struct's field names (which match the
/// `[classifications.<name>]` table in `.blog-os.toml`).
fn classification_entries(c: &Classifications) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if let Some(v) = &c.format {
        out.push(("format", v.clone()));
    }
    if let Some(v) = &c.hook {
        out.push(("hook", v.clone()));
    }
    if let Some(v) = &c.tone {
        out.push(("tone", v.clone()));
    }
    if let Some(v) = &c.audience {
        out.push(("audience", v.clone()));
    }
    if let Some(v) = &c.strategic_role {
        out.push(("strategic_role", v.clone()));
    }
    for v in &c.theme {
        out.push(("theme", v.clone()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    use crate::kind::Kind;
    use crate::post::PostMetadata;
    use crate::stage::Stage;
    use crate::target::{TargetEntry, TargetMetrics, TargetStatus};

    fn now() -> OffsetDateTime {
        datetime!(2026-05-17 00:00:00 UTC)
    }

    fn fixture_post(
        slug: &str,
        format: &str,
        themes: &[&str],
        target: Target,
        impressions: u64,
        reactions: u64,
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
                    name: target,
                    status: TargetStatus::Published,
                    url: Some("https://example.invalid".into()),
                    published_at: Some(datetime!(2026-05-08 14:32:00 UTC)),
                    metrics: Some(TargetMetrics {
                        impressions,
                        reactions,
                        comments: 0,
                        reposts: 0,
                        sampled_at: datetime!(2026-05-14 00:00:00 UTC),
                    }),
                }],
                classifications: Classifications {
                    format: Some(format.into()),
                    theme: themes.iter().map(|s| (*s).to_string()).collect(),
                    ..Default::default()
                },
                ai: None,
            },
            "body\n",
        )
    }

    fn unclassified_post_with_metrics(impressions: u64, reactions: u64) -> Post {
        Post::new(
            PostMetadata {
                title: "u".into(),
                slug: "u".into(),
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
                    url: None,
                    published_at: None,
                    metrics: Some(TargetMetrics {
                        impressions,
                        reactions,
                        comments: 0,
                        reposts: 0,
                        sampled_at: datetime!(2026-05-14 00:00:00 UTC),
                    }),
                }],
                classifications: Classifications::default(),
                ai: None,
            },
            "body\n",
        )
    }

    #[test]
    fn empty_input_yields_empty_summary() {
        let s = compute(&[], None, None, now());
        assert!(s.dimensions.is_empty());
    }

    #[test]
    fn unclassified_posts_do_not_appear_in_summary() {
        // The post has metrics but no classifications — nothing to
        // aggregate by. Empty result.
        let posts = vec![unclassified_post_with_metrics(1000, 50)];
        let s = compute(&posts, None, None, now());
        assert!(s.dimensions.is_empty());
    }

    #[test]
    fn posts_without_metrics_are_excluded() {
        let mut p = fixture_post("a", "thesis", &[], Target::Linkedin, 1000, 50);
        p.metadata.targets[0].metrics = None;
        let s = compute(&[p], None, None, now());
        assert!(s.dimensions.is_empty());
    }

    #[test]
    fn target_filter_excludes_other_targets() {
        let blog_post = {
            let mut p = fixture_post("a", "thesis", &[], Target::Blog, 5000, 100);
            p.metadata.slug = "a".into();
            p
        };
        let li_post = fixture_post("b", "thesis", &[], Target::Linkedin, 1000, 50);

        let li_only = compute(&[blog_post, li_post], Some(Target::Linkedin), None, now());
        // One dim, one value, n=1 — the blog post was filtered out.
        assert_eq!(li_only.dimensions.len(), 1);
        let format = &li_only.dimensions[0];
        assert_eq!(format.values[0].n, 1);
    }

    #[test]
    fn dimension_filter_restricts_to_one_dimension() {
        let p = fixture_post("a", "thesis", &["ambiguity"], Target::Linkedin, 1000, 50);
        let only_format = compute(std::slice::from_ref(&p), None, Some("format"), now());
        assert_eq!(only_format.dimensions.len(), 1);
        assert_eq!(only_format.dimensions[0].name, "format");
        // And no_filter yields both dimensions for the same post.
        let all = compute(&[p], None, None, now());
        assert_eq!(all.dimensions.len(), 2);
    }

    #[test]
    fn low_n_flag_is_set_below_threshold() {
        // n=2 for thesis. LOW_N_THRESHOLD is 3 → low_n: true.
        let posts = vec![
            fixture_post("a", "thesis", &[], Target::Linkedin, 1000, 50),
            fixture_post("b", "thesis", &[], Target::Linkedin, 2000, 80),
        ];
        let s = compute(&posts, None, None, now());
        let thesis = &s.dimensions[0].values[0];
        assert_eq!(thesis.n, 2);
        assert!(thesis.low_n);
    }

    #[test]
    fn low_n_flag_is_unset_at_or_above_threshold() {
        let posts = vec![
            fixture_post("a", "thesis", &[], Target::Linkedin, 1000, 50),
            fixture_post("b", "thesis", &[], Target::Linkedin, 2000, 80),
            fixture_post("c", "thesis", &[], Target::Linkedin, 1500, 60),
        ];
        let s = compute(&posts, None, None, now());
        let thesis = &s.dimensions[0].values[0];
        assert_eq!(thesis.n, 3);
        assert!(!thesis.low_n);
    }

    #[test]
    fn values_sorted_by_median_engagement_rate_descending() {
        // Two formats. parable has lower engagement, thesis higher.
        let posts = vec![
            fixture_post("p1", "parable", &[], Target::Linkedin, 1000, 20),
            fixture_post("p2", "parable", &[], Target::Linkedin, 1000, 20),
            fixture_post("p3", "parable", &[], Target::Linkedin, 1000, 20),
            fixture_post("t1", "thesis", &[], Target::Linkedin, 1000, 60),
            fixture_post("t2", "thesis", &[], Target::Linkedin, 1000, 60),
            fixture_post("t3", "thesis", &[], Target::Linkedin, 1000, 60),
        ];
        let s = compute(&posts, None, None, now());
        let format_dim = s
            .dimensions
            .iter()
            .find(|d| d.name == "format")
            .expect("format dim");
        assert_eq!(format_dim.values[0].value, "thesis");
        assert_eq!(format_dim.values[1].value, "parable");
    }

    #[test]
    fn multi_valued_theme_contributes_to_every_listed_theme() {
        // One post with theme=[a, b] contributes to both a and b.
        let p = fixture_post(
            "x",
            "thesis",
            &["ambiguity", "delivery"],
            Target::Linkedin,
            1000,
            50,
        );
        let s = compute(&[p], None, Some("theme"), now());
        let theme = &s.dimensions[0];
        assert_eq!(theme.values.len(), 2);
        for v in &theme.values {
            assert_eq!(v.n, 1, "value {} should have n=1", v.value);
        }
    }

    #[test]
    fn zero_impressions_post_contributes_to_n_but_not_engagement_rate_stream() {
        // Post with metrics but impressions=0 has engagement_rate=None.
        // It still counts in n (it has metrics), but doesn't influence
        // the engagement_rate percentile stream.
        let posts = vec![
            fixture_post("a", "thesis", &[], Target::Linkedin, 0, 5),
            fixture_post("b", "thesis", &[], Target::Linkedin, 100, 5),
            fixture_post("c", "thesis", &[], Target::Linkedin, 200, 10),
        ];
        let s = compute(&posts, None, None, now());
        let thesis = s
            .dimensions
            .iter()
            .find(|d| d.name == "format")
            .unwrap()
            .values
            .iter()
            .find(|v| v.value == "thesis")
            .unwrap();
        assert_eq!(thesis.n, 3);
        let er = thesis.engagement_rate.expect("at least 2 samples with er");
        // p50 of (0.05, 0.05) → 0.05 (nearest-rank of two equal values).
        assert!((er.p50 - 0.05).abs() < 1e-9, "got: {}", er.p50);
    }

    #[test]
    fn json_shape_matches_documented_schema() {
        // Round-trip a single-row summary through serde_json and
        // check the field names match what #439's spec documents.
        let posts = vec![
            fixture_post("a", "thesis", &[], Target::Linkedin, 1000, 50),
            fixture_post("b", "thesis", &[], Target::Linkedin, 1200, 60),
            fixture_post("c", "thesis", &[], Target::Linkedin, 1500, 75),
        ];
        let s = compute(&posts, None, Some("format"), now());
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        let thesis = &json["dimensions"][0]["values"][0];
        assert_eq!(thesis["value"], "thesis");
        assert_eq!(thesis["n"], 3);
        // p25/p50/p75 keys exist for every metric.
        assert!(thesis["impressions"]["p50"].is_number());
        assert!(thesis["interactions"]["p25"].is_number());
        assert!(thesis["engagement_rate"]["p75"].is_number());
        assert_eq!(thesis["low_n"], false);
    }
}
