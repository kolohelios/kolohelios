//! Find the first overnight window where at least `min_consecutive`
//! consecutive hours score at or above `threshold`. Returns at most one
//! alert per call — the first qualifying run. The entrypoint (#409)
//! filters hours to the overnight window before calling this; the
//! function itself takes the slice as-is.
//!
//! `summary` translates the static reason strings from `scoring` into
//! short prose suitable for a notification body. Reasons are unioned
//! across the run, deduplicated, and ordered by how many hours in the
//! window contributed each one (most-common first).

use chrono::NaiveDateTime;

use crate::scoring::{ForecastHour, HourScore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    /// Highest score observed within the qualifying window.
    pub score: u8,
    pub window_start: NaiveDateTime,
    pub window_end: NaiveDateTime,
    /// Reasons that fired in the window, deduplicated and ordered by
    /// total contribution (most common first; ties broken by the
    /// original `scoring::score_hour` insertion order via stable
    /// sort).
    pub reasons: Vec<&'static str>,
    /// Short human string for the notification body.
    pub summary: String,
}

pub fn find_alert(
    hours: &[(ForecastHour, HourScore)],
    threshold: u8,
    min_consecutive: usize,
) -> Option<Alert> {
    if min_consecutive == 0 || hours.is_empty() {
        return None;
    }

    let mut run_start: Option<usize> = None;
    for (i, (_h, score)) in hours.iter().enumerate() {
        if score.points >= threshold {
            run_start.get_or_insert(i);
            let run_len = i - run_start.unwrap() + 1;
            if run_len >= min_consecutive {
                // Extend the run to its full length — the qualifying
                // window may continue past the min_consecutive mark.
                let mut end = i;
                for (j, (_, s)) in hours.iter().enumerate().skip(i + 1) {
                    if s.points >= threshold {
                        end = j;
                    } else {
                        break;
                    }
                }
                return Some(build_alert(&hours[run_start.unwrap()..=end]));
            }
        } else {
            run_start = None;
        }
    }
    None
}

fn build_alert(run: &[(ForecastHour, HourScore)]) -> Alert {
    let score = run.iter().map(|(_, s)| s.points).max().unwrap_or(0);
    let window_start = run.first().unwrap().0.local_time;
    let window_end = run.last().unwrap().0.local_time;

    // Tally reason counts in first-seen order so ties break by the
    // original `score_hour` insertion order.
    let mut order: Vec<&'static str> = Vec::new();
    let mut counts: Vec<(usize, usize)> = Vec::new();
    for (_, s) in run {
        for r in &s.reasons {
            match order.iter().position(|x| x == r) {
                Some(idx) => counts[idx].1 += 1,
                None => {
                    counts.push((order.len(), 1));
                    order.push(r);
                }
            }
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let reasons: Vec<&'static str> = counts.iter().map(|(i, _)| order[*i]).collect();

    let summary = reasons
        .iter()
        .map(|r| reason_to_prose(r))
        .collect::<Vec<_>>()
        .join(", ");

    Alert {
        score,
        window_start,
        window_end,
        reasons,
        summary,
    }
}

/// Static mapping from the reason strings `scoring::score_hour` emits
/// into prose suitable for a notification body. Keeping it in this
/// module (not on `HourScore`) keeps `scoring` purely about the
/// numeric rule and `alert` owning the user-facing rendering.
///
/// Falls back to the input string for any reason this module hasn't
/// learned to translate — a defensive default so a new scoring rule
/// doesn't drop its reason from the notification entirely while this
/// module catches up.
fn reason_to_prose(reason: &str) -> &str {
    match reason {
        "humidity ≥ 80%" => "high humidity",
        "wind ≤ 4 mph" => "calm wind",
        "precip prob < 30%" => "little rain",
        "sky clear / partly cloudy" => "clear sky",
        "tree pollen season" => "tree pollen season",
        _ => reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::{score_hour, Sky};
    use chrono::{NaiveDate, NaiveTime};

    fn at(hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 4, 15)
            .unwrap()
            .and_time(NaiveTime::from_hms_opt(hour, 0, 0).unwrap())
    }

    fn risky(hour: u32) -> (ForecastHour, HourScore) {
        let f = ForecastHour {
            local_time: at(hour),
            humidity_pct: 90.0,
            wind_mph: 2.0,
            precip_prob_pct: 10.0,
            sky: Some(Sky::Clear),
        };
        let s = score_hour(&f, true);
        assert!(s.points >= 5, "risky hour fixture failed to score: {s:?}");
        (f, s)
    }

    fn safe(hour: u32) -> (ForecastHour, HourScore) {
        let f = ForecastHour {
            local_time: at(hour),
            humidity_pct: 50.0,
            wind_mph: 15.0,
            precip_prob_pct: 80.0,
            sky: Some(Sky::Overcast),
        };
        let s = score_hour(&f, false);
        (f, s)
    }

    #[test]
    fn empty_hours_returns_none() {
        assert!(find_alert(&[], 5, 3).is_none());
    }

    #[test]
    fn zero_min_consecutive_returns_none() {
        // Guard against an empty-window "alert" — `0` is never a real
        // ask but we shouldn't pretend the first hour is an alert
        // either.
        assert!(find_alert(&[risky(22)], 5, 0).is_none());
    }

    #[test]
    fn no_risky_hours_returns_none() {
        let hours: Vec<_> = (19..24).map(safe).collect();
        assert!(find_alert(&hours, 5, 3).is_none());
    }

    #[test]
    fn exact_min_consecutive_run_returns_alert() {
        let hours = vec![safe(19), risky(20), risky(21), risky(22), safe(23)];
        let alert = find_alert(&hours, 5, 3).expect("alert");
        assert_eq!(alert.window_start, at(20));
        assert_eq!(alert.window_end, at(22));
    }

    #[test]
    fn longer_run_extends_window_end() {
        let hours = vec![
            safe(19),
            risky(20),
            risky(21),
            risky(22),
            risky(23),
            risky(0),
            safe(1),
        ];
        let alert = find_alert(&hours, 5, 3).expect("alert");
        assert_eq!(alert.window_start, at(20));
        assert_eq!(alert.window_end, at(0));
    }

    #[test]
    fn run_shorter_than_min_consecutive_returns_none() {
        let hours = vec![risky(20), risky(21), safe(22), risky(23)];
        assert!(find_alert(&hours, 5, 3).is_none());
    }

    #[test]
    fn returns_first_qualifying_run_not_later_ones() {
        let hours = vec![
            risky(19),
            risky(20),
            risky(21),
            safe(22),
            risky(23),
            risky(0),
            risky(1),
        ];
        let alert = find_alert(&hours, 5, 3).expect("alert");
        assert_eq!(alert.window_start, at(19));
        assert_eq!(alert.window_end, at(21));
    }

    #[test]
    fn alert_score_is_max_in_window() {
        // First hour scores 7 (all rules), second scores 5 (drops sky
        // by being overcast). max should be 7.
        let f_max = ForecastHour {
            local_time: at(20),
            humidity_pct: 95.0,
            wind_mph: 2.0,
            precip_prob_pct: 10.0,
            sky: Some(Sky::Clear),
        };
        let f_low = ForecastHour {
            sky: Some(Sky::Overcast),
            local_time: at(21),
            ..f_max.clone()
        };
        let f_low2 = ForecastHour {
            local_time: at(22),
            ..f_low.clone()
        };
        let hours = vec![
            (f_max.clone(), score_hour(&f_max, true)),
            (f_low.clone(), score_hour(&f_low, true)),
            (f_low2.clone(), score_hour(&f_low2, true)),
        ];
        let alert = find_alert(&hours, 5, 3).expect("alert");
        assert_eq!(alert.score, 7);
    }

    #[test]
    fn reasons_deduplicated_and_ordered_by_contribution() {
        // Pass in_tree_season=true everywhere so each hour scores ≥ 5.
        // First hour scores humidity + wind + tree (3 reasons; sky
        // overcast doesn't fire). Second & third add sky.
        //
        // Counts across the run: humidity (3), wind (3), tree (3),
        // sky (2). The three-way tie breaks by `score_hour`'s
        // insertion order (humidity, wind, tree), then sky last on
        // count.
        let f_a = ForecastHour {
            local_time: at(20),
            humidity_pct: 90.0,
            wind_mph: 2.0,
            precip_prob_pct: 50.0,
            sky: Some(Sky::Overcast),
        };
        let f_b = ForecastHour {
            sky: Some(Sky::Clear),
            local_time: at(21),
            ..f_a.clone()
        };
        let f_c = ForecastHour {
            local_time: at(22),
            ..f_b.clone()
        };
        let hours = vec![
            (f_a.clone(), score_hour(&f_a, true)),
            (f_b.clone(), score_hour(&f_b, true)),
            (f_c.clone(), score_hour(&f_c, true)),
        ];
        let alert = find_alert(&hours, 5, 3).expect("alert");
        assert_eq!(
            alert.reasons,
            vec![
                "humidity ≥ 80%",
                "wind ≤ 4 mph",
                "tree pollen season",
                "sky clear / partly cloudy",
            ]
        );
    }

    #[test]
    fn summary_is_short_prose() {
        let hours: Vec<_> = (20..23).map(risky).collect();
        let alert = find_alert(&hours, 5, 3).expect("alert");
        assert_eq!(
            alert.summary,
            "high humidity, calm wind, little rain, clear sky, tree pollen season"
        );
    }

    #[test]
    fn threshold_edge_exactly_at_threshold_qualifies() {
        // Score = threshold should count.
        let f = ForecastHour {
            local_time: at(20),
            humidity_pct: 90.0,
            wind_mph: 2.0,
            precip_prob_pct: 50.0,
            sky: Some(Sky::Overcast),
        };
        let s = score_hour(&f, false); // humidity + wind = 4
        assert_eq!(s.points, 4);
        let hours = vec![
            (f.clone(), s.clone()),
            (f.clone(), s.clone()),
            (f.clone(), s.clone()),
        ];
        assert!(find_alert(&hours, 4, 3).is_some());
        assert!(find_alert(&hours, 5, 3).is_none());
    }
}
