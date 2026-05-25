//! Notification delivery for crossed-threshold alerts.
//!
//! `Notifier` is the trait the entrypoint depends on so the
//! delivery target (Pushover, log-only dry-run, future channels) is
//! pluggable. Body formatting lives in `render_body` — testable
//! standalone without a network round-trip.

use async_trait::async_trait;
use serde::Serialize;

use crate::alert::Alert;

#[derive(Debug)]
pub enum NotifyError {
    Http(String),
    /// Pushover returned a 2xx status but a body the API treats as an
    /// error (token/user rejected, message too long, etc.). The body
    /// is captured so the caller can log enough to debug.
    Rejected(String),
    Serialize(String),
}

impl std::fmt::Display for NotifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "notify http: {e}"),
            Self::Rejected(e) => write!(f, "notify rejected: {e}"),
            Self::Serialize(e) => write!(f, "notify serialize: {e}"),
        }
    }
}

impl std::error::Error for NotifyError {}

#[async_trait(?Send)]
pub trait Notifier {
    async fn send(&self, alert: &Alert) -> Result<(), NotifyError>;
}

/// Thin POST boundary the Pushover notifier depends on. Mirror of
/// `forecast::HttpFetcher` but for `application/x-www-form-urlencoded`
/// POSTs — a separate trait so the two stay independently mockable
/// (a single shared HTTP trait would force every test to handle both
/// shapes).
#[async_trait(?Send)]
pub trait HttpPoster {
    async fn post_form(&self, url: &str, body: &str) -> Result<String, NotifyError>;
}

/// Pushover Messages API: `POST https://api.pushover.net/1/messages.json`
/// with form-encoded `token`/`user`/`message`. Returns a JSON body
/// with `status: 1` on success; anything else is a rejection.
pub struct PushoverNotifier<P: HttpPoster> {
    pub app_token: String,
    pub user_key: String,
    pub http: P,
}

const PUSHOVER_URL: &str = "https://api.pushover.net/1/messages.json";

#[derive(Serialize)]
struct PushoverForm<'a> {
    token: &'a str,
    user: &'a str,
    message: String,
}

#[async_trait(?Send)]
impl<P: HttpPoster> Notifier for PushoverNotifier<P> {
    async fn send(&self, alert: &Alert) -> Result<(), NotifyError> {
        let form = PushoverForm {
            token: &self.app_token,
            user: &self.user_key,
            message: render_body(alert),
        };
        let body = serde_urlencoded::to_string(&form)
            .map_err(|e| NotifyError::Serialize(e.to_string()))?;
        let resp = self.http.post_form(PUSHOVER_URL, &body).await?;
        // Pushover wraps success/failure in JSON: `{"status": 1, ...}`
        // for success, anything else (or HTTP error) is a rejection.
        let parsed: serde_json::Value =
            serde_json::from_str(&resp).map_err(|e| NotifyError::Rejected(e.to_string()))?;
        if parsed["status"].as_i64() == Some(1) {
            Ok(())
        } else {
            Err(NotifyError::Rejected(resp))
        }
    }
}

/// Dry-run notifier used when `DRY_RUN=true` is set in the Worker
/// env. Logs the formatted body via `worker::console_log!` on the
/// Worker target and `println!` on native — same body the production
/// path would post.
pub struct LogNotifier;

#[async_trait(?Send)]
impl Notifier for LogNotifier {
    async fn send(&self, alert: &Alert) -> Result<(), NotifyError> {
        let body = render_body(alert);
        log_dry_run(&body);
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
fn log_dry_run(body: &str) {
    worker::console_log!("DRY_RUN notify:\n{body}");
}

#[cfg(not(target_arch = "wasm32"))]
fn log_dry_run(body: &str) {
    println!("DRY_RUN notify:\n{body}");
}

/// Format an alert's body for delivery. Pulled out of `Notifier`
/// impls so unit tests can pin the exact string without exercising
/// either HTTP or the dry-run logger.
pub fn render_body(alert: &Alert) -> String {
    format!(
        "Close the windows tonight (score {}).\n\n\
         Risk window: {start} – {end} local.\n\
         Reasons: {reasons}.",
        alert.score,
        start = alert.window_start.format("%a %b %e %H:%M"),
        end = alert.window_end.format("%H:%M"),
        reasons = alert.summary,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    fn at(hour: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 4, 15)
            .unwrap()
            .and_time(NaiveTime::from_hms_opt(hour, 0, 0).unwrap())
    }

    fn alert() -> Alert {
        Alert {
            score: 7,
            window_start: at(20),
            window_end: at(23),
            reasons: vec!["humidity ≥ 80%", "wind ≤ 4 mph"],
            summary: "high humidity, calm wind".to_string(),
        }
    }

    #[test]
    fn render_body_includes_score_and_summary() {
        let body = render_body(&alert());
        assert!(body.starts_with("Close the windows tonight (score 7)."));
        assert!(body.contains("Reasons: high humidity, calm wind."));
    }

    #[test]
    fn render_body_includes_window_times() {
        let body = render_body(&alert());
        assert!(
            body.contains("Risk window:"),
            "body missing window line: {body}"
        );
        // Start formats as full date+time, end as time-only.
        assert!(body.contains("20:00"));
        assert!(body.contains("23:00"));
    }

    #[test]
    fn pushover_form_encodes_secrets_and_message() {
        let form = PushoverForm {
            token: "app-token-value",
            user: "user-key-value",
            message: "Close the windows tonight (score 7).\nReasons: high humidity.".to_string(),
        };
        let encoded = serde_urlencoded::to_string(&form).expect("encode");
        assert!(encoded.contains("token=app-token-value"));
        assert!(encoded.contains("user=user-key-value"));
        // Spaces and newlines must percent-encode for `application/
        // x-www-form-urlencoded`; `%20` for space, `%0A` for newline.
        assert!(encoded.contains("Close+the+windows") || encoded.contains("Close%20the%20windows"));
        assert!(encoded.contains("%0A"));
    }

    struct FakePoster {
        response: String,
        captured: std::cell::RefCell<Option<(String, String)>>,
    }

    #[async_trait(?Send)]
    impl HttpPoster for FakePoster {
        async fn post_form(&self, url: &str, body: &str) -> Result<String, NotifyError> {
            *self.captured.borrow_mut() = Some((url.to_string(), body.to_string()));
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn pushover_success_status_one_is_ok() {
        let poster = FakePoster {
            response: r#"{"status":1,"request":"abc"}"#.to_string(),
            captured: std::cell::RefCell::new(None),
        };
        let n = PushoverNotifier {
            app_token: "t".into(),
            user_key: "u".into(),
            http: poster,
        };
        n.send(&alert()).await.expect("send ok");
    }

    #[tokio::test]
    async fn pushover_non_one_status_is_rejected() {
        let n = PushoverNotifier {
            app_token: "t".into(),
            user_key: "u".into(),
            http: FakePoster {
                response: r#"{"status":0,"errors":["application token invalid"]}"#.to_string(),
                captured: std::cell::RefCell::new(None),
            },
        };
        let err = n.send(&alert()).await.expect_err("rejected");
        assert!(matches!(err, NotifyError::Rejected(_)));
    }

    #[tokio::test]
    async fn log_notifier_always_succeeds() {
        LogNotifier.send(&alert()).await.expect("log ok");
    }
}
