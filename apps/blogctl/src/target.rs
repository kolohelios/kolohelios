//! Distribution targets and their per-target state. Orthogonal to the
//! editorial `Stage` pipeline: a post is editorially in one stage
//! (concept→…→published) but may end up distributed to zero, one, or
//! more venues, each with its own small state machine.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

use crate::error::{Error, Result};

// Serialize `MetricSample::date` as a bare `YYYY-MM-DD` ISO date rather
// than `time`'s default integer encoding, keeping the frontmatter
// human-readable.
time::serde::format_description!(iso_date, Date, "[year]-[month]-[day]");

/// A venue a post can be distributed to. Closed enum — adding a new
/// venue is a deliberate code change, not a frontmatter typo away.
///
/// `ValueEnum` is derived so `--target linkedin` parses natively at
/// the clap layer; clap rejects unknown values with a helpful error
/// rather than punting validation into the command body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum Target {
    Linkedin,
    Blog,
}

impl Target {
    pub const ALL: &'static [Target] = &[Target::Linkedin, Target::Blog];

    pub fn as_str(self) -> &'static str {
        match self {
            Target::Linkedin => "linkedin",
            Target::Blog => "blog",
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Target {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "linkedin" => Ok(Target::Linkedin),
            "blog" => Ok(Target::Blog),
            other => Err(Error::InvalidTarget {
                value: other.to_string(),
            }),
        }
    }
}

/// Per-target state. Deliberately smaller than editorial `Stage` —
/// distribution is "have I posted this here, and where can I find it?"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetStatus {
    Planned,
    Published,
    Retracted,
}

impl TargetStatus {
    pub const ALL: &'static [TargetStatus] = &[
        TargetStatus::Planned,
        TargetStatus::Published,
        TargetStatus::Retracted,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TargetStatus::Planned => "planned",
            TargetStatus::Published => "published",
            TargetStatus::Retracted => "retracted",
        }
    }
}

impl fmt::Display for TargetStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TargetStatus {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "planned" => Ok(TargetStatus::Planned),
            "published" => Ok(TargetStatus::Published),
            "retracted" => Ok(TargetStatus::Retracted),
            other => Err(Error::InvalidTargetStatus {
                value: other.to_string(),
            }),
        }
    }
}

/// Performance numbers observed for a target at a point in time.
/// Held on `TargetEntry` rather than the post root because the same
/// post on LinkedIn vs. a blog will have different numbers.
///
/// `sampled_at` is when these counts were last refreshed (a manual
/// `metrics update` action) — older samples grow stale. Engagement
/// rate and post age are derived in the analytics module, not stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetMetrics {
    pub impressions: u64,
    pub reactions: u64,
    pub comments: u64,
    pub reposts: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub sampled_at: OffsetDateTime,
}

/// One day's observed performance for a target, sourced from a daily
/// analytics export (`blogctl linkedin import`). Coarser than
/// [`TargetMetrics`] — engagements is a single aggregate, not split into
/// reactions/comments/reposts — and keyed by `date` so a target accrues
/// a time-series. Either metric may be absent when an export populated
/// only one of its two ranked tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricSample {
    #[serde(with = "iso_date")]
    pub date: Date,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impressions: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engagements: Option<u64>,
}

/// One entry in a post's `targets:` list. `url` and `published_at` are
/// required when `status == Published` and optional otherwise; the
/// invariant is enforced at parse time, not by the type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetEntry {
    pub name: Target,
    pub status: TargetStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub published_at: Option<OffsetDateTime>,
    /// Latest observed metrics for this target. `None` while the
    /// target has never been measured (the common state for drafts
    /// and for posts on a venue without analytics surfaced yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<TargetMetrics>,
    /// Per-day metric samples imported from LinkedIn analytics exports,
    /// kept sorted by date and idempotent on date (see
    /// [`TargetEntry::record_sample`]). Distinct from `metrics`, which
    /// holds the latest single manual sample.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<MetricSample>,
}

impl TargetEntry {
    /// Record a daily metric sample, keeping `samples` sorted by date.
    /// Idempotent on date: a sample whose date is already present is a
    /// no-op (returns `false`); a new date is inserted in order
    /// (returns `true`). Idempotency on `(urn, date)` is what lets
    /// re-running overlapping exports add no duplicate data points.
    pub fn record_sample(&mut self, sample: MetricSample) -> bool {
        match self
            .samples
            .binary_search_by(|existing| existing.date.cmp(&sample.date))
        {
            Ok(_) => false,
            Err(idx) => {
                self.samples.insert(idx, sample);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_parse_round_trips() {
        for &t in Target::ALL {
            assert_eq!(t.as_str().parse::<Target>().unwrap(), t);
        }
    }

    #[test]
    fn target_parse_rejects_unknown() {
        let err = "mastodon".parse::<Target>().unwrap_err();
        assert!(matches!(err, Error::InvalidTarget { .. }));
    }

    #[test]
    fn target_status_parse_round_trips() {
        for &s in TargetStatus::ALL {
            assert_eq!(s.as_str().parse::<TargetStatus>().unwrap(), s);
        }
    }

    #[test]
    fn target_status_parse_rejects_unknown() {
        let err = "draft".parse::<TargetStatus>().unwrap_err();
        assert!(matches!(err, Error::InvalidTargetStatus { .. }));
    }

    #[test]
    fn target_serde_uses_kebab_case() {
        let yaml = serde_yaml_ng::to_string(&Target::Linkedin).unwrap();
        assert_eq!(yaml.trim(), "linkedin");
        let parsed: Target = serde_yaml_ng::from_str("linkedin").unwrap();
        assert_eq!(parsed, Target::Linkedin);
    }

    #[test]
    fn target_status_serde_uses_kebab_case() {
        let yaml = serde_yaml_ng::to_string(&TargetStatus::Published).unwrap();
        assert_eq!(yaml.trim(), "published");
        let parsed: TargetStatus = serde_yaml_ng::from_str("planned").unwrap();
        assert_eq!(parsed, TargetStatus::Planned);
    }

    #[test]
    fn target_entry_round_trips_published() {
        let raw = "name: linkedin\nstatus: published\nurl: https://www.linkedin.com/posts/x\npublished_at: 2026-05-08T14:32:00Z\n";
        let parsed: TargetEntry = serde_yaml_ng::from_str(raw).unwrap();
        assert_eq!(parsed.name, Target::Linkedin);
        assert_eq!(parsed.status, TargetStatus::Published);
        assert_eq!(
            parsed.url.as_deref(),
            Some("https://www.linkedin.com/posts/x")
        );
        assert!(parsed.published_at.is_some());

        let rendered = serde_yaml_ng::to_string(&parsed).unwrap();
        let reparsed: TargetEntry = serde_yaml_ng::from_str(&rendered).unwrap();
        assert_eq!(reparsed, parsed);
    }

    #[test]
    fn target_entry_round_trips_planned_without_url_or_timestamp() {
        let raw = "name: blog\nstatus: planned\n";
        let parsed: TargetEntry = serde_yaml_ng::from_str(raw).unwrap();
        assert_eq!(parsed.name, Target::Blog);
        assert_eq!(parsed.status, TargetStatus::Planned);
        assert!(parsed.url.is_none());
        assert!(parsed.published_at.is_none());

        let rendered = serde_yaml_ng::to_string(&parsed).unwrap();
        // skip_serializing_if drops the empty optional fields entirely
        assert!(!rendered.contains("url"));
        assert!(!rendered.contains("published_at"));
        assert!(!rendered.contains("metrics"));
    }

    #[test]
    fn target_metrics_round_trip() {
        let raw = concat!(
            "impressions: 1842\n",
            "reactions: 67\n",
            "comments: 14\n",
            "reposts: 5\n",
            "sampled_at: 2026-05-14T00:00:00Z\n",
        );
        let parsed: TargetMetrics = serde_yaml_ng::from_str(raw).unwrap();
        assert_eq!(parsed.impressions, 1842);
        assert_eq!(parsed.reactions, 67);
        assert_eq!(parsed.comments, 14);
        assert_eq!(parsed.reposts, 5);

        let rendered = serde_yaml_ng::to_string(&parsed).unwrap();
        let reparsed: TargetMetrics = serde_yaml_ng::from_str(&rendered).unwrap();
        assert_eq!(reparsed, parsed);
    }

    #[test]
    fn target_entry_with_metrics_round_trips() {
        let raw = concat!(
            "name: linkedin\n",
            "status: published\n",
            "url: https://www.linkedin.com/posts/x\n",
            "published_at: 2026-05-08T14:32:00Z\n",
            "metrics:\n",
            "  impressions: 1842\n",
            "  reactions: 67\n",
            "  comments: 14\n",
            "  reposts: 5\n",
            "  sampled_at: 2026-05-14T00:00:00Z\n",
        );
        let parsed: TargetEntry = serde_yaml_ng::from_str(raw).unwrap();
        let m = parsed.metrics.as_ref().expect("metrics present");
        assert_eq!(m.impressions, 1842);

        let rendered = serde_yaml_ng::to_string(&parsed).unwrap();
        let reparsed: TargetEntry = serde_yaml_ng::from_str(&rendered).unwrap();
        assert_eq!(reparsed, parsed);
    }

    #[test]
    fn target_entry_without_metrics_parses_unchanged() {
        // Pre-#433 frontmatter — no `metrics:` key on the target.
        let raw = "name: blog\nstatus: planned\n";
        let parsed: TargetEntry = serde_yaml_ng::from_str(raw).unwrap();
        assert!(parsed.metrics.is_none());
        // And no `samples:` key either — pre-#859 frontmatter.
        assert!(parsed.samples.is_empty());
    }

    #[test]
    fn metric_sample_serializes_date_as_iso_and_round_trips() {
        let sample = MetricSample {
            date: time::macros::date!(2026 - 05 - 22),
            impressions: Some(1234),
            engagements: Some(56),
        };
        let yaml = serde_yaml_ng::to_string(&sample).unwrap();
        assert!(yaml.contains("date: 2026-05-22"), "got: {yaml}");
        let reparsed: MetricSample = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(reparsed, sample);
    }

    #[test]
    fn metric_sample_omits_absent_metric() {
        let sample = MetricSample {
            date: time::macros::date!(2026 - 05 - 22),
            impressions: Some(10),
            engagements: None,
        };
        let yaml = serde_yaml_ng::to_string(&sample).unwrap();
        assert!(!yaml.contains("engagements"), "got: {yaml}");
    }

    #[test]
    fn record_sample_is_idempotent_on_date_and_keeps_sorted() {
        let mut entry = TargetEntry {
            name: Target::Linkedin,
            status: TargetStatus::Published,
            url: Some("https://www.linkedin.com/posts/x-7400000000000000001-Ab/".into()),
            published_at: None,
            metrics: None,
            samples: Vec::new(),
        };
        let sample = |day, imp| MetricSample {
            date: day,
            impressions: Some(imp),
            engagements: None,
        };
        assert!(entry.record_sample(sample(time::macros::date!(2026 - 05 - 02), 2)));
        assert!(entry.record_sample(sample(time::macros::date!(2026 - 05 - 01), 1)));
        // Re-recording an existing date is a no-op that keeps the first value.
        assert!(!entry.record_sample(sample(time::macros::date!(2026 - 05 - 01), 999)));

        let dates: Vec<_> = entry.samples.iter().map(|s| s.date).collect();
        assert_eq!(
            dates,
            vec![
                time::macros::date!(2026 - 05 - 01),
                time::macros::date!(2026 - 05 - 02),
            ],
        );
        assert_eq!(entry.samples[0].impressions, Some(1));
    }
}
