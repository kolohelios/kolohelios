use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::project::schema_check;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

use super::workflow::{
    self, Concurrency, Empty, Job, On, PermissionLevel, Permissions, PushTrigger, ReusableCall,
    Workflow,
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
}

#[derive(Deserialize)]
struct CiApply {
    reusable_workflow: String,
}

pub fn run(check: bool) {
    let schema_path = match schema_check::write_schema() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} could not write schema: {e}");
            std::process::exit(1);
        }
    };

    let mut items: Vec<(PathBuf, String)> = Vec::new();
    for project_dir in schema_check::discover(Path::new(".")) {
        let meta = match read_meta(&schema_path, &project_dir) {
            Ok(m) => m,
            Err(e) => {
                eprintln!(
                    "{RED}{BOLD}error:{RESET} reading {}: {e}",
                    project_dir.display()
                );
                std::process::exit(1);
            }
        };
        if let Some(ci) = meta.ci {
            if let Some(apply) = ci.apply {
                let workflow = build_apply_workflow(&meta.name, &project_dir, &apply);
                let target =
                    PathBuf::from(".github/workflows").join(format!("{}-apply.yml", meta.name));
                items.push((target, workflow::emit(&workflow)));
            }
        }
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));

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
    with.insert("project_dir".to_string(), project_dir_str.clone());

    let mut secrets = BTreeMap::new();
    secrets.insert(
        "OP_SERVICE_ACCOUNT_TOKEN".to_string(),
        "${{ secrets.OP_SERVICE_ACCOUNT_TOKEN }}".to_string(),
    );

    let mut jobs = BTreeMap::new();
    jobs.insert(
        "apply".to_string(),
        Job::ReusableCall(ReusableCall {
            uses: apply.reusable_workflow.clone(),
            with,
            secrets,
        }),
    );

    Workflow {
        name: format!("{name} apply"),
        on: On {
            push: PushTrigger {
                branches: vec!["main".to_string()],
                paths: vec![
                    format!("{project_dir_str}/**"),
                    format!(".github/workflows/{name}-apply.yml"),
                    format!(".github/workflows/{reusable_filename}"),
                ],
            },
            workflow_dispatch: Empty,
        },
        permissions: Permissions {
            id_token: Some(PermissionLevel::Write),
            contents: Some(PermissionLevel::Read),
        },
        concurrency: Concurrency {
            group: format!("{name}-apply"),
            cancel_in_progress: false,
        },
        jobs,
    }
}

fn read_meta(schema_path: &Path, project_dir: &Path) -> Result<ProjectMeta, String> {
    let project_file = project_dir.join("project.cue");
    if !project_file.exists() {
        return Err("missing project.cue".into());
    }
    let output = Command::new("cue")
        .arg("export")
        .arg(schema_path)
        .arg(&project_file)
        .output()
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
