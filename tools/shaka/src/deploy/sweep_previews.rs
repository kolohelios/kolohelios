//! `shaka deploy sweep-previews` — reap orphaned Cloudflare preview
//! Workers.
//!
//! Each Worker app with a `ci.deploy` block deploys a per-PR preview
//! Worker named `<previewScriptPrefix>-pr-<N>` (see
//! `ci/worker_deploy_workflow.rs`). The generated `<name>-cleanup.yml`
//! deletes each one on its PR's `closed` event. This command is the
//! project-agnostic backstop for anything that path misses (a cleanup
//! run that errored, a Worker created before cleanup was generated,
//! #719): it lists every preview Worker, asks GitHub whether its PR is
//! still open, and deletes the ones whose PR has closed or merged.
//!
//! Discovery is metadata-driven — it reads each `project.cue`'s
//! `ci.deploy.previewScriptPrefix` — so a new Worker app is covered
//! the moment it lands, with no edit here. It runs on `push: main` and
//! a daily cron via `.github/workflows/sweep-preview-workers.yaml`.
//!
//! Credentials come from the environment (`CLOUDFLARE_API_TOKEN`,
//! `CLOUDFLARE_ACCOUNT_ID`) exactly as `wrangler` consumes them, so the
//! caller wraps this in `op run --env-file=.env -- shaka deploy
//! sweep-previews`. PR state comes from `gh`.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;
use snafu::{OptionExt, ResultExt, Snafu};

use crate::project::schema_check::{cue_project, discover, project_schema_path};
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

#[derive(Debug, Snafu)]
enum SweepError {
    #[snafu(display(
        "{name} is not set; run via `op run --env-file=.env -- shaka deploy sweep-previews`"
    ))]
    MissingEnv { name: String },

    #[snafu(display("could not resolve project schema: {message}"))]
    Schema { message: String },

    #[snafu(display("cue export failed for {path}: {stderr}"))]
    CueExport { path: String, stderr: String },

    #[snafu(display("failed to spawn {cmd}: {source}"))]
    Spawn { cmd: String, source: std::io::Error },

    // Deliberately omits the Cloudflare bearer token that lives in the
    // curl argv — only the label and stderr surface.
    #[snafu(display("{label} failed: {stderr}"))]
    Curl { label: String, stderr: String },

    #[snafu(display("Cloudflare API error ({label}): {message}"))]
    CloudflareApi { label: String, message: String },

    #[snafu(display("{context}: {source}"))]
    JsonParse {
        context: String,
        source: serde_json::Error,
    },

    #[snafu(display("gh: {source}"))]
    Gh { source: crate::gh::GhError },
}

/// Subset of a project's CUE export: just the preview-Worker prefix.
#[derive(Deserialize)]
struct ProjectExport {
    #[serde(default)]
    ci: Option<Ci>,
}

#[derive(Deserialize)]
struct Ci {
    #[serde(default)]
    deploy: Option<CiDeploy>,
}

#[derive(Deserialize)]
struct CiDeploy {
    #[serde(rename = "previewScriptPrefix")]
    preview_script_prefix: String,
}

pub fn run(dry_run: bool) {
    match sweep(Path::new("."), dry_run) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{RED}{BOLD}deploy sweep-previews failed{RESET}: {e}");
            std::process::exit(1);
        }
    }
}

fn sweep(root: &Path, dry_run: bool) -> Result<(), SweepError> {
    let token = env_var("CLOUDFLARE_API_TOKEN")?;
    let account = env_var("CLOUDFLARE_ACCOUNT_ID")?;

    let prefixes = collect_prefixes(root)?;
    if prefixes.is_empty() {
        println!("{GREEN}{BOLD}sweep-previews{RESET}: no Worker projects with a deploy block");
        return Ok(());
    }

    let scripts = cf_list_script_ids(&account, &token)?;

    // Every (script, prefix, pr) where the script is a preview Worker.
    let mut previews: Vec<(String, u64)> = Vec::new();
    for script in &scripts {
        for prefix in &prefixes {
            if let Some(pr) = preview_pr_number(script, prefix) {
                previews.push((script.clone(), pr));
                break;
            }
        }
    }

    if previews.is_empty() {
        println!(
            "{GREEN}{BOLD}sweep-previews{RESET}: no preview Workers found ({} script(s) scanned)",
            scripts.len()
        );
        return Ok(());
    }

    let mut deleted = 0usize;
    let mut kept = 0usize;
    for (script, pr) in &previews {
        if pr_is_open(*pr)? {
            println!("  {DIM}keep{RESET}   {script} (PR #{pr} open)");
            kept += 1;
            continue;
        }
        if dry_run {
            println!("  {YELLOW}orphan{RESET} {script} (PR #{pr} closed) — would delete");
        } else {
            cf_delete_script(&account, &token, script)?;
            println!("  {YELLOW}delete{RESET} {script} (PR #{pr} closed)");
        }
        deleted += 1;
    }

    let verb = if dry_run { "would delete" } else { "deleted" };
    println!(
        "{GREEN}{BOLD}sweep-previews{RESET}: {verb} {deleted}, kept {kept} ({} preview Worker(s))",
        previews.len()
    );
    Ok(())
}

fn env_var(name: &str) -> Result<String, SweepError> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .context(MissingEnvSnafu { name })
}

/// Walk every project and collect the `ci.deploy.previewScriptPrefix`
/// of each Worker app, sorted and deduplicated.
fn collect_prefixes(root: &Path) -> Result<Vec<String>, SweepError> {
    project_schema_path().map_err(|e| {
        SchemaSnafu {
            message: e.to_string(),
        }
        .build()
    })?;
    let mut out = Vec::new();
    for project_dir in discover(root) {
        let project_file = project_dir.join("project.cue");
        if !project_file.exists() {
            continue;
        }
        let output =
            cue_project(&["export"], &project_file).context(SpawnSnafu { cmd: "cue export" })?;
        if !output.status.success() {
            return CueExportSnafu {
                path: project_file.display().to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            }
            .fail();
        }
        let export: ProjectExport =
            serde_json::from_slice(&output.stdout).context(JsonParseSnafu {
                context: format!("cue export of {}", project_file.display()),
            })?;
        if let Some(prefix) = export
            .ci
            .and_then(|c| c.deploy)
            .map(|d| d.preview_script_prefix)
        {
            out.push(prefix);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// `<prefix>-pr-<N>` → `Some(N)`, else `None`. Anchored on both ends:
/// the production Worker `<prefix>` (no `-pr-<N>` suffix) and unrelated
/// Workers never match, so they're never deletion candidates.
fn preview_pr_number(script: &str, prefix: &str) -> Option<u64> {
    let suffix = script.strip_prefix(prefix)?.strip_prefix("-pr-")?;
    if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    suffix.parse().ok()
}

/// `.result[].id` from a Cloudflare `workers/scripts` list response.
fn extract_script_ids(body: &Value) -> Vec<String> {
    body.get("result")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn cf_list_script_ids(account: &str, token: &str) -> Result<Vec<String>, SweepError> {
    let url = format!("https://api.cloudflare.com/client/v4/accounts/{account}/workers/scripts");
    let body = cf_curl("GET", token, &url, "cloudflare list scripts")?;
    Ok(extract_script_ids(&body))
}

fn cf_delete_script(account: &str, token: &str, name: &str) -> Result<(), SweepError> {
    let url =
        format!("https://api.cloudflare.com/client/v4/accounts/{account}/workers/scripts/{name}");
    cf_curl("DELETE", token, &url, "cloudflare delete script")?;
    Ok(())
}

/// Run an authenticated CF API call via `curl` and return the parsed
/// body, failing if the API's `success` flag is false. The bearer
/// token rides in the argv; `label` (not the full command) is what
/// reaches error messages so the token never lands in a log.
fn cf_curl(method: &str, token: &str, url: &str, label: &str) -> Result<Value, SweepError> {
    let output = Command::new("curl")
        .args(["-sS", "-X", method, "-H"])
        .arg(format!("Authorization: Bearer {token}"))
        .arg(url)
        .output()
        .context(SpawnSnafu { cmd: "curl" })?;
    if !output.status.success() {
        return CurlSnafu {
            label: label.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }
        .fail();
    }
    let body: Value = serde_json::from_slice(&output.stdout).context(JsonParseSnafu {
        context: format!("{label} response"),
    })?;
    if body.get("success").and_then(Value::as_bool) != Some(true) {
        let message = body
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown error".to_string());
        return CloudflareApiSnafu {
            label: label.to_string(),
            message,
        }
        .fail();
    }
    Ok(body)
}

/// Is PR `number` still open? Closed and merged PRs both count as "not
/// open" — either way the preview is reapable. Resolves the repo from
/// `GITHUB_REPOSITORY` when set (CI), otherwise relies on `gh`'s
/// ambient repo context (a checkout's git remote).
fn pr_is_open(number: u64) -> Result<bool, SweepError> {
    let endpoint = match std::env::var("GITHUB_REPOSITORY") {
        Ok(repo) if !repo.is_empty() => format!("repos/{repo}/pulls/{number}"),
        _ => format!("repos/{{owner}}/{{repo}}/pulls/{number}"),
    };
    let body = crate::gh::api_get(&endpoint).context(GhSnafu)?;
    Ok(body.get("state").and_then(Value::as_str) == Some("open"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_pr_number_matches_prefix_pr_n() {
        assert_eq!(
            preview_pr_number("cascadiajs2026-notes-pr-713", "cascadiajs2026-notes"),
            Some(713)
        );
        assert_eq!(preview_pr_number("portfolio-pr-1", "portfolio"), Some(1));
    }

    #[test]
    fn preview_pr_number_rejects_production_and_unrelated() {
        // Production Worker — no `-pr-<N>` suffix.
        assert_eq!(
            preview_pr_number("cascadiajs2026-notes", "cascadiajs2026-notes"),
            None
        );
        // A Worker for a different app entirely.
        assert_eq!(preview_pr_number("buzzingo", "cascadiajs2026-notes"), None);
        // Non-numeric / empty suffix.
        assert_eq!(preview_pr_number("portfolio-pr-", "portfolio"), None);
        assert_eq!(preview_pr_number("portfolio-pr-abc", "portfolio"), None);
        // Prefix is a substring but not a `-pr-` boundary.
        assert_eq!(preview_pr_number("portfolio-staging", "portfolio"), None);
    }

    #[test]
    fn extract_script_ids_reads_cloudflare_list_response() {
        // Real `GET /accounts/{id}/workers/scripts` response shape.
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sweep-previews/workers-scripts.json"
        ))
        .expect("fixture readable");
        let body: Value = serde_json::from_str(&raw).expect("valid JSON");
        let ids = extract_script_ids(&body);
        assert_eq!(
            ids,
            vec![
                "buzzingo",
                "cascadiajs2026-notes",
                "cascadiajs2026-notes-pr-713",
                "kolohelios-portfolio",
            ]
        );
    }
}
