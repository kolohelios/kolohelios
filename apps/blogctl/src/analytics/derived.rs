#![forbid(unsafe_code)]

//! Derived metrics — values computed from raw per-target counts at
//! read time. None of these are persisted; the markdown frontmatter
//! is the source of truth for impressions/reactions/comments/reposts/
//! sampled_at, and every analytics command runs the post's raw
//! numbers through `DerivedMetrics::from_target` to get comparable
//! shapes.
//!
//! `now` is injected so tests stay deterministic.

use time::OffsetDateTime;

use crate::target::TargetEntry;

/// Comparable shapes over a single target's raw metrics. Every field
/// is computed from the target's `metrics` (when set) and the
/// injected `now`; nothing here reads from disk or talks to the
/// network.
///
/// All `Option` fields encode "this couldn't be computed from the
/// data available," not "this came out as zero." For example,
/// `engagement_rate` is `None` for a target with zero impressions
/// — we never divide by zero, never emit NaN or infinity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DerivedMetrics {
    /// `(reactions + comments + reposts) / impressions`. `None` when
    /// the target has no metrics yet OR when impressions is zero.
    pub engagement_rate: Option<f64>,
    /// `reactions + comments + reposts`. Defaults to 0 when the
    /// target has no metrics.
    pub interactions: u64,
    /// `sampled_at - published_at`, floor to days. `None` when
    /// either timestamp is missing.
    pub age_days_at_sample: Option<i64>,
    /// `now - published_at`, floor to days. `None` when
    /// `published_at` is missing.
    pub age_days_now: Option<i64>,
    /// `now - sampled_at`, floor to days. `None` when the target
    /// has no metrics. Negative values are possible if `sampled_at`
    /// is in the future relative to `now` — callers can clamp.
    pub staleness_days: Option<i64>,
}

impl DerivedMetrics {
    /// Compute every derived value for `target` at the moment `now`.
    /// Status is irrelevant — retracted targets still get their
    /// last-known engagement_rate computed from their saved metrics.
    pub fn from_target(target: &TargetEntry, now: OffsetDateTime) -> Self {
        let (interactions, engagement_rate, staleness_days) = match &target.metrics {
            Some(m) => {
                let interactions = m.reactions + m.comments + m.reposts;
                let engagement_rate = if m.impressions == 0 {
                    None
                } else {
                    Some(interactions as f64 / m.impressions as f64)
                };
                let staleness_days = Some((now - m.sampled_at).whole_days());
                (interactions, engagement_rate, staleness_days)
            }
            None => (0, None, None),
        };

        let age_days_at_sample = match (&target.metrics, &target.published_at) {
            (Some(m), Some(p)) => Some((m.sampled_at - *p).whole_days()),
            _ => None,
        };
        let age_days_now = target
            .published_at
            .as_ref()
            .map(|p| (now - *p).whole_days());

        Self {
            engagement_rate,
            interactions,
            age_days_at_sample,
            age_days_now,
            staleness_days,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    use crate::target::{Target, TargetEntry, TargetMetrics, TargetStatus};

    fn now() -> OffsetDateTime {
        datetime!(2026-05-17 00:00:00 UTC)
    }

    fn target_with_metrics(
        impressions: u64,
        reactions: u64,
        comments: u64,
        reposts: u64,
    ) -> TargetEntry {
        TargetEntry {
            name: Target::Linkedin,
            status: TargetStatus::Published,
            url: Some("https://example.invalid".into()),
            published_at: Some(datetime!(2026-05-01 00:00:00 UTC)),
            metrics: Some(TargetMetrics {
                impressions,
                reactions,
                comments,
                reposts,
                sampled_at: datetime!(2026-05-14 00:00:00 UTC),
            }),
        }
    }

    #[test]
    fn canonical_1000_impressions_50_interactions_yields_5_percent() {
        // The acceptance criterion's anchor case. 50 / 1000 = 0.05.
        let t = target_with_metrics(1000, 30, 15, 5);
        let d = DerivedMetrics::from_target(&t, now());
        assert_eq!(d.interactions, 50);
        // Use approximate equality — float math.
        let er = d.engagement_rate.expect("engagement_rate should be Some");
        assert!((er - 0.05).abs() < 1e-9, "got: {er}");
    }

    #[test]
    fn zero_impressions_yields_none_engagement_rate_no_div_by_zero() {
        let t = target_with_metrics(0, 5, 0, 0);
        let d = DerivedMetrics::from_target(&t, now());
        // Interactions still count — only the ratio is undefined.
        assert_eq!(d.interactions, 5);
        assert!(d.engagement_rate.is_none());
    }

    #[test]
    fn missing_metrics_yields_none_engagement_rate_and_zero_interactions() {
        let t = TargetEntry {
            name: Target::Linkedin,
            status: TargetStatus::Planned,
            url: None,
            published_at: None,
            metrics: None,
        };
        let d = DerivedMetrics::from_target(&t, now());
        assert_eq!(d.interactions, 0);
        assert!(d.engagement_rate.is_none());
        assert!(d.age_days_at_sample.is_none());
        assert!(d.age_days_now.is_none());
        assert!(d.staleness_days.is_none());
    }

    #[test]
    fn retracted_target_with_metrics_still_computes_engagement_rate() {
        // Retracted is a distribution-state thing; the last-known
        // numbers stay analytically meaningful.
        let mut t = target_with_metrics(2000, 80, 10, 10);
        t.status = TargetStatus::Retracted;
        let d = DerivedMetrics::from_target(&t, now());
        assert_eq!(d.interactions, 100);
        assert!(d.engagement_rate.is_some());
    }

    #[test]
    fn age_days_at_sample_uses_published_at_and_sampled_at() {
        // Published 2026-05-01, sampled 2026-05-14 → 13 days.
        let t = target_with_metrics(100, 1, 1, 1);
        let d = DerivedMetrics::from_target(&t, now());
        assert_eq!(d.age_days_at_sample, Some(13));
    }

    #[test]
    fn age_days_now_uses_published_at_and_injected_now() {
        // Published 2026-05-01, now 2026-05-17 → 16 days.
        let t = target_with_metrics(100, 1, 1, 1);
        let d = DerivedMetrics::from_target(&t, now());
        assert_eq!(d.age_days_now, Some(16));
    }

    #[test]
    fn age_days_now_is_none_when_published_at_missing() {
        let mut t = target_with_metrics(100, 1, 1, 1);
        t.published_at = None;
        let d = DerivedMetrics::from_target(&t, now());
        assert!(d.age_days_now.is_none());
        // age_days_at_sample also needs published_at — same fate.
        assert!(d.age_days_at_sample.is_none());
    }

    #[test]
    fn staleness_days_floors_to_days() {
        // Sampled 2026-05-14T00, now 2026-05-17T00 → 3 days.
        let t = target_with_metrics(100, 1, 1, 1);
        let d = DerivedMetrics::from_target(&t, now());
        assert_eq!(d.staleness_days, Some(3));
    }

    #[test]
    fn staleness_under_one_day_floors_to_zero() {
        // Sampled 2026-05-14T00:00:00, now 2026-05-14T23:59:59 →
        // 23h 59m 59s < 24h, must floor to 0.
        let mut t = target_with_metrics(100, 1, 1, 1);
        t.metrics.as_mut().unwrap().sampled_at = datetime!(2026-05-14 00:00:00 UTC);
        let now = datetime!(2026-05-14 23:59:59 UTC);
        let d = DerivedMetrics::from_target(&t, now);
        assert_eq!(d.staleness_days, Some(0));
    }

    #[test]
    fn staleness_can_be_negative_for_future_sample() {
        // Pathological but defensible: sampled in the future,
        // either through clock skew or a hand-edited timestamp.
        // The module passes the value through; callers may clamp.
        let mut t = target_with_metrics(100, 1, 1, 1);
        t.metrics.as_mut().unwrap().sampled_at = datetime!(2026-05-20 00:00:00 UTC);
        let d = DerivedMetrics::from_target(&t, now());
        // 2026-05-17 to 2026-05-20 = -3 (time::Duration::whole_days
        // truncates toward zero — -3.0 stays -3).
        assert_eq!(d.staleness_days, Some(-3));
    }
}
