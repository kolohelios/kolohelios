//! Glue that turns a forecast into an alert decision.
//!
//! Kept out of `lib.rs` so the pipeline is unit-testable against
//! synthetic forecast hours without requiring the Worker runtime.
//! `lib.rs` reads env, constructs the trait objects, and calls
//! `decide` with what it gathered.

use crate::alert::{find_alert, Alert};
use crate::scoring::{in_tree_season, score_hour, ForecastHour};

/// Filter `hours` to the first contiguous overnight window
/// (19:00–07:00 local) and search for a qualifying alert. Returns
/// `None` if no window crosses threshold for `min_consecutive`
/// hours, or if the forecast contained no overnight hours at all.
pub fn decide(hours: &[ForecastHour], threshold: u8, min_consecutive: usize) -> Option<Alert> {
    let window = overnight_window(hours);
    if window.is_empty() {
        return None;
    }
    let season = window
        .first()
        .map(|h| in_tree_season(h.local_time.date()))
        .unwrap_or(false);
    let scored: Vec<_> = window
        .iter()
        .cloned()
        .map(|h| {
            let s = score_hour(&h, season);
            (h, s)
        })
        .collect();
    find_alert(&scored, threshold, min_consecutive)
}

/// First contiguous run of hours whose local time falls inside
/// the overnight window `[19:00, 07:00)`. The cron fires at 02:00
/// UTC (≈ 19:00 PDT / 18:00 PST), so the first qualifying hour in
/// Open-Meteo's response is the start of the user's evening.
fn overnight_window(hours: &[ForecastHour]) -> Vec<ForecastHour> {
    use chrono::Timelike;
    let mut started = false;
    let mut window = Vec::new();
    for h in hours {
        let hr = h.local_time.hour();
        let in_window = !(7..19).contains(&hr);
        if in_window {
            started = true;
            window.push(h.clone());
        } else if started {
            break;
        }
    }
    window
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::Sky;
    use chrono::{NaiveDate, NaiveTime};

    fn hr(date: (i32, u32, u32), hour: u32) -> ForecastHour {
        ForecastHour {
            local_time: NaiveDate::from_ymd_opt(date.0, date.1, date.2)
                .unwrap()
                .and_time(NaiveTime::from_hms_opt(hour, 0, 0).unwrap()),
            humidity_pct: 90.0,
            wind_mph: 2.0,
            precip_prob_pct: 10.0,
            sky: Some(Sky::Clear),
        }
    }

    fn safe(date: (i32, u32, u32), hour: u32) -> ForecastHour {
        ForecastHour {
            humidity_pct: 50.0,
            wind_mph: 15.0,
            precip_prob_pct: 80.0,
            sky: Some(Sky::Overcast),
            ..hr(date, hour)
        }
    }

    #[test]
    fn overnight_window_picks_19_to_06() {
        let hours = vec![
            hr((2026, 4, 15), 19),
            hr((2026, 4, 15), 20),
            hr((2026, 4, 15), 21),
            hr((2026, 4, 16), 6),
            hr((2026, 4, 16), 7),  // out
            hr((2026, 4, 16), 19), // next evening — stops at first break
        ];
        let w = overnight_window(&hours);
        assert_eq!(w.len(), 4);
        assert_eq!(w[0].local_time.format("%H").to_string(), "19");
        assert_eq!(w[3].local_time.format("%H").to_string(), "06");
    }

    #[test]
    fn overnight_window_skips_leading_daytime_hours() {
        let hours = vec![
            hr((2026, 4, 15), 14), // skipped
            hr((2026, 4, 15), 15), // skipped
            hr((2026, 4, 15), 19), // start
            hr((2026, 4, 15), 20),
        ];
        let w = overnight_window(&hours);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].local_time.format("%H").to_string(), "19");
    }

    #[test]
    fn decide_returns_alert_when_overnight_qualifies() {
        let hours = vec![
            hr((2026, 4, 15), 19),
            hr((2026, 4, 15), 20),
            hr((2026, 4, 15), 21),
            hr((2026, 4, 15), 22),
        ];
        let alert = decide(&hours, 5, 3).expect("alert");
        assert!(alert.score >= 5);
    }

    #[test]
    fn decide_returns_none_when_overnight_is_safe() {
        let hours = vec![
            safe((2026, 4, 15), 19),
            safe((2026, 4, 15), 20),
            safe((2026, 4, 15), 21),
            safe((2026, 4, 15), 22),
        ];
        assert!(decide(&hours, 5, 3).is_none());
    }

    #[test]
    fn decide_returns_none_when_no_overnight_hours() {
        // All daytime — no window to score.
        let hours = vec![
            hr((2026, 4, 15), 10),
            hr((2026, 4, 15), 11),
            hr((2026, 4, 15), 12),
        ];
        assert!(decide(&hours, 5, 3).is_none());
    }
}
