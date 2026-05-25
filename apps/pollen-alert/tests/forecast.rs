//! Integration test for the Open-Meteo forecast provider. Runs the
//! parser against a real-shape captured response (see
//! `tests/fixtures/open-meteo/`); no network round-trip.
//!
//! The fixture is a Bainbridge Island overnight window — eight
//! consecutive hours from 7 PM to 2 AM local. Trends across the
//! fixture: humidity climbs into the 90s, wind drops below 2 mph,
//! sky clears, precip stays low. The middle hours score high under
//! the v1 rules (this is the input shape the scoring + alert
//! pipeline will see in production).

use std::path::PathBuf;

use async_trait::async_trait;
use chrono::{NaiveDate, NaiveTime};

use pollen_alert::forecast::{ForecastError, ForecastProvider, HttpFetcher, OpenMeteoProvider};
use pollen_alert::scoring::Sky;

struct FixtureFetcher {
    body: String,
}

#[async_trait(?Send)]
impl HttpFetcher for FixtureFetcher {
    async fn get(&self, _url: &str) -> Result<String, ForecastError> {
        Ok(self.body.clone())
    }
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/open-meteo")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

#[tokio::test]
async fn parses_bainbridge_overnight_fixture() {
    let provider = OpenMeteoProvider::new(FixtureFetcher {
        body: fixture("bainbridge-overnight.json"),
    });
    let hours = provider
        .fetch(47.625, -122.5, "America/Los_Angeles")
        .await
        .expect("fetch ok");

    assert_eq!(hours.len(), 8);

    let first = &hours[0];
    assert_eq!(
        first.local_time,
        NaiveDate::from_ymd_opt(2026, 4, 15)
            .unwrap()
            .and_time(NaiveTime::from_hms_opt(19, 0, 0).unwrap())
    );
    assert_eq!(first.humidity_pct, 78.0);
    assert_eq!(first.wind_mph, 5.5);
    assert_eq!(first.precip_prob_pct, 15.0);
    assert_eq!(first.sky, Some(Sky::MostlyCloudy));

    // 23:00 — the peak risky hour: humidity 92, wind 1.6, precip 7,
    // cloud cover 5 → Clear.
    let peak = &hours[4];
    assert_eq!(peak.humidity_pct, 92.0);
    assert_eq!(peak.wind_mph, 1.6);
    assert_eq!(peak.sky, Some(Sky::Clear));
}

#[tokio::test]
async fn fixture_drives_scoring_above_threshold() {
    use pollen_alert::alert::find_alert;
    use pollen_alert::scoring::score_hour;

    let provider = OpenMeteoProvider::new(FixtureFetcher {
        body: fixture("bainbridge-overnight.json"),
    });
    let hours = provider
        .fetch(47.625, -122.5, "America/Los_Angeles")
        .await
        .expect("fetch ok");

    // Drive the full pipeline: score each hour (April → tree season),
    // then ask `find_alert` for the first 5-point ≥ 3-hour window.
    let scored: Vec<_> = hours
        .iter()
        .cloned()
        .map(|h| {
            let s = score_hour(&h, true);
            (h, s)
        })
        .collect();
    let alert = find_alert(&scored, 5, 3).expect("alert");
    assert!(
        alert.score >= 5,
        "alert score {} below threshold",
        alert.score
    );
    // The risky run starts at 20:00 (humidity hits 82, wind drops to
    // 3.8, sky clears to PartlyCloudy) and continues through the
    // overnight peak.
    assert_eq!(
        alert.window_start,
        NaiveDate::from_ymd_opt(2026, 4, 15)
            .unwrap()
            .and_time(NaiveTime::from_hms_opt(20, 0, 0).unwrap())
    );
}
