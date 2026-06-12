//! HTTP fetching for public LinkedIn post HTML, behind a trait.
//!
//! Abstracted (like [`crate::sync::Jj`]) so `blogctl linkedin import`
//! stays testable offline — the nix build sandbox has no network, and
//! tests must not hammer linkedin.com. Production uses [`UreqFetcher`];
//! tests inject canned responses via [`FakeFetcher`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::error::{Error, Result};

/// Fetch the HTML body at a URL.
pub trait Fetcher {
    fn get(&self, url: &str) -> Result<String>;
}

/// A browser User-Agent — LinkedIn serves the Open Graph / JSON-LD
/// markup to browsers, and returns a barer page to unknown clients.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

/// Polite delay between consecutive fetches.
const DEFAULT_DELAY: Duration = Duration::from_secs(4);

/// Real fetcher over `ureq`. Sends a browser User-Agent and paces
/// requests with a delay between calls (not before the first).
pub struct UreqFetcher {
    delay: Duration,
    started: AtomicBool,
}

impl UreqFetcher {
    pub fn new() -> Self {
        Self {
            delay: DEFAULT_DELAY,
            started: AtomicBool::new(false),
        }
    }
}

impl Default for UreqFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetcher for UreqFetcher {
    fn get(&self, url: &str) -> Result<String> {
        // Pace between fetches, but don't delay the first one.
        if self.started.swap(true, Ordering::Relaxed) {
            std::thread::sleep(self.delay);
        }
        ureq::get(url)
            .set("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| Error::LinkedinFetch {
                url: url.to_string(),
                message: e.to_string(),
            })?
            .into_string()
            .map_err(|e| Error::LinkedinFetch {
                url: url.to_string(),
                message: e.to_string(),
            })
    }
}

/// Test fetcher returning canned HTML per URL. Public (like
/// [`crate::sync::FakeJj`]) so integration tests in sibling files can
/// build a corpus of fixed responses.
#[derive(Default)]
pub struct FakeFetcher {
    responses: HashMap<String, String>,
}

impl FakeFetcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a canned response, builder-style.
    #[must_use]
    pub fn with(mut self, url: impl Into<String>, html: impl Into<String>) -> Self {
        self.responses.insert(url.into(), html.into());
        self
    }
}

impl Fetcher for FakeFetcher {
    fn get(&self, url: &str) -> Result<String> {
        self.responses
            .get(url)
            .cloned()
            .ok_or_else(|| Error::LinkedinFetch {
                url: url.to_string(),
                message: "no canned response registered (test fetcher)".to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ureq_fetcher_returns_body_over_http() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/post")
            .with_status(200)
            .with_body("<html>hi</html>")
            .create();
        // First call on a fresh fetcher does not sleep.
        let body = UreqFetcher::new()
            .get(&format!("{}/post", server.url()))
            .unwrap();
        assert_eq!(body, "<html>hi</html>");
        mock.assert();
    }

    #[test]
    fn ureq_fetcher_errors_on_http_error_status() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/missing").with_status(404).create();
        let err = UreqFetcher::new()
            .get(&format!("{}/missing", server.url()))
            .unwrap_err();
        assert!(matches!(err, Error::LinkedinFetch { .. }));
    }

    #[test]
    fn fake_fetcher_serves_canned_and_errors_on_unknown_url() {
        let fetcher = FakeFetcher::new().with("https://example/post", "<html>ok</html>");
        assert_eq!(
            fetcher.get("https://example/post").unwrap(),
            "<html>ok</html>"
        );
        assert!(matches!(
            fetcher.get("https://example/other").unwrap_err(),
            Error::LinkedinFetch { .. }
        ));
    }
}
