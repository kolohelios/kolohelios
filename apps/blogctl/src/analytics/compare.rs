#![forbid(unsafe_code)]

//! Pairwise crosstab of two dimensions, plus per-dimension marginals.
//!
//! "Thesis posts with a contradiction hook" vs. "thesis posts with a
//! direct-claim hook" is the load-bearing question this answers; the
//! marginals (the per-dimension medians, ignoring the other axis)
//! exist so the user can tell whether a strong cell is the cell's
//! intersection winning or just the strong dimension dragging its
//! weight.

use std::collections::BTreeMap;

use serde::Serialize;
use time::OffsetDateTime;

use crate::analytics::percentile::{percentiles_f64, percentiles_u64};
use crate::analytics::summary;
use crate::analytics::DerivedMetrics;
use crate::classifications::Classifications;
use crate::post::Post;
use crate::target::Target;

/// Full result of a `compare <dim_a> <dim_b>` query — every cell
/// (non-empty intersection only) plus both marginals.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comparison {
    pub dim_a: String,
    pub dim_b: String,
    pub target_filter: Option<Target>,
    pub min_n: usize,
    /// Non-empty cells only. Cells with n == 0 don't appear — the
    /// text renderer fills them in with `--`, but in the data model
    /// "no posts" is encoded by absence rather than a zero row.
    pub cells: Vec<Cell>,
    /// dim_a values collapsed across all of dim_b.
    pub marginals_a: Vec<Marginal>,
    /// dim_b values collapsed across all of dim_a.
    pub marginals_b: Vec<Marginal>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cell {
    pub value_a: String,
    pub value_b: String,
    pub n: usize,
    pub impressions_p50: u64,
    /// `None` when every sample in the cell had `engagement_rate ==
    /// None` (e.g. all zero-impression).
    pub engagement_rate_p50: Option<f64>,
    pub low_n: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Marginal {
    pub dimension: String,
    pub value: String,
    pub n: usize,
    pub impressions_p50: u64,
    pub engagement_rate_p50: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    impressions: u64,
    engagement_rate: Option<f64>,
}

/// Compute the crosstab. A post contributes one sample to every
/// `(value_a, value_b)` cross-product of its values on `dim_a` and
/// `dim_b`, filtered to targets that match `target_filter` (or all
/// targets, if `None`) and have metrics. Multi-valued dimensions
/// (e.g. `theme: [a, b]`) explode into multiple cells.
pub fn compute(
    posts: &[Post],
    dim_a: &str,
    dim_b: &str,
    target_filter: Option<Target>,
    min_n: usize,
    now: OffsetDateTime,
) -> Comparison {
    let mut buckets: BTreeMap<(String, String), Vec<Sample>> = BTreeMap::new();

    for post in posts {
        let values_a = dimension_values(&post.metadata.classifications, dim_a);
        let values_b = dimension_values(&post.metadata.classifications, dim_b);
        if values_a.is_empty() || values_b.is_empty() {
            continue;
        }
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
                engagement_rate: derived.engagement_rate,
            };
            for va in &values_a {
                for vb in &values_b {
                    buckets
                        .entry((va.clone(), vb.clone()))
                        .or_default()
                        .push(sample);
                }
            }
        }
    }

    let cells: Vec<Cell> = buckets
        .into_iter()
        .map(|((value_a, value_b), samples)| build_cell(value_a, value_b, &samples, min_n))
        .collect();

    let marginals_a = build_marginals(posts, dim_a, target_filter, now);
    let marginals_b = build_marginals(posts, dim_b, target_filter, now);

    Comparison {
        dim_a: dim_a.to_string(),
        dim_b: dim_b.to_string(),
        target_filter,
        min_n,
        cells,
        marginals_a,
        marginals_b,
    }
}

/// Find a cell by (value_a, value_b). `None` when the intersection
/// had no samples. Text renderers use this to lay out a full grid
/// of cells where the data model only stores the non-empty ones.
impl Comparison {
    pub fn cell(&self, value_a: &str, value_b: &str) -> Option<&Cell> {
        self.cells
            .iter()
            .find(|c| c.value_a == value_a && c.value_b == value_b)
    }

    /// Distinct `dim_a` values across the cells, in alphabetical
    /// order. The text renderer uses these to lay out rows.
    pub fn row_values(&self) -> Vec<String> {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for c in &self.cells {
            s.insert(c.value_a.clone());
        }
        s.into_iter().collect()
    }

    /// Distinct `dim_b` values across the cells, in alphabetical
    /// order. The text renderer uses these for column headers.
    pub fn column_values(&self) -> Vec<String> {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for c in &self.cells {
            s.insert(c.value_b.clone());
        }
        s.into_iter().collect()
    }
}

fn build_cell(value_a: String, value_b: String, samples: &[Sample], min_n: usize) -> Cell {
    let impressions: Vec<u64> = samples.iter().map(|s| s.impressions).collect();
    let engagement_rates: Vec<f64> = samples.iter().filter_map(|s| s.engagement_rate).collect();
    Cell {
        value_a,
        value_b,
        n: samples.len(),
        impressions_p50: percentiles_u64(&impressions)
            .expect("samples.len() >= 1 by construction")
            .p50,
        engagement_rate_p50: percentiles_f64(&engagement_rates).map(|p| p.p50),
        low_n: samples.len() < min_n,
    }
}

fn build_marginals(
    posts: &[Post],
    dim: &str,
    target_filter: Option<Target>,
    now: OffsetDateTime,
) -> Vec<Marginal> {
    // Reuse summary::compute restricted to this one dimension —
    // free percentile path, same definition of "sample".
    let s = summary::compute(posts, target_filter, Some(dim), now);
    s.dimensions
        .into_iter()
        .flat_map(|d| {
            d.values.into_iter().map(move |v| Marginal {
                dimension: d.name.clone(),
                value: v.value,
                n: v.n,
                impressions_p50: v.impressions.p50,
                engagement_rate_p50: v.engagement_rate.map(|p| p.p50),
            })
        })
        .collect()
}

fn dimension_values(c: &Classifications, dim: &str) -> Vec<String> {
    match dim {
        "format" => c.format.iter().cloned().collect(),
        "hook" => c.hook.iter().cloned().collect(),
        "tone" => c.tone.iter().cloned().collect(),
        "audience" => c.audience.iter().cloned().collect(),
        "strategic_role" => c.strategic_role.iter().cloned().collect(),
        "theme" => c.theme.clone(),
        _ => vec![], // unknown dimension — no samples contribute
    }
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
        format: Option<&str>,
        hook: Option<&str>,
        themes: &[&str],
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
                    name: Target::Linkedin,
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
                    format: format.map(|s| s.to_string()),
                    hook: hook.map(|s| s.to_string()),
                    theme: themes.iter().map(|s| (*s).to_string()).collect(),
                    ..Default::default()
                },
            },
            "body\n",
        )
    }

    #[test]
    fn empty_input_yields_no_cells_no_marginals() {
        let c = compute(&[], "format", "hook", None, 3, now());
        assert!(c.cells.is_empty());
        assert!(c.marginals_a.is_empty());
        assert!(c.marginals_b.is_empty());
    }

    #[test]
    fn cell_only_appears_when_intersection_is_non_empty() {
        // thesis + contradiction posts, but no thesis + question.
        let posts = vec![
            fixture_post("a", Some("thesis"), Some("contradiction"), &[], 1000, 50),
            fixture_post("b", Some("thesis"), Some("contradiction"), &[], 1500, 80),
        ];
        let c = compute(&posts, "format", "hook", None, 3, now());
        assert_eq!(c.cells.len(), 1);
        assert_eq!(c.cells[0].value_a, "thesis");
        assert_eq!(c.cells[0].value_b, "contradiction");
        assert_eq!(c.cells[0].n, 2);
        assert!(c.cell("thesis", "question").is_none());
    }

    #[test]
    fn low_n_flag_uses_provided_min_n_threshold() {
        // min_n = 5, n = 2 → low_n=true. Same data with min_n = 2
        // → low_n=false.
        let posts = vec![
            fixture_post("a", Some("thesis"), Some("contradiction"), &[], 1000, 50),
            fixture_post("b", Some("thesis"), Some("contradiction"), &[], 1500, 80),
        ];
        let strict = compute(&posts, "format", "hook", None, 5, now());
        assert!(strict.cells[0].low_n);
        let lax = compute(&posts, "format", "hook", None, 2, now());
        assert!(!lax.cells[0].low_n);
    }

    #[test]
    fn multi_valued_theme_explodes_into_multiple_cells() {
        // One post with theme=[a, b] and format=thesis → contributes
        // to (thesis, a) AND (thesis, b).
        let p = fixture_post(
            "x",
            Some("thesis"),
            None,
            &["ambiguity", "delivery"],
            1000,
            50,
        );
        let c = compute(&[p], "format", "theme", None, 3, now());
        assert_eq!(c.cells.len(), 2);
        for cell in &c.cells {
            assert_eq!(cell.value_a, "thesis");
            assert_eq!(cell.n, 1);
        }
    }

    #[test]
    fn marginals_are_independent_of_the_crosstab() {
        // 3 thesis posts, 2 with hook=contradiction, 1 with no hook.
        // Crosstab has 1 cell (thesis × contradiction, n=2). The
        // marginal for thesis should still see all 3.
        let posts = vec![
            fixture_post("a", Some("thesis"), Some("contradiction"), &[], 1000, 50),
            fixture_post("b", Some("thesis"), Some("contradiction"), &[], 1500, 80),
            fixture_post("c", Some("thesis"), None, &[], 800, 30),
        ];
        let c = compute(&posts, "format", "hook", None, 3, now());
        // Cells: only thesis × contradiction with n=2.
        assert_eq!(c.cells.len(), 1);
        assert_eq!(c.cells[0].n, 2);
        // marginal_a (format=thesis) should see n=3 — independent of
        // whether the post has a hook set.
        let thesis_marg = c.marginals_a.iter().find(|m| m.value == "thesis").unwrap();
        assert_eq!(thesis_marg.n, 3);
    }

    #[test]
    fn unknown_dimension_yields_no_cells() {
        // The dim name doesn't match any Classifications field —
        // dimension_values returns empty, no samples contribute.
        let p = fixture_post("x", Some("thesis"), Some("contradiction"), &[], 1000, 50);
        let c = compute(&[p], "format", "not-a-dim", None, 3, now());
        assert!(c.cells.is_empty());
        // dim_a's marginal still computes — only the unknown dim drops.
        // (summary::compute on "not-a-dim" yields no dimension, so
        // marginals_b is empty too.)
        assert!(c.marginals_b.is_empty());
        // marginals_a should have format=thesis.
        let thesis = c.marginals_a.iter().find(|m| m.value == "thesis").unwrap();
        assert_eq!(thesis.n, 1);
    }

    #[test]
    fn target_filter_excludes_other_targets() {
        // One post with metrics on Linkedin AND a separate post with
        // metrics on Blog (same classification) — Linkedin filter
        // sees only the first sample.
        let li = fixture_post("li", Some("thesis"), Some("contradiction"), &[], 1000, 50);
        let mut blog = fixture_post(
            "blog",
            Some("thesis"),
            Some("contradiction"),
            &[],
            5000,
            100,
        );
        blog.metadata.targets[0].name = Target::Blog;
        let c = compute(
            &[li, blog],
            "format",
            "hook",
            Some(Target::Linkedin),
            3,
            now(),
        );
        assert_eq!(c.cells.len(), 1);
        assert_eq!(c.cells[0].n, 1);
    }

    #[test]
    fn row_and_column_values_are_alphabetical() {
        let posts = vec![
            fixture_post("a", Some("thesis"), Some("contradiction"), &[], 1000, 50),
            fixture_post("b", Some("parable"), Some("question"), &[], 1000, 50),
            fixture_post("c", Some("parable"), Some("contradiction"), &[], 1000, 50),
        ];
        let c = compute(&posts, "format", "hook", None, 3, now());
        assert_eq!(c.row_values(), vec!["parable", "thesis"]);
        assert_eq!(c.column_values(), vec!["contradiction", "question"]);
    }

    #[test]
    fn json_shape_serializes_expected_fields() {
        let posts = vec![fixture_post(
            "a",
            Some("thesis"),
            Some("contradiction"),
            &[],
            1000,
            50,
        )];
        let c = compute(&posts, "format", "hook", None, 3, now());
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(v["dim_a"], "format");
        assert_eq!(v["dim_b"], "hook");
        assert_eq!(v["min_n"], 3);
        assert!(v["target_filter"].is_null());
        let cell = &v["cells"][0];
        assert_eq!(cell["value_a"], "thesis");
        assert_eq!(cell["value_b"], "contradiction");
        assert_eq!(cell["n"], 1);
        assert_eq!(cell["low_n"], true);
        assert!(cell["impressions_p50"].is_number());
        // engagement_rate_p50 is a number (0.05) — not null.
        assert!(cell["engagement_rate_p50"].is_number());
    }
}
