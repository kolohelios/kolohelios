// Non-test code must not `.unwrap()`; `not(test)` exempts unit tests,
// and integration tests compile as separate crates (no attribute).
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod alert;
pub mod forecast;
pub mod notify;
pub mod pipeline;
pub mod scoring;

// Worker runtime — scheduled cron + fetch handler, env reading,
// HTTP adapters. Cfg-gated to `wasm32` because all of this depends
// on `worker::Fetch` / `worker::Env`, which only exist on the
// Cloudflare Worker target. Native `cargo test` exercises the
// pure modules above through their own unit tests.
#[cfg(target_arch = "wasm32")]
mod worker_runtime {
    use async_trait::async_trait;
    use worker::{event, Context, Env, Request, Response, Result, ScheduleContext, ScheduledEvent};

    use crate::forecast::{ForecastError, ForecastProvider, OpenMeteoProvider};
    use crate::notify::{HttpPoster, LogNotifier, Notifier, NotifyError, PushoverNotifier};
    use crate::pipeline::decide;

    /// Cron entrypoint. Reads env, fetches the forecast for the
    /// configured location, finds the overnight alert window, and
    /// dispatches to the notifier. Errors are logged (so a single
    /// bad run shows up in `wrangler tail`) and swallowed — the next
    /// cron retries on its own schedule.
    #[event(scheduled)]
    pub async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
        if let Err(e) = run(&env).await {
            worker::console_error!("pollen-alert run failed: {e}");
        }
    }

    #[event(fetch)]
    pub async fn fetch(_req: Request, _env: Env, _ctx: Context) -> Result<Response> {
        Response::error("Not Found", 404)
    }

    #[derive(Debug)]
    enum RunError {
        Env(String),
        Forecast(ForecastError),
        Notify(NotifyError),
    }

    impl std::fmt::Display for RunError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Env(e) => write!(f, "env: {e}"),
                Self::Forecast(e) => write!(f, "{e}"),
                Self::Notify(e) => write!(f, "{e}"),
            }
        }
    }

    impl From<ForecastError> for RunError {
        fn from(e: ForecastError) -> Self {
            Self::Forecast(e)
        }
    }

    impl From<NotifyError> for RunError {
        fn from(e: NotifyError) -> Self {
            Self::Notify(e)
        }
    }

    async fn run(env: &Env) -> std::result::Result<(), RunError> {
        let cfg = Config::from_env(env)?;
        let provider = OpenMeteoProvider::new(crate::forecast::WorkerFetcher);
        let hours = provider.fetch(cfg.lat, cfg.lon, &cfg.tz).await?;

        match decide(&hours, cfg.threshold, cfg.min_consecutive) {
            Some(alert) => {
                worker::console_log!(
                    "pollen-alert: window {start} – {end} (score {score})",
                    start = alert.window_start,
                    end = alert.window_end,
                    score = alert.score,
                );
                if cfg.dry_run {
                    LogNotifier.send(&alert).await?;
                } else {
                    let notifier = PushoverNotifier {
                        app_token: cfg.pushover_app_token.unwrap_or_default(),
                        user_key: cfg.pushover_user_key.unwrap_or_default(),
                        http: WorkerPoster,
                    };
                    notifier.send(&alert).await?;
                }
            }
            None => {
                worker::console_log!("pollen-alert: no overnight window crossed threshold");
            }
        }
        Ok(())
    }

    struct Config {
        lat: f32,
        lon: f32,
        tz: String,
        threshold: u8,
        min_consecutive: usize,
        dry_run: bool,
        pushover_app_token: Option<String>,
        pushover_user_key: Option<String>,
    }

    impl Config {
        fn from_env(env: &Env) -> std::result::Result<Self, RunError> {
            let lat = parse_var(env, "LAT")?;
            let lon = parse_var(env, "LON")?;
            let tz = var(env, "TZ")?;
            let threshold = parse_var(env, "THRESHOLD")?;
            let min_consecutive = parse_var(env, "MIN_CONSECUTIVE_HOURS")?;
            let dry_run = var(env, "DRY_RUN")?.eq_ignore_ascii_case("true");
            // Secrets are optional in dry-run mode so a staging deploy
            // can come up before `wrangler secret put` runs. In live
            // mode `run` reaches for them and a missing value surfaces
            // as a Pushover `Rejected` from an empty token.
            let pushover_app_token = env.secret("PUSHOVER_APP_TOKEN").ok().map(|s| s.to_string());
            let pushover_user_key = env.secret("PUSHOVER_USER_KEY").ok().map(|s| s.to_string());
            Ok(Self {
                lat,
                lon,
                tz,
                threshold,
                min_consecutive,
                dry_run,
                pushover_app_token,
                pushover_user_key,
            })
        }
    }

    fn var(env: &Env, key: &str) -> std::result::Result<String, RunError> {
        env.var(key)
            .map(|v| v.to_string())
            .map_err(|e| RunError::Env(format!("{key}: {e}")))
    }

    fn parse_var<T: std::str::FromStr>(env: &Env, key: &str) -> std::result::Result<T, RunError>
    where
        T::Err: std::fmt::Display,
    {
        var(env, key)?
            .parse::<T>()
            .map_err(|e| RunError::Env(format!("{key}: {e}")))
    }

    /// `worker::Fetch`-backed form-encoded POST for the Pushover
    /// notifier. The GET adapter for the forecast lives in
    /// `forecast::WorkerFetcher` (also wasm-only); separate adapters
    /// keep each trait independently mockable.
    struct WorkerPoster;

    #[async_trait(?Send)]
    impl HttpPoster for WorkerPoster {
        async fn post_form(
            &self,
            url: &str,
            body: &str,
        ) -> std::result::Result<String, NotifyError> {
            use worker::{wasm_bindgen::JsValue, Fetch, Headers, Method, Request, RequestInit};
            let headers = Headers::new();
            headers
                .set("content-type", "application/x-www-form-urlencoded")
                .map_err(|e| NotifyError::Http(e.to_string()))?;
            let req = Request::new_with_init(
                url,
                RequestInit::new()
                    .with_method(Method::Post)
                    .with_headers(headers)
                    .with_body(Some(JsValue::from_str(body))),
            )
            .map_err(|e| NotifyError::Http(e.to_string()))?;
            let mut resp = Fetch::Request(req)
                .send()
                .await
                .map_err(|e| NotifyError::Http(e.to_string()))?;
            resp.text()
                .await
                .map_err(|e| NotifyError::Http(e.to_string()))
        }
    }
}
