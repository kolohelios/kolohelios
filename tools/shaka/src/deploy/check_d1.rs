//! `shaka deploy check-d1` — guards against a D1 binding shipping with a
//! placeholder `database_id`.
//!
//! A D1 database's id is provisioned by `shaka deploy generate-tf` (a
//! `cloudflare_d1_database` resource) and surfaced as a Terraform output the
//! maintainer pastes into the consumer's `wrangler.toml`
//! `[[d1_databases]] database_id`. Between provisioning and pasting, that id
//! is a placeholder — and a placeholder *passes* `wrangler deploy --dry-run`
//! (the PR-time `verify` job) yet fails the real post-merge deploy with
//! `binding <X> of type d1 must have a valid database_id` (code 10021),
//! breaking `main`'s deploy and blocking every deploy after it.
//!
//! This check fails preflight when a project that declares a `deploy.d1`
//! binding has a `wrangler.toml` `database_id` that isn't a real D1 id (a
//! UUID), so a deploy-breaking placeholder can't pass CI and auto-merge
//! (#964).

use std::path::Path;

use serde::Deserialize;

use crate::project::schema_check::{cue_project, discover};
use crate::term::{BOLD, DIM, GREEN, RED, RESET};

/// Subset of a project's CUE export: the name and, if present, the `d1`
/// binding under its `deploy:` block. The `deploy.target` discriminator and
/// every other deploy field are ignored — only `d1.binding` matters here.
#[derive(Deserialize)]
struct Export {
    name: String,
    #[serde(default)]
    deploy: Option<DeployD1>,
}

#[derive(Deserialize)]
struct DeployD1 {
    #[serde(default)]
    d1: Option<D1Binding>,
}

#[derive(Deserialize)]
struct D1Binding {
    binding: String,
}

pub fn run() {
    match run_at(Path::new(".")) {
        Ok(0) => {}
        Ok(n) => {
            eprintln!(
                "{RED}{BOLD}check-d1 failed{RESET}: {n} d1 binding(s) without a valid database_id"
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    }
}

/// Walk every project; for each that declares a `deploy.d1` binding, assert
/// its `wrangler.toml` carries a matching `[[d1_databases]]` entry whose
/// `database_id` is a real D1 id. Returns the number of failing bindings
/// (0 = pass).
fn run_at(root: &Path) -> Result<usize, String> {
    let mut failures = 0usize;
    let mut checked = 0usize;

    for project_dir in discover(root) {
        let project_file = project_dir.join("project.cue");
        if !project_file.exists() {
            continue;
        }
        let export = export_project(&project_file)?;
        let Some(binding) = export.deploy.and_then(|d| d.d1).map(|d| d.binding) else {
            continue;
        };
        checked += 1;

        let wrangler = project_dir.join("wrangler.toml");
        let contents = std::fs::read_to_string(&wrangler).map_err(|e| {
            format!(
                "{}: declares a d1 binding {binding:?} but {} is unreadable: {e}",
                export.name,
                wrangler.display()
            )
        })?;

        match parse_d1_database_ids(&contents)
            .into_iter()
            .find(|(b, _)| b == &binding)
        {
            None => {
                eprintln!(
                    "  {RED}{BOLD}FAIL{RESET}  {} {DIM}(no [[d1_databases]] entry for binding {binding:?} in wrangler.toml){RESET}",
                    export.name
                );
                failures += 1;
            }
            Some((_, id)) if !is_valid_d1_id(&id) => {
                eprintln!(
                    "  {RED}{BOLD}FAIL{RESET}  {} {DIM}(binding {binding:?} database_id {id:?} is not a valid D1 id — paste the real id from `tofu output`){RESET}",
                    export.name
                );
                failures += 1;
            }
            Some(_) => {
                println!(
                    "  {GREEN}{BOLD}ok{RESET}    {} {DIM}(d1 binding {binding:?}){RESET}",
                    export.name
                );
            }
        }
    }

    if checked == 0 {
        println!("{DIM}check-d1: no projects declare a d1 deploy binding{RESET}");
    }
    Ok(failures)
}

fn export_project(project_file: &Path) -> Result<Export, String> {
    let output =
        cue_project(&["export"], project_file).map_err(|e| format!("failed to spawn cue: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "cue export failed for {}: {stderr}",
            project_file.display()
        ));
    }
    serde_json::from_slice::<Export>(&output.stdout).map_err(|e| {
        format!(
            "could not parse cue export of {}: {e}",
            project_file.display()
        )
    })
}

/// Parse the `[[d1_databases]]` array-of-tables from a `wrangler.toml`,
/// returning `(binding, database_id)` for every entry that declares both.
/// A line scan keyed on table headers — enough for the flat shape wrangler
/// emits, and avoids pulling in a TOML-parser dependency.
pub(crate) fn parse_d1_database_ids(toml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_d1 = false;
    let mut binding: Option<String> = None;
    let mut id: Option<String> = None;

    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // A new table header ends the current `[[d1_databases]]` entry.
            if in_d1 {
                if let (Some(b), Some(i)) = (binding.take(), id.take()) {
                    out.push((b, i));
                }
                binding = None;
                id = None;
            }
            in_d1 = trimmed == "[[d1_databases]]";
            continue;
        }
        if in_d1 {
            if let Some(v) = toml_value(trimmed, "binding") {
                binding = Some(v);
            } else if let Some(v) = toml_value(trimmed, "database_id") {
                id = Some(v);
            }
        }
    }
    if in_d1 {
        if let (Some(b), Some(i)) = (binding.take(), id.take()) {
            out.push((b, i));
        }
    }
    out
}

/// Extract the string value of a `key = "value"` TOML line. Returns `None`
/// when `line` isn't an assignment to exactly `key` (a longer key like
/// `database_name` won't match `key = "database"`, since the `=` check fails
/// on the leftover `_name`).
fn toml_value(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start();
    let value = rest.strip_prefix('=')?.trim();
    Some(value.trim_matches('"').to_string())
}

/// Whether `s` looks like a real Cloudflare D1 id — a UUID
/// (`8-4-4-4-12` hex groups). A placeholder (`""`, `"REPLACE_ME"`, a
/// `tofu`-output reference, …) fails this, which is the whole point.
pub(crate) fn is_valid_d1_id(s: &str) -> bool {
    const GROUP_LENS: [usize; 5] = [8, 4, 4, 4, 12];
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == GROUP_LENS.len()
        && groups
            .iter()
            .zip(GROUP_LENS)
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/check-d1")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    #[test]
    fn parses_valid_d1_binding() {
        let ids = parse_d1_database_ids(&fixture("valid/with-d1.toml"));
        assert_eq!(
            ids,
            vec![(
                "DB".to_string(),
                "a1b2c3d4-1234-5678-9abc-def012345678".to_string()
            )]
        );
    }

    #[test]
    fn parses_multiple_d1_bindings() {
        let ids = parse_d1_database_ids(&fixture("valid/multiple-d1.toml"));
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0].0, "DB");
        assert_eq!(ids[1].0, "ANALYTICS");
    }

    #[test]
    fn no_d1_tables_yields_empty() {
        assert!(parse_d1_database_ids(&fixture("valid/no-d1.toml")).is_empty());
    }

    #[test]
    fn placeholder_id_is_parsed_but_invalid() {
        let ids = parse_d1_database_ids(&fixture("invalid/placeholder-id.toml"));
        let (_, id) = ids.iter().find(|(b, _)| b == "DB").expect("DB binding");
        assert!(!is_valid_d1_id(id), "placeholder {id:?} should be invalid");
    }

    #[test]
    fn valid_uuid_passes() {
        assert!(is_valid_d1_id("a1b2c3d4-1234-5678-9abc-def012345678"));
        assert!(is_valid_d1_id("00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn placeholders_and_malformed_ids_fail() {
        for bad in [
            "",
            "REPLACE_ME",
            "TODO",
            "placeholder",
            "a1b2c3d4-1234-5678-9abc",              // too few groups
            "a1b2c3d4-1234-5678-9abc-defXYZ345678", // non-hex
            "a1b2c3d4123456789abcdef012345678",     // no dashes
        ] {
            assert!(!is_valid_d1_id(bad), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn toml_value_rejects_longer_key() {
        // `database_name` must not satisfy a lookup for `database`.
        assert_eq!(toml_value("database_name = \"x\"", "database"), None);
        assert_eq!(
            toml_value("database_id = \"x\"", "database_id").as_deref(),
            Some("x")
        );
    }
}
