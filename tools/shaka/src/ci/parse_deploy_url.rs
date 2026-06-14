use std::path::PathBuf;
use std::process;

use crate::term::{BOLD, RED, RESET};

/// Extract a just-deployed Worker's public URL from captured
/// `wrangler deploy` output.
///
/// Prefers the `*.workers.dev` URL wrangler prints for a route-free
/// Worker. When the Worker declares an explicit `custom_domain` route
/// wrangler disables the workers.dev URL (unless `workers_dev = true`)
/// and prints only `<host> (custom domain)` trigger lines — fall back to
/// the first of those, returning `https://<host>`. Returns `None` only
/// when neither shape is present, which signals a genuine wrangler output
/// format change rather than a valid custom-domain-only deploy (#867).
pub fn parse(output: &str) -> Option<String> {
    if let Some(url) = output
        .split_whitespace()
        .find(|t| t.starts_with("https://") && t.ends_with(".workers.dev"))
    {
        return Some(url.to_string());
    }
    output
        .lines()
        .find_map(custom_domain_host)
        .map(|host| format!("https://{host}"))
}

/// A custom-domain trigger line looks like `  buzzingo.app (custom domain)`.
/// Return the host (`buzzingo.app`) when the trimmed line ends with the
/// `(custom domain)` marker; `None` otherwise.
fn custom_domain_host(line: &str) -> Option<&str> {
    let host = line.trim().strip_suffix("(custom domain)")?;
    host.split_whitespace().next()
}

/// Read the captured wrangler output at `log` and print the deployed
/// Worker's URL to stdout. Exits non-zero (with a message on stderr for
/// the CI log) when the file can't be read or no URL of any kind is
/// present; the workflow turns that exit into a GitHub error annotation.
pub fn run(log: PathBuf) {
    let output = match std::fs::read_to_string(&log) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} reading {}: {e}", log.display());
            process::exit(1);
        }
    };
    match parse(&output) {
        Some(url) => println!("{url}"),
        None => {
            eprintln!(
                "{RED}{BOLD}error:{RESET} no *.workers.dev or custom-domain \
                 line in wrangler output (format change?)"
            );
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use std::fs;
    use std::path::Path;

    fn fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/parse-deploy-url")
            .join(name);
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    #[test]
    fn prefers_workers_dev_url() {
        assert_eq!(
            parse(&fixture("workers-dev.log")).as_deref(),
            Some("https://my-worker.jedwards.workers.dev"),
        );
    }

    #[test]
    fn falls_back_to_custom_domain() {
        assert_eq!(
            parse(&fixture("custom-domain.log")).as_deref(),
            Some("https://buzzingo.app"),
        );
    }

    #[test]
    fn prefers_workers_dev_when_both_present() {
        assert_eq!(
            parse(&fixture("both.log")).as_deref(),
            Some("https://buzzingo.jedwards.workers.dev"),
        );
    }

    #[test]
    fn none_when_neither_present() {
        assert_eq!(parse(&fixture("none.log")), None);
    }
}
