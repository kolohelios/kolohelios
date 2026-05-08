//! Thin Linode API client built on `curl` shell-out.
//!
//! Linode's Object Storage management endpoints (bucket lifecycle, access
//! keys) are JSON-over-HTTPS and need only a Bearer token from
//! `LINODE_TOKEN`. Following shaka's existing pattern (`gh.rs`, `jj.rs`)
//! we shell out to `curl` rather than pull in `reqwest` and its TLS deps.

use std::process::Command;

use serde_json::{json, Value};
use snafu::{OptionExt, ResultExt, Snafu};

const API_BASE: &str = "https://api.linode.com/v4";

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum LinodeError {
    #[snafu(display("failed to run curl: {source}"))]
    Spawn { source: std::io::Error },

    #[snafu(display("LINODE_TOKEN is not set in the environment"))]
    TokenMissing,

    #[snafu(display("curl {method} {endpoint}: {stderr}"))]
    CurlCommand {
        method: String,
        endpoint: String,
        stderr: String,
    },

    #[snafu(display("could not parse HTTP status from curl output: {output:?}"))]
    StatusParse { output: String },

    #[snafu(display("failed to parse JSON from {method} {endpoint}: {source}"))]
    JsonParse {
        method: String,
        endpoint: String,
        source: serde_json::Error,
    },

    #[snafu(display("{message}"))]
    Schema { message: String },
}

/// Read the Linode personal access token from `LINODE_TOKEN`.
pub fn token_from_env() -> Result<String, LinodeError> {
    std::env::var("LINODE_TOKEN")
        .ok()
        .context(TokenMissingSnafu)
}

#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Value,
}

impl HttpResponse {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// GET an endpoint, returning parsed JSON. `endpoint` is path-only
/// (e.g. `/object-storage/buckets/us-sea-1/kolohelios`).
pub fn get(token: &str, endpoint: &str) -> Result<HttpResponse, LinodeError> {
    request(token, "GET", endpoint, None)
}

/// POST a JSON body and return parsed JSON. Pass `None` for empty body.
pub fn post(
    token: &str,
    endpoint: &str,
    body: Option<&Value>,
) -> Result<HttpResponse, LinodeError> {
    request(token, "POST", endpoint, body)
}

fn request(
    token: &str,
    method: &str,
    endpoint: &str,
    body: Option<&Value>,
) -> Result<HttpResponse, LinodeError> {
    let url = format!("{API_BASE}{endpoint}");
    let auth = format!("Authorization: Bearer {token}");

    let mut args: Vec<String> = vec![
        "--silent".into(),
        "--show-error".into(),
        "--write-out".into(),
        "\n%{http_code}".into(),
        "-X".into(),
        method.into(),
        "-H".into(),
        auth,
        "-H".into(),
        "Content-Type: application/json".into(),
    ];

    let body_str = body.map(|v| serde_json::to_string(v).unwrap_or_default());
    if let Some(ref s) = body_str {
        args.push("--data".into());
        args.push(s.clone());
    }
    args.push(url);

    let output = Command::new("curl")
        .args(&args)
        .output()
        .context(SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return CurlCommandSnafu {
            method: method.to_string(),
            endpoint: endpoint.to_string(),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (body_part, status_part) = match stdout.rsplit_once('\n') {
        Some((b, s)) => (b, s),
        None => ("", stdout.as_ref()),
    };
    let status: u16 = status_part
        .trim()
        .parse()
        .ok()
        .with_context(|| StatusParseSnafu {
            output: stdout.to_string(),
        })?;
    let body_json = if body_part.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(body_part).with_context(|_| JsonParseSnafu {
            method: method.to_string(),
            endpoint: endpoint.to_string(),
        })?
    };
    Ok(HttpResponse {
        status,
        body: body_json,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketKey {
    pub label: String,
    pub access_key: String,
    pub secret_key: String,
}

/// Look up a bucket by cluster and label. `Ok(None)` when the bucket
/// does not exist (HTTP 404); other non-2xx responses become errors.
pub fn get_bucket(token: &str, cluster: &str, label: &str) -> Result<Option<Value>, LinodeError> {
    let endpoint = format!("/object-storage/buckets/{cluster}/{label}");
    let resp = get(token, &endpoint)?;
    if resp.status == 404 {
        return Ok(None);
    }
    if !resp.ok() {
        return SchemaSnafu {
            message: format!(
                "GET {endpoint} returned HTTP {} ({})",
                resp.status,
                resp.body
                    .get("errors")
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "no error body".into())
            ),
        }
        .fail();
    }
    Ok(Some(resp.body))
}

/// Create a bucket. The Linode API rejects duplicate labels per cluster,
/// so call [`get_bucket`] first if you need idempotency.
pub fn create_bucket(token: &str, cluster: &str, label: &str) -> Result<Value, LinodeError> {
    let body = json!({
        "cluster": cluster,
        "label": label,
        "acl": "private",
    });
    let resp = post(token, "/object-storage/buckets", Some(&body))?;
    if !resp.ok() {
        return SchemaSnafu {
            message: format!(
                "POST /object-storage/buckets returned HTTP {} ({})",
                resp.status, resp.body
            ),
        }
        .fail();
    }
    Ok(resp.body)
}

/// Create a bucket-scoped access key with read/write on a single bucket.
pub fn create_bucket_key(
    token: &str,
    cluster: &str,
    bucket: &str,
    label: &str,
) -> Result<BucketKey, LinodeError> {
    let body = json!({
        "label": label,
        "bucket_access": [
            {
                "cluster": cluster,
                "bucket_name": bucket,
                "permissions": "read_write",
            }
        ],
    });
    let resp = post(token, "/object-storage/keys", Some(&body))?;
    if !resp.ok() {
        return SchemaSnafu {
            message: format!(
                "POST /object-storage/keys returned HTTP {} ({})",
                resp.status, resp.body
            ),
        }
        .fail();
    }
    let access_key = resp.body["access_key"]
        .as_str()
        .context(SchemaSnafu {
            message: "create_bucket_key response missing 'access_key'",
        })?
        .to_string();
    let secret_key = resp.body["secret_key"]
        .as_str()
        .context(SchemaSnafu {
            message: "create_bucket_key response missing 'secret_key' \
                (Linode only returns the secret on creation — \
                deletion via the Linode console is required to retry)",
        })?
        .to_string();
    Ok(BucketKey {
        label: label.to_string(),
        access_key,
        secret_key,
    })
}
