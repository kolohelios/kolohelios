use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;

use crate::project::schema_check;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

use super::workflow::{
    self, CancelInProgress, Concurrency, Empty, Job, On, PermissionLevel, Permissions, PushTrigger,
    ReusableCall, Workflow,
};

#[derive(Deserialize)]
struct ProjectMeta {
    name: String,
    #[serde(default)]
    ci: Option<Ci>,
}

#[derive(Deserialize)]
struct Ci {
    #[serde(default)]
    apply: Option<CiApply>,
    #[serde(default)]
    build: Option<super::main_workflow::CiBuild>,
    #[serde(default)]
    deploy: Option<super::worker_deploy_workflow::CiDeploy>,
}

#[derive(Deserialize)]
struct CiApply {
    reusable_workflow: String,
}

pub fn run(check: bool) {
    if let Err(e) = schema_check::project_schema_path() {
        eprintln!("{RED}{BOLD}error:{RESET} could not resolve project schema: {e}");
        std::process::exit(1);
    }

    let mut items: Vec<(PathBuf, String)> = Vec::new();
    let mut build_specs: Vec<super::main_workflow::MainBuildSpec> = Vec::new();
    for project_dir in schema_check::discover(Path::new(".")) {
        let meta = match read_meta(&project_dir) {
            Ok(m) => m,
            Err(e) => {
                eprintln!(
                    "{RED}{BOLD}error:{RESET} reading {}: {e}",
                    project_dir.display()
                );
                std::process::exit(1);
            }
        };
        let stripped_dir = project_dir
            .strip_prefix("./")
            .unwrap_or(&project_dir)
            .to_path_buf();
        if let Some(ci) = meta.ci {
            if let Some(apply) = ci.apply {
                let workflow = build_apply_workflow(&meta.name, &project_dir, &apply);
                let target =
                    PathBuf::from(".github/workflows").join(format!("{}-apply.yml", meta.name));
                items.push((target, workflow::emit(&workflow)));
            }
            if let Some(build) = ci.build {
                build_specs.push(super::main_workflow::MainBuildSpec {
                    project_dir: stripped_dir.clone(),
                    project_name: meta.name.clone(),
                    build,
                });
            }
            if let Some(deploy) = ci.deploy {
                let spec = super::worker_deploy_workflow::WorkerDeploySpec {
                    project_dir: stripped_dir,
                    project_name: meta.name.clone(),
                    deploy,
                };
                // A `ci.deploy` block emits two siblings from the same
                // spec: `<name>-deploy.yml` (verify/preview/deploy) and
                // `<name>-cleanup.yml` (delete the preview Worker on PR
                // close). Generating cleanup alongside deploy means a
                // new Worker app can't silently leak previews (#719).
                let deploy_workflow = super::worker_deploy_workflow::build(&spec);
                items.push((
                    PathBuf::from(".github/workflows").join(format!("{}-deploy.yml", meta.name)),
                    workflow::emit(&deploy_workflow),
                ));
                let cleanup_workflow = super::worker_cleanup_workflow::build(&spec);
                items.push((
                    PathBuf::from(".github/workflows").join(format!("{}-cleanup.yml", meta.name)),
                    workflow::emit(&cleanup_workflow),
                ));
            }
        }
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));

    if !build_specs.is_empty() {
        let main_workflow = super::main_workflow::build(&build_specs);
        items.push((
            PathBuf::from(".github/workflows/main.yaml"),
            workflow::emit(&main_workflow),
        ));
    }

    if check {
        check_drift(&items);
    } else {
        write_all(&items);
    }
}

fn build_apply_workflow(name: &str, project_dir: &Path, apply: &CiApply) -> Workflow {
    // `discover` joins paths against `Path::new(".")`, producing
    // `./<slot>/<name>`. Strip the leading `./` so generated globs and
    // inputs read as `infra/<name>/**` rather than `./infra/<name>/**`.
    let project_dir_str = project_dir
        .strip_prefix("./")
        .unwrap_or(project_dir)
        .to_string_lossy()
        .to_string();
    // Path filter watches the project, the wrapper itself, and the
    // reusable workflow it calls — so workflow refactors re-apply on
    // the next main-merge.
    let reusable_filename = Path::new(&apply.reusable_workflow)
        .file_name()
        .and_then(|n| n.to_str())
        .expect("reusable_workflow has a filename")
        .to_string();

    let mut with = BTreeMap::new();
    with.insert(
        "project_dir".to_string(),
        serde_yaml_ng::Value::String(project_dir_str.clone()),
    );

    let mut secrets = BTreeMap::new();
    secrets.insert(
        "OP_SERVICE_ACCOUNT_TOKEN".to_string(),
        "${{ secrets.OP_SERVICE_ACCOUNT_TOKEN }}".to_string(),
    );

    let mut jobs = IndexMap::new();
    jobs.insert(
        "apply".to_string(),
        Job::ReusableCall(ReusableCall {
            needs: super::workflow::Needs::Multiple(vec![]),
            if_: None,
            uses: apply.reusable_workflow.clone(),
            with,
            secrets,
        }),
    );

    Workflow {
        name: format!("{name} apply"),
        on: On {
            pull_request: None,
            push: Some(PushTrigger {
                branches: vec!["main".to_string()],
                paths: vec![
                    format!("{project_dir_str}/**"),
                    format!(".github/workflows/{name}-apply.yml"),
                    format!(".github/workflows/{reusable_filename}"),
                ],
            }),
            workflow_dispatch: Some(Empty),
        },
        permissions: Some(Permissions {
            id_token: Some(PermissionLevel::Write),
            contents: Some(PermissionLevel::Read),
            pull_requests: None,
        }),
        concurrency: Some(Concurrency {
            group: format!("{name}-apply"),
            cancel_in_progress: CancelInProgress::Bool(false),
        }),
        jobs,
    }
}

fn read_meta(project_dir: &Path) -> Result<ProjectMeta, String> {
    let project_file = project_dir.join("project.cue");
    if !project_file.exists() {
        return Err("missing project.cue".into());
    }
    let output = schema_check::cue_project(&["export"], &project_file)
        .map_err(|e| format!("failed to spawn cue: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("cue export failed: {stderr}"));
    }
    serde_json::from_slice::<ProjectMeta>(&output.stdout)
        .map_err(|e| format!("could not parse cue export output: {e}"))
}

fn write_all(items: &[(PathBuf, String)]) {
    if items.is_empty() {
        println!("{YELLOW}{BOLD}generate-workflows:{RESET} no projects declare ci blocks");
        return;
    }
    for (path, content) in items {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!(
                    "{RED}{BOLD}error:{RESET} creating {}: {e}",
                    parent.display()
                );
                std::process::exit(1);
            }
        }
        if let Err(e) = std::fs::write(path, content) {
            eprintln!("{RED}{BOLD}error:{RESET} writing {}: {e}", path.display());
            std::process::exit(1);
        }
        println!("  {GREEN}{BOLD}wrote{RESET}    {}", path.display());
    }
    println!(
        "\n{GREEN}{BOLD}generate-workflows:{RESET} wrote {} files",
        items.len()
    );
}

fn check_drift(items: &[(PathBuf, String)]) {
    let mut drift = Vec::new();
    for (path, expected) in items {
        match std::fs::read_to_string(path) {
            Ok(actual) if actual == *expected => {
                println!("  {GREEN}{BOLD}ok{RESET}       {}", path.display());
            }
            Ok(_) => {
                println!("  {RED}{BOLD}DRIFT{RESET}    {}", path.display());
                drift.push(path.clone());
            }
            Err(_) => {
                println!("  {RED}{BOLD}MISSING{RESET}  {}", path.display());
                drift.push(path.clone());
            }
        }
    }
    println!();
    if !drift.is_empty() {
        eprintln!(
            "{RED}{BOLD}generate-workflows --check failed{RESET}: {} file(s) drifted",
            drift.len()
        );
        eprintln!("{YELLOW}Run `shaka ci generate-workflows` and commit the result.{RESET}");
        eprintln!(
            "{DIM}drifted: {}{RESET}",
            drift
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        std::process::exit(1);
    }
    if items.is_empty() {
        println!(
            "{GREEN}{BOLD}generate-workflows --check passed{RESET} (no projects declare ci blocks)"
        );
    } else {
        println!("{GREEN}{BOLD}generate-workflows --check passed{RESET}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_fixture() -> Workflow {
        build_apply_workflow(
            "cloudflare-dns",
            Path::new("./infra/cloudflare-dns"),
            &CiApply {
                reusable_workflow: "./.github/workflows/tf-apply.yml".to_string(),
            },
        )
    }

    #[test]
    fn apply_workflow_strips_leading_dot_slash_from_paths() {
        let yaml = workflow::emit(&apply_fixture());
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).expect("valid YAML");
        let paths = parsed["on"]["push"]["paths"]
            .as_sequence()
            .expect("push paths");
        // Globs must read as `infra/...`, never `./infra/...`.
        assert!(paths
            .iter()
            .any(|p| p.as_str() == Some("infra/cloudflare-dns/**")));
        for p in paths {
            let s = p.as_str().unwrap();
            assert!(!s.starts_with("./"), "path leaked a ./ prefix: {s}");
        }
    }

    #[test]
    fn apply_workflow_watches_itself_and_the_reusable_workflow() {
        let yaml = workflow::emit(&apply_fixture());
        // A refactor of either the wrapper or the reusable workflow it
        // calls should re-apply on the next main-merge.
        assert!(yaml.contains(".github/workflows/cloudflare-dns-apply.yml"));
        assert!(yaml.contains(".github/workflows/tf-apply.yml"));
    }

    #[test]
    fn apply_workflow_requests_oidc_and_serializes_applies() {
        let yaml = workflow::emit(&apply_fixture());
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed["name"].as_str(), Some("cloudflare-dns apply"));
        assert_eq!(parsed["permissions"]["id-token"].as_str(), Some("write"));
        // Applies must not cancel each other mid-flight.
        assert_eq!(
            parsed["concurrency"]["cancel-in-progress"].as_bool(),
            Some(false)
        );
        assert_eq!(
            parsed["concurrency"]["group"].as_str(),
            Some("cloudflare-dns-apply")
        );
    }

    #[test]
    fn read_meta_errors_when_project_cue_is_absent() {
        let dir = std::env::temp_dir().join("shaka-ci-generate-no-cue");
        // No project.cue here — read_meta should short-circuit before
        // ever spawning `cue`.
        let err = match read_meta(&dir) {
            Err(e) => e,
            Ok(_) => panic!("missing project.cue must error"),
        };
        assert!(err.contains("missing project.cue"), "{err}");
    }
}
