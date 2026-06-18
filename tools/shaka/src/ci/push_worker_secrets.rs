//! Push a Worker's declared runtime secrets to Cloudflare from the
//! ambient environment.
//!
//! The reusable `cf-deploy` workflow runs this under
//! `shaka ci mask-and-run --env-file=.env --`, so each declared secret's
//! `op://` reference is already resolved into an env var (and masked)
//! by the time this runs. For every declared name we read the matching
//! env var and pipe it to `wrangler secret put <NAME>`, which is
//! idempotent — re-uploading the same value is a no-op from the
//! consumer's perspective.
//!
//! Secret *names* come from two sources, unioned: `wrangler.secrets`
//! (for a shaka-managed `wrangler.toml`, the same source
//! `generate-wrangler` reads) and `ci.deploy.secrets` (for a
//! hand-authored `wrangler.toml`, which can't carry a `wrangler:` block
//! without tripping `generate-wrangler` drift). Secret *values* never
//! touch the repo — they arrive only through the resolved environment. A
//! project with no declared secrets is a clean no-op.
//!
//! `--name` / `--env` mirror `wrangler deploy`'s flags so the secret
//! lands on the same Worker the deploy targeted: `--name` for a
//! PR-preview script (`<project>-pr-<n>`), `--env` for a named
//! `[env.<name>]` production deploy.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::project::schema_check::cue_project;
use crate::term::{BOLD, GREEN, RED, RESET, YELLOW};

/// Subset of a project's CUE export — just the declared secret names,
/// from either `wrangler.secrets` or `ci.deploy.secrets` (see
/// `secrets_from_export`).
#[derive(Deserialize)]
struct ProjectExport {
    #[serde(default)]
    wrangler: Option<Wrangler>,
    #[serde(default)]
    ci: Option<Ci>,
}

#[derive(Deserialize)]
struct Wrangler {
    #[serde(default)]
    secrets: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Ci {
    #[serde(default)]
    deploy: Option<CiDeploy>,
}

#[derive(Deserialize)]
struct CiDeploy {
    #[serde(default)]
    secrets: Option<Vec<String>>,
}

pub fn run(project_dir: PathBuf, name: Option<String>, environment: Option<String>) {
    match run_inner(&project_dir, name.as_deref(), environment.as_deref()) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    }
}

fn run_inner(
    project_dir: &Path,
    script_name: Option<&str>,
    environment: Option<&str>,
) -> Result<(), String> {
    let project_file = project_dir.join("project.cue");
    if !project_file.exists() {
        return Err(format!("no project.cue at {}", project_dir.display()));
    }

    let secrets = declared_secrets(&project_file)?;
    if secrets.is_empty() {
        println!(
            "{YELLOW}{BOLD}push-worker-secrets:{RESET} no runtime secrets declared; nothing to push"
        );
        return Ok(());
    }

    for name in &secrets {
        let value = std::env::var(name).map_err(|_| {
            format!(
                "secret {name} is declared in {} but unset in the environment; \
                 add its `op://` reference to the project `.env.example`",
                project_file.display()
            )
        })?;
        if value.is_empty() {
            return Err(format!(
                "secret {name} resolved to an empty value; check its `op://` reference"
            ));
        }
        put_secret(name, &value, script_name, environment)?;
        // Print only the name — never the value (already masked, but
        // belt-and-suspenders).
        println!("  {GREEN}{BOLD}put{RESET}      {name}");
    }

    println!(
        "\n{GREEN}{BOLD}push-worker-secrets:{RESET} pushed {} secret(s)",
        secrets.len()
    );
    Ok(())
}

/// Read the project's CUE export and return its declared secret names —
/// the union of `wrangler.secrets` and `ci.deploy.secrets` (empty when
/// the project declares neither).
fn declared_secrets(project_file: &Path) -> Result<Vec<String>, String> {
    let output =
        cue_project(&["export"], project_file).map_err(|e| format!("failed to spawn cue: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "cue export failed for {}: {stderr}",
            project_file.display()
        ));
    }
    secrets_from_export(&output.stdout).map_err(|e| {
        format!(
            "could not parse cue export of {}: {e}",
            project_file.display()
        )
    })
}

/// Parse declared secret names out of a project's `cue export` JSON —
/// the union of `wrangler.secrets` and `ci.deploy.secrets`, order-stable
/// (wrangler names first) and de-duplicated so a name declared in both
/// is pushed once.
fn secrets_from_export(json: &[u8]) -> Result<Vec<String>, serde_json::Error> {
    let export: ProjectExport = serde_json::from_slice(json)?;
    let mut names = export.wrangler.and_then(|w| w.secrets).unwrap_or_default();
    let ci = export
        .ci
        .and_then(|c| c.deploy)
        .and_then(|d| d.secrets)
        .unwrap_or_default();
    for name in ci {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Ok(names)
}

/// Run `wrangler secret put <name>`, piping `value` on stdin so the
/// command runs non-interactively.
fn put_secret(
    name: &str,
    value: &str,
    script_name: Option<&str>,
    environment: Option<&str>,
) -> Result<(), String> {
    let args = secret_put_args(name, script_name, environment);
    let mut child = Command::new("wrangler")
        .args(&args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not spawn `wrangler {}`: {e}", args.join(" ")))?;

    child
        .stdin
        .take()
        .ok_or_else(|| "wrangler stdin not captured".to_string())?
        .write_all(value.as_bytes())
        .map_err(|e| format!("writing secret {name} to wrangler stdin: {e}"))?;

    let status = child
        .wait()
        .map_err(|e| format!("waiting on `wrangler {}`: {e}", args.join(" ")))?;
    if !status.success() {
        return Err(format!(
            "`wrangler {}` exited with status {status}",
            args.join(" ")
        ));
    }
    Ok(())
}

/// Build the `wrangler secret put` argument vector, appending `--name`
/// and `--env` to mirror the targeting flags `wrangler deploy` used.
fn secret_put_args(
    name: &str,
    script_name: Option<&str>,
    environment: Option<&str>,
) -> Vec<String> {
    let mut args = vec!["secret".to_string(), "put".to_string(), name.to_string()];
    if let Some(script) = script_name {
        args.push("--name".to_string());
        args.push(script.to_string());
    }
    if let Some(env) = environment {
        args.push("--env".to_string());
        args.push(env.to_string());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_from_export_reads_declared_names() {
        let json = br#"{"name":"x","wrangler":{"secrets":["KIT_API_KEY","OTHER"]}}"#;
        assert_eq!(
            secrets_from_export(json).unwrap(),
            vec!["KIT_API_KEY".to_string(), "OTHER".to_string()]
        );
    }

    #[test]
    fn secrets_from_export_is_empty_without_wrangler_block() {
        assert!(secrets_from_export(br#"{"name":"x"}"#).unwrap().is_empty());
    }

    #[test]
    fn secrets_from_export_is_empty_without_secrets_field() {
        let json = br#"{"name":"x","wrangler":{"main":"shim.mjs"}}"#;
        assert!(secrets_from_export(json).unwrap().is_empty());
    }

    #[test]
    fn secrets_from_export_reads_ci_deploy_secrets() {
        // A hand-authored-wrangler project declares secrets under
        // `ci.deploy.secrets` (no `wrangler:` block at all).
        let json = br#"{"name":"x","ci":{"deploy":{"secrets":["OPENROUTER_API_KEY"]}}}"#;
        assert_eq!(
            secrets_from_export(json).unwrap(),
            vec!["OPENROUTER_API_KEY".to_string()]
        );
    }

    #[test]
    fn secrets_from_export_unions_wrangler_and_ci_deploy_dedup() {
        // Both sources contribute; `SHARED` appears in both and is pushed
        // once, with wrangler names ordered first.
        let json = br#"{"name":"x","wrangler":{"secrets":["KIT_API_KEY","SHARED"]},"ci":{"deploy":{"secrets":["SHARED","OPENROUTER_API_KEY"]}}}"#;
        assert_eq!(
            secrets_from_export(json).unwrap(),
            vec![
                "KIT_API_KEY".to_string(),
                "SHARED".to_string(),
                "OPENROUTER_API_KEY".to_string()
            ]
        );
    }

    #[test]
    fn secret_put_args_bare() {
        assert_eq!(
            secret_put_args("KIT_API_KEY", None, None),
            vec!["secret", "put", "KIT_API_KEY"]
        );
    }

    #[test]
    fn secret_put_args_with_script_name() {
        assert_eq!(
            secret_put_args("KIT_API_KEY", Some("kolohelios-portfolio-pr-7"), None),
            vec![
                "secret",
                "put",
                "KIT_API_KEY",
                "--name",
                "kolohelios-portfolio-pr-7"
            ]
        );
    }

    #[test]
    fn secret_put_args_with_environment() {
        assert_eq!(
            secret_put_args("KIT_API_KEY", None, Some("production")),
            vec!["secret", "put", "KIT_API_KEY", "--env", "production"]
        );
    }
}
