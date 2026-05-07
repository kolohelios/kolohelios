//! S3-compatible operations against the kolohelios bucket.
//!
//! Linode Object Storage speaks the AWS S3 API. Rather than vendor a SigV4
//! signer, we shell out to `awscli2` (in shaka's devshell) with
//! `--endpoint-url` pointing at the Linode cluster. Operations
//! skip-gracefully when AWS_* creds are absent so `shaka preflight` can
//! run in CI without secrets configured.

use std::collections::BTreeSet;
use std::fmt;
use std::process::Command;

use serde_json::Value;

#[derive(Debug)]
pub struct S3Error {
    pub message: String,
}

impl fmt::Display for S3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<std::io::Error> for S3Error {
    fn from(e: std::io::Error) -> Self {
        S3Error {
            message: format!("failed to run aws: {e}"),
        }
    }
}

/// True iff both `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` are set.
pub fn creds_present() -> bool {
    std::env::var("AWS_ACCESS_KEY_ID").is_ok() && std::env::var("AWS_SECRET_ACCESS_KEY").is_ok()
}

/// True iff `aws` is on PATH. Lets us skip-gracefully when the audit
/// runs outside shaka's devshell.
pub fn aws_available() -> bool {
    Command::new("aws")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn endpoint_url(cluster: &str) -> String {
    format!("https://{cluster}.linodeobjects.com")
}

/// List all `<kind>/<name>` prefixes that exist in the bucket. Walks two
/// levels: first the top-level kinds (e.g. `tfstate/`), then the names
/// within each (e.g. `tfstate/devbox/`). Returns an empty set for an
/// empty bucket.
pub fn list_namespace_prefixes(bucket: &str, cluster: &str) -> Result<BTreeSet<String>, S3Error> {
    let kinds = list_prefixes(bucket, cluster, "")?;
    let mut out = BTreeSet::new();
    for kind in kinds {
        let kind_prefix = format!("{kind}/");
        for name in list_prefixes(bucket, cluster, &kind_prefix)? {
            // `name` comes back including the kind prefix, e.g. `tfstate/devbox`.
            out.insert(name);
        }
    }
    Ok(out)
}

fn list_prefixes(bucket: &str, cluster: &str, prefix: &str) -> Result<BTreeSet<String>, S3Error> {
    let endpoint = endpoint_url(cluster);
    let mut args: Vec<&str> = vec![
        "s3api",
        "list-objects-v2",
        "--bucket",
        bucket,
        "--delimiter",
        "/",
        "--endpoint-url",
        &endpoint,
    ];
    if !prefix.is_empty() {
        args.push("--prefix");
        args.push(prefix);
    }
    let output = Command::new("aws").args(&args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(S3Error {
            message: format!("aws s3api list-objects-v2: {}", stderr.trim()),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(BTreeSet::new());
    }

    let parsed: Value = serde_json::from_str(&stdout).map_err(|e| S3Error {
        message: format!("failed to parse aws s3api JSON: {e}"),
    })?;

    let mut prefixes = BTreeSet::new();
    if let Some(arr) = parsed["CommonPrefixes"].as_array() {
        for p in arr {
            if let Some(s) = p["Prefix"].as_str() {
                let trimmed = s.trim_end_matches('/');
                if !trimmed.is_empty() {
                    prefixes.insert(trimmed.to_string());
                }
            }
        }
    }
    Ok(prefixes)
}
