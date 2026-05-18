//! Distribution targets and their per-target state. Orthogonal to the
//! editorial `Stage` pipeline: a post is editorially in one stage
//! (concept→…→published) but may end up distributed to zero, one, or
//! more venues, each with its own small state machine.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::{Error, Result};

/// A venue a post can be distributed to. Closed enum — adding a new
/// venue is a deliberate code change, not a frontmatter typo away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
            other => Err(Error::InvalidTarget(other.to_string())),
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
            other => Err(Error::InvalidTargetStatus(other.to_string())),
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
        assert!(matches!(err, Error::InvalidTarget(_)));
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
        assert!(matches!(err, Error::InvalidTargetStatus(_)));
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
    }
}
