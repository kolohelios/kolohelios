//! Per-hour overnight pollen-risk scoring.
//!
//! Pure function with no I/O. The forecast provider (#407) and the
//! window-detection aggregator (#406) feed this module; the worker
//! entrypoint (#409) glues them together. Score → action mapping
//! happens at the aggregator: ≥ 5 points for ≥ 3 consecutive hours
//! crosses threshold.

use chrono::{Datelike, NaiveDate, NaiveDateTime};

/// One hour's forecast. `sky` is optional because not every provider
/// surfaces a categorical sky condition; the rule that depends on it
/// is simply skipped when absent rather than zeroing out a real
/// signal.
#[derive(Debug, Clone)]
pub struct ForecastHour {
    pub local_time: NaiveDateTime,
    pub humidity_pct: f32,
    pub wind_mph: f32,
    pub precip_prob_pct: f32,
    pub sky: Option<Sky>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sky {
    Clear,
    PartlyCloudy,
    MostlyCloudy,
    Overcast,
}

/// Score with the reasons each point fired, for explainability in
/// notifications (a 5-point trigger gets the user a list of which
/// conditions actually crossed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HourScore {
    pub points: u8,
    pub reasons: Vec<&'static str>,
}

/// Western Washington tree pollen runs March through May (v1
/// approximation; refine with real cell-data if false-positives bite).
pub fn in_tree_season(date: NaiveDate) -> bool {
    matches!(date.month(), 3..=5)
}

/// Apply the v1 scoring rules to a single hour. Caller supplies
/// `in_tree_season` (precomputed once per run rather than per-hour) so
/// the function stays a pure aggregator over the forecast row.
pub fn score_hour(hour: &ForecastHour, in_tree_season: bool) -> HourScore {
    let mut score = HourScore {
        points: 0,
        reasons: Vec::new(),
    };

    if hour.humidity_pct >= 80.0 {
        score.points += 2;
        score.reasons.push("humidity ≥ 80%");
    }
    if hour.wind_mph <= 4.0 {
        score.points += 2;
        score.reasons.push("wind ≤ 4 mph");
    }
    if hour.precip_prob_pct < 30.0 {
        score.points += 1;
        score.reasons.push("precip prob < 30%");
    }
    if matches!(hour.sky, Some(Sky::Clear | Sky::PartlyCloudy)) {
        score.points += 1;
        score.reasons.push("sky clear / partly cloudy");
    }
    if in_tree_season {
        score.points += 1;
        score.reasons.push("tree pollen season");
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    fn hour() -> ForecastHour {
        ForecastHour {
            local_time: NaiveDate::from_ymd_opt(2026, 4, 15)
                .unwrap()
                .and_time(NaiveTime::from_hms_opt(22, 0, 0).unwrap()),
            humidity_pct: 50.0,
            wind_mph: 10.0,
            precip_prob_pct: 50.0,
            sky: Some(Sky::Overcast),
        }
    }

    #[test]
    fn humidity_at_threshold_fires() {
        let h = ForecastHour {
            humidity_pct: 80.0,
            ..hour()
        };
        let s = score_hour(&h, false);
        assert_eq!(s.points, 2);
        assert!(s.reasons.contains(&"humidity ≥ 80%"));
    }

    #[test]
    fn humidity_below_threshold_does_not_fire() {
        let h = ForecastHour {
            humidity_pct: 79.9,
            ..hour()
        };
        assert_eq!(score_hour(&h, false).points, 0);
    }

    #[test]
    fn wind_at_threshold_fires() {
        let h = ForecastHour {
            wind_mph: 4.0,
            ..hour()
        };
        let s = score_hour(&h, false);
        assert_eq!(s.points, 2);
        assert!(s.reasons.contains(&"wind ≤ 4 mph"));
    }

    #[test]
    fn wind_above_threshold_does_not_fire() {
        let h = ForecastHour {
            wind_mph: 4.1,
            ..hour()
        };
        assert_eq!(score_hour(&h, false).points, 0);
    }

    #[test]
    fn precip_just_below_threshold_fires() {
        let h = ForecastHour {
            precip_prob_pct: 29.9,
            ..hour()
        };
        let s = score_hour(&h, false);
        assert_eq!(s.points, 1);
        assert!(s.reasons.contains(&"precip prob < 30%"));
    }

    #[test]
    fn precip_at_threshold_does_not_fire() {
        // Strict `<`: 30% is right on the cusp; treat as "could rain
        // enough to wash out", don't credit it.
        let h = ForecastHour {
            precip_prob_pct: 30.0,
            ..hour()
        };
        assert_eq!(score_hour(&h, false).points, 0);
    }

    #[test]
    fn clear_sky_fires() {
        let h = ForecastHour {
            sky: Some(Sky::Clear),
            ..hour()
        };
        assert_eq!(score_hour(&h, false).points, 1);
    }

    #[test]
    fn partly_cloudy_sky_fires() {
        let h = ForecastHour {
            sky: Some(Sky::PartlyCloudy),
            ..hour()
        };
        assert_eq!(score_hour(&h, false).points, 1);
    }

    #[test]
    fn mostly_cloudy_sky_does_not_fire() {
        let h = ForecastHour {
            sky: Some(Sky::MostlyCloudy),
            ..hour()
        };
        assert_eq!(score_hour(&h, false).points, 0);
    }

    #[test]
    fn absent_sky_does_not_fire() {
        let h = ForecastHour {
            sky: None,
            ..hour()
        };
        assert_eq!(score_hour(&h, false).points, 0);
    }

    #[test]
    fn tree_season_fires_when_passed_true() {
        assert_eq!(score_hour(&hour(), true).points, 1);
    }

    #[test]
    fn all_rules_fire_together() {
        let h = ForecastHour {
            humidity_pct: 95.0,
            wind_mph: 2.0,
            precip_prob_pct: 10.0,
            sky: Some(Sky::Clear),
            ..hour()
        };
        let s = score_hour(&h, true);
        assert_eq!(s.points, 7);
        assert_eq!(s.reasons.len(), 5);
    }

    #[test]
    fn in_tree_season_march_starts_in() {
        assert!(in_tree_season(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
    }

    #[test]
    fn in_tree_season_may_ends_in() {
        assert!(in_tree_season(
            NaiveDate::from_ymd_opt(2026, 5, 31).unwrap()
        ));
    }

    #[test]
    fn in_tree_season_february_excluded() {
        assert!(!in_tree_season(
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()
        ));
    }

    #[test]
    fn in_tree_season_june_excluded() {
        assert!(!in_tree_season(
            NaiveDate::from_ymd_opt(2026, 6, 1).unwrap()
        ));
    }
}
