//! Renew declarations through a PR; Terraform owns credentials and remote state.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use hcl_edit::structure::Body;
use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu};

use crate::{gh, jj, repo};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("token renewal I/O failed: {source}"))]
    Io { source: std::io::Error },
    #[snafu(display("{command} failed (output withheld to protect credentials)"))]
    Command { command: String },
    #[snafu(display("invalid token renewal configuration: {message}"))]
    Invalid { message: String },
    #[snafu(display("could not decode token metadata: {source}"))]
    Json { source: serde_json::Error },
}

#[derive(Deserialize)]
struct Token {
    name: String,
    expires_on: Option<String>,
    renewal: Option<Policy>,
}

#[derive(Deserialize)]
struct Policy {
    valid_days: i64,
    renew_before_days: i64,
    secret_ref: String,
}

pub struct RenewOptions {
    pub registry_dir: PathBuf,
    pub terraform_file: PathBuf,
    pub now: Option<String>,
    pub dry_run: bool,
    pub pr_bookmark: Option<String>,
    pub auto_merge: bool,
}

fn checked(command: &mut Command, label: &str) -> Result<Output, Error> {
    let out = command.output().context(IoSnafu)?;
    ensure_success(out, label)
}

fn ensure_success(out: Output, label: &str) -> Result<Output, Error> {
    if !out.status.success() {
        return CommandSnafu { command: label }.fail();
    }
    Ok(out)
}

fn invalid(message: impl Into<String>) -> Error {
    InvalidSnafu {
        message: message.into(),
    }
    .build()
}

fn timestamp(value: &str) -> Result<DateTime<Utc>, Error> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|_| invalid("expected an RFC3339 timestamp"))
}

fn read_registry(dir: &Path) -> Result<Vec<(PathBuf, Token)>, Error> {
    let mut schema = tempfile::Builder::new()
        .suffix(".cue")
        .tempfile()
        .context(IoSnafu)?;
    schema
        .write_all(include_bytes!("../../../schema/cloudflare-token.cue"))
        .context(IoSnafu)?;
    let mut files = std::fs::read_dir(dir)
        .context(IoSnafu)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .context(IoSnafu)?;
    files.retain(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "cue"));
    files.sort();
    if files.is_empty() {
        return Err(invalid("token registry is empty"));
    }
    files
        .into_iter()
        .map(|path| {
            let out = checked(
                Command::new("cue")
                    .arg("export")
                    .arg(schema.path())
                    .arg(&path),
                "cue export",
            )?;
            let token = serde_json::from_slice(&out.stdout).context(JsonSnafu)?;
            Ok((path, token))
        })
        .collect()
}

fn next_expiry(token: &Token, now: DateTime<Utc>) -> Result<Option<String>, Error> {
    let Some(policy) = &token.renewal else {
        return Ok(None);
    };
    if !(2..=365).contains(&policy.valid_days)
        || policy.renew_before_days < 1
        || policy.renew_before_days >= policy.valid_days
    {
        return Err(invalid(
            "renewal window must be positive and shorter than lifetime (2–365 days)",
        ));
    }
    let old = token.expires_on.as_deref().context(InvalidSnafu {
        message: "renewable tokens require expires_on",
    })?;
    if timestamp(old)? > now + Duration::days(policy.renew_before_days) {
        return Ok(None);
    }
    Ok(Some(
        (now + Duration::days(policy.valid_days)).to_rfc3339_opts(SecondsFormat::Secs, true),
    ))
}

/// Read and validate every input before changing any file. A mismatched TF
/// declaration fails closed rather than creating a PR that renews only CUE.
fn prepare(options: &RenewOptions, now: DateTime<Utc>) -> Result<Vec<(PathBuf, String)>, Error> {
    let tokens = read_registry(&options.registry_dir)?;
    let mut tf: Body = std::fs::read_to_string(&options.terraform_file)
        .context(IoSnafu)?
        .parse()
        .map_err(|_| invalid("could not parse Terraform declarations"))?;
    let mut edits = Vec::new();
    let mut names = std::collections::HashSet::new();
    for (path, token) in tokens {
        if !names.insert(token.name.clone()) {
            return Err(invalid("duplicate token name"));
        }
        let Some(new) = next_expiry(&token, now)? else {
            continue;
        };
        let old = token
            .expires_on
            .as_deref()
            .expect("next_expiry checked expiry");
        let key = format!("{}_expires_on", token.name.replace('-', "_"));
        let mut found = 0;
        for block in tf
            .blocks_mut()
            .filter(|block| block.ident.as_str() == "locals")
        {
            if let Some(mut attr) = block.body.get_attribute_mut(&key) {
                if attr.value.as_str() != Some(old) {
                    return Err(invalid(format!(
                        "Terraform expiry for {} does not match CUE",
                        token.name
                    )));
                }
                *attr.value_mut() = new.as_str().into();
                found += 1;
            }
        }
        if found != 1 {
            return Err(invalid(format!(
                "expected exactly one Terraform local {key}"
            )));
        }
        let source = std::fs::read_to_string(&path).context(IoSnafu)?;
        let quoted = format!("\"{old}\"");
        if source.matches(&quoted).count() != 1 {
            return Err(invalid(format!(
                "expected one literal expiry in {}",
                path.display()
            )));
        }
        edits.push((path, source.replacen(&quoted, &format!("\"{new}\""), 1)));
    }
    if !edits.is_empty() {
        edits.push((options.terraform_file.clone(), tf.to_string()));
    }
    Ok(edits)
}

pub fn renew(options: RenewOptions) -> Result<(), Error> {
    if options.auto_merge && options.pr_bookmark.is_none() {
        return Err(invalid("--auto-merge requires --pr-bookmark"));
    }
    if options.pr_bookmark.is_some() {
        let diff = jj::run(&["diff", "--summary"]).map_err(|e| invalid(e.to_string()))?;
        if !diff.trim().is_empty() {
            return Err(invalid("publishing renewal requires a clean working copy"));
        }
        let slug = gh::detect_repo_or_env().map_err(|e| invalid(e.to_string()))?;
        let branch = options.pr_bookmark.as_deref().expect("checked bookmark");
        if let Some(pr) = gh::pr_for_head(&slug, branch).map_err(|e| invalid(e.to_string()))? {
            if pr.state == gh::PrState::Open {
                println!("Renewal PR already open: {}", pr.url);
                return Ok(());
            }
        }
    }
    let now = options
        .now
        .as_deref()
        .map(timestamp)
        .transpose()?
        .unwrap_or_else(Utc::now);
    let edits = prepare(&options, now)?;
    if edits.is_empty() {
        println!("No Cloudflare tokens are due for renewal");
        return Ok(());
    }
    for (path, contents) in &edits {
        println!(
            "{} {}",
            if options.dry_run {
                "Would renew"
            } else {
                "Renewing"
            },
            path.display()
        );
        if !options.dry_run {
            std::fs::write(path, contents).context(IoSnafu)?;
        }
    }
    if !options.dry_run {
        checked(
            Command::new("tofu").arg("fmt").arg(&options.terraform_file),
            "tofu fmt",
        )?;
        if let Some(bookmark) = options.pr_bookmark {
            jj::describe("chore(cloudflare-tokens): renew API token lifetimes\n\nRenew before expiry; Terraform rotates credentials and updates 1Password\nafter this declaration change merges.")
                .map_err(|e| invalid(e.to_string()))?;
            crate::preflight::run(false, Some("main@origin".to_string()), false);
            repo::send::run(Some(bookmark), false, !options.auto_merge, false);
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct Verification {
    success: bool,
    result: Option<VerificationResult>,
}

#[derive(Deserialize)]
struct VerificationResult {
    status: String,
    expires_on: Option<String>,
}

fn check_verification(bytes: &[u8], expected: &str, now: DateTime<Utc>) -> Result<(), Error> {
    let response: Verification = serde_json::from_slice(bytes).context(JsonSnafu)?;
    let result = response
        .result
        .filter(|r| response.success && r.status == "active")
        .ok_or_else(|| invalid("Cloudflare credential is not active"))?;
    let expires = result
        .expires_on
        .ok_or_else(|| invalid("Cloudflare did not report an expiry"))?;
    if timestamp(&expires)? < timestamp(expected)? || timestamp(&expires)? <= now {
        return Err(invalid(
            "Cloudflare credential expiry is stale or in the past",
        ));
    }
    Ok(())
}

pub fn verify(registry_dir: &Path) -> Result<(), Error> {
    for (_, token) in read_registry(registry_dir)? {
        let Some(policy) = &token.renewal else {
            continue;
        };
        let out = checked(
            Command::new("op").arg("read").arg(&policy.secret_ref),
            "op read",
        )?;
        let value = std::str::from_utf8(&out.stdout)
            .map_err(|_| invalid("credential is not UTF-8"))?
            .trim();
        // Header config must not admit quotes/newlines or curl config injection.
        if value.is_empty()
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(invalid("unexpected credential format"));
        }
        // Credentials go over stdin, never in argv, diagnostics, or temp files.
        let mut child = Command::new("curl")
            .args([
                "--silent",
                "--max-time",
                "30",
                "--connect-timeout",
                "10",
                "--proto",
                "=https",
                "--config",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context(IoSnafu)?;
        {
            let mut input = child.stdin.take().expect("stdin piped");
            writeln!(input, "url = \"https://api.cloudflare.com/client/v4/user/tokens/verify\"\nheader = \"Authorization: Bearer {value}\"")
                .context(IoSnafu)?;
        }
        let response = ensure_success(
            child.wait_with_output().context(IoSnafu)?,
            "Cloudflare verification request",
        )?;
        check_verification(
            &response.stdout,
            token.expires_on.as_deref().context(InvalidSnafu {
                message: "renewable token has no expiry",
            })?,
            Utc::now(),
        )
        .map_err(|_| {
            invalid(format!(
                "{}: live credential verification failed",
                token.name
            ))
        })?;
        println!("Verified {}: active with current expiry", token.name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(expiry: &str) -> Token {
        Token {
            name: "deploy".into(),
            expires_on: Some(expiry.into()),
            renewal: Some(Policy {
                valid_days: 90,
                renew_before_days: 14,
                secret_ref: "op://vault/item/password".into(),
            }),
        }
    }

    #[test]
    fn renewal_is_due_at_boundary_and_recovers_expired_tokens() {
        let now = timestamp("2027-01-27T00:00:00Z").unwrap();
        assert!(next_expiry(&token("2027-02-10T00:00:01Z"), now)
            .unwrap()
            .is_none());
        assert_eq!(
            next_expiry(&token("2027-02-10T00:00:00Z"), now)
                .unwrap()
                .unwrap(),
            "2027-04-27T00:00:00Z"
        );
        assert_eq!(
            next_expiry(&token("2026-08-11T00:00:00Z"), now)
                .unwrap()
                .unwrap(),
            "2027-04-27T00:00:00Z"
        );
    }

    #[test]
    fn renewal_is_opt_in_and_policy_is_bounded() {
        let now = timestamp("2027-01-27T00:00:00Z").unwrap();
        let mut value = token("2027-02-10T00:00:00Z");
        value.renewal.as_mut().unwrap().renew_before_days = 90;
        assert!(next_expiry(&value, now).is_err());
        value.renewal = None;
        assert!(next_expiry(&value, now).unwrap().is_none());
    }

    #[test]
    fn verification_fixtures() {
        let base =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/token-renewal/verification");
        for (dir, pass) in [("valid", true), ("invalid", false)] {
            for entry in std::fs::read_dir(base.join(dir)).unwrap() {
                let path = entry.unwrap().path();
                let bytes = std::fs::read(&path).unwrap();
                assert_eq!(
                    check_verification(
                        &bytes,
                        "2027-02-10T00:00:00Z",
                        timestamp("2027-01-27T00:00:00Z").unwrap()
                    )
                    .is_ok(),
                    pass,
                    "{}",
                    path.display()
                );
            }
        }
    }
}
