//! Forecast access for the scoring pipeline.
//!
//! `ForecastProvider` is the trait the worker entrypoint depends on;
//! `OpenMeteoProvider` is the only implementation today (Open-Meteo
//! is free + key-less + exposes the four fields scoring needs on an
//! hourly grid). HTTP is abstracted behind `HttpFetcher` so native
//! tests can replay fixture JSON without hitting the network.

use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde::Deserialize;

use crate::scoring::{ForecastHour, Sky};

#[derive(Debug)]
pub enum ForecastError {
    Http(String),
    Parse(String),
    /// The provider returned arrays whose lengths don't line up;
    /// usually a sign of a partial response.
    LengthMismatch {
        hours: usize,
        field: &'static str,
    },
}

impl std::fmt::Display for ForecastError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "forecast http: {e}"),
            Self::Parse(e) => write!(f, "forecast parse: {e}"),
            Self::LengthMismatch { hours, field } => {
                write!(f, "forecast: {field} has wrong length vs hours ({hours})")
            }
        }
    }
}

impl std::error::Error for ForecastError {}

/// Thin HTTP boundary the provider depends on. The worker impl wraps
/// `worker::Fetch`; native tests use a fixture-replaying fake so the
/// parser is exercised without a real network round-trip.
#[async_trait(?Send)]
pub trait HttpFetcher {
    async fn get(&self, url: &str) -> Result<String, ForecastError>;
}

#[async_trait(?Send)]
pub trait ForecastProvider {
    async fn fetch(&self, lat: f32, lon: f32, tz: &str)
        -> Result<Vec<ForecastHour>, ForecastError>;
}

pub struct OpenMeteoProvider<F: HttpFetcher> {
    pub fetcher: F,
}

impl<F: HttpFetcher> OpenMeteoProvider<F> {
    pub fn new(fetcher: F) -> Self {
        Self { fetcher }
    }
}

#[async_trait(?Send)]
impl<F: HttpFetcher> ForecastProvider for OpenMeteoProvider<F> {
    async fn fetch(
        &self,
        lat: f32,
        lon: f32,
        tz: &str,
    ) -> Result<Vec<ForecastHour>, ForecastError> {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?\
             latitude={lat}&longitude={lon}\
             &hourly=relative_humidity_2m,wind_speed_10m,\
             precipitation_probability,cloud_cover\
             &wind_speed_unit=mph&timezone={tz}"
        );
        let body = self.fetcher.get(&url).await?;
        parse_open_meteo(&body)
    }
}

/// Open-Meteo response shape (subset we care about). Hourly fields
/// come back as parallel arrays — `time[i]` lines up with
/// `relative_humidity_2m[i]` and so on. The parser walks the indices
/// and builds `ForecastHour` rows; mismatched lengths fail loudly
/// instead of producing garbage.
#[derive(Debug, Deserialize)]
struct OpenMeteoResponse {
    hourly: OpenMeteoHourly,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoHourly {
    // Open-Meteo returns times without seconds (`2026-04-15T22:00`),
    // which chrono's default `NaiveDateTime` deserializer rejects.
    // Capture as strings and parse with an explicit format below.
    time: Vec<String>,
    relative_humidity_2m: Vec<f32>,
    wind_speed_10m: Vec<f32>,
    precipitation_probability: Vec<f32>,
    cloud_cover: Vec<f32>,
}

const OPEN_METEO_TIME_FMT: &str = "%Y-%m-%dT%H:%M";

fn parse_open_meteo(body: &str) -> Result<Vec<ForecastHour>, ForecastError> {
    let resp: OpenMeteoResponse =
        serde_json::from_str(body).map_err(|e| ForecastError::Parse(e.to_string()))?;
    let h = &resp.hourly;
    let n = h.time.len();
    if h.relative_humidity_2m.len() != n {
        return Err(ForecastError::LengthMismatch {
            hours: n,
            field: "relative_humidity_2m",
        });
    }
    if h.wind_speed_10m.len() != n {
        return Err(ForecastError::LengthMismatch {
            hours: n,
            field: "wind_speed_10m",
        });
    }
    if h.precipitation_probability.len() != n {
        return Err(ForecastError::LengthMismatch {
            hours: n,
            field: "precipitation_probability",
        });
    }
    if h.cloud_cover.len() != n {
        return Err(ForecastError::LengthMismatch {
            hours: n,
            field: "cloud_cover",
        });
    }

    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let local_time = NaiveDateTime::parse_from_str(&h.time[i], OPEN_METEO_TIME_FMT)
            .map_err(|e| ForecastError::Parse(format!("time[{i}]: {e}")))?;
        out.push(ForecastHour {
            local_time,
            humidity_pct: h.relative_humidity_2m[i],
            wind_mph: h.wind_speed_10m[i],
            precip_prob_pct: h.precipitation_probability[i],
            sky: Some(cloud_cover_to_sky(h.cloud_cover[i])),
        });
    }
    Ok(out)
}

/// Map Open-Meteo's continuous cloud-cover percent into the categorical
/// `Sky` the scorer accepts. Boundaries are NWS-ish ("clear" through
/// 25%, "scattered" 26-50%, etc.); refine if false-positives in the
/// scoring side bite.
fn cloud_cover_to_sky(pct: f32) -> Sky {
    match pct as u8 {
        0..=25 => Sky::Clear,
        26..=50 => Sky::PartlyCloudy,
        51..=75 => Sky::MostlyCloudy,
        _ => Sky::Overcast,
    }
}

/// `worker::Fetch`-backed HTTP fetcher for the Cloudflare Worker
/// runtime. Compiled out on native targets so `cargo test` doesn't
/// need to depend on the wasm runtime.
#[cfg(target_arch = "wasm32")]
pub struct WorkerFetcher;

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl HttpFetcher for WorkerFetcher {
    async fn get(&self, url: &str) -> Result<String, ForecastError> {
        use worker::{Fetch, Method, Request, RequestInit};
        let req = Request::new_with_init(url, RequestInit::new().with_method(Method::Get))
            .map_err(|e| ForecastError::Http(e.to_string()))?;
        let mut resp = Fetch::Request(req)
            .send()
            .await
            .map_err(|e| ForecastError::Http(e.to_string()))?;
        resp.text()
            .await
            .map_err(|e| ForecastError::Http(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_cover_boundaries() {
        assert_eq!(cloud_cover_to_sky(0.0), Sky::Clear);
        assert_eq!(cloud_cover_to_sky(25.0), Sky::Clear);
        assert_eq!(cloud_cover_to_sky(26.0), Sky::PartlyCloudy);
        assert_eq!(cloud_cover_to_sky(50.0), Sky::PartlyCloudy);
        assert_eq!(cloud_cover_to_sky(51.0), Sky::MostlyCloudy);
        assert_eq!(cloud_cover_to_sky(75.0), Sky::MostlyCloudy);
        assert_eq!(cloud_cover_to_sky(76.0), Sky::Overcast);
        assert_eq!(cloud_cover_to_sky(100.0), Sky::Overcast);
    }

    #[test]
    fn parser_rejects_mismatched_array_lengths() {
        let body = r#"{
            "hourly": {
                "time": ["2026-04-15T22:00"],
                "relative_humidity_2m": [80.0, 85.0],
                "wind_speed_10m": [2.0],
                "precipitation_probability": [10.0],
                "cloud_cover": [20.0]
            }
        }"#;
        let err = parse_open_meteo(body).expect_err("length mismatch");
        match err {
            ForecastError::LengthMismatch { field, .. } => {
                assert_eq!(field, "relative_humidity_2m");
            }
            other => panic!("expected LengthMismatch, got {other:?}"),
        }
    }

    #[test]
    fn parser_rejects_malformed_json() {
        let err = parse_open_meteo("not json").expect_err("parse error");
        assert!(matches!(err, ForecastError::Parse(_)));
    }
}
