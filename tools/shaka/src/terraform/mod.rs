//! `shaka terraform` — generate Terraform HCL from typed CUE.
//!
//! User CUE in `<project>/cue/*.cue` declares a module conforming to
//! `tools/shaka/schema/terraform-schema.cue`. `shaka terraform emit`
//! evaluates the CUE, walks it into an internal IR, renders HCL via the
//! `hcl-edit` AST, and normalizes the output with `tofu fmt`. The
//! generated `module.tf` is **committed** alongside the CUE source —
//! `shaka terraform check` is wired into `shaka preflight` to detect
//! drift, mirroring `shaka deploy generate-tf --check`.

pub mod cue;
pub mod emit;
pub mod ir;

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Subcommand;

use crate::project::schema_check::discover;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

#[derive(Subcommand)]
pub enum TerraformCommand {
    /// Emit Terraform HCL from a project's `cue/` directory.
    ///
    /// Writes `<project>/terraform/module.tf`, then runs `tofu fmt` to
    /// canonicalize. Output is deterministic — re-running reproduces
    /// the same bytes. The result is committed; drift is caught by
    /// `shaka terraform check` (also in `shaka preflight`).
    Emit {
        /// Path to the project directory (must contain `cue/`).
        #[arg(long)]
        project: PathBuf,
    },
    /// Walk every project, and for each one that has a `cue/` directory
    /// verify that the committed `<project>/terraform/module.tf` matches
    /// what emit would write. Exits non-zero on drift.
    Check,
}

pub fn run(cmd: TerraformCommand) {
    match cmd {
        TerraformCommand::Emit { project } => emit_cmd(&project),
        TerraformCommand::Check => check_cmd(),
    }
}

fn emit_cmd(project: &Path) {
    if !project.is_dir() {
        die(&format!(
            "project path is not a directory: {}",
            project.display()
        ));
    }
    let cue_dir = project.join("cue");
    let tf_dir = project.join("terraform");
    if !tf_dir.is_dir() {
        if let Err(e) = std::fs::create_dir_all(&tf_dir) {
            die(&format!("could not create {}: {e}", tf_dir.display()));
        }
    }

    let out_path = tf_dir.join("module.tf");
    match render_to_path(&cue_dir, &out_path) {
        Ok(()) => {
            println!(
                "{GREEN}{BOLD}wrote{RESET} {}  {DIM}(from {}){RESET}",
                out_path.display(),
                cue_dir.display()
            );
        }
        Err(e) => die(&e),
    }
}

fn check_cmd() {
    let projects: Vec<PathBuf> = discover(Path::new("."))
        .into_iter()
        .filter(|p| p.join("cue").is_dir())
        .collect();

    if projects.is_empty() {
        println!("{YELLOW}{BOLD}terraform check:{RESET} no projects with a `cue/` directory");
        return;
    }

    let mut drift_count = 0usize;

    for project in &projects {
        let cue_dir = project.join("cue");
        let on_disk = project.join("terraform").join("module.tf");

        match render_to_string(&cue_dir) {
            Ok(expected) => match std::fs::read_to_string(&on_disk) {
                Ok(actual) if actual == expected => {
                    println!("  {GREEN}{BOLD}ok{RESET}       {}", on_disk.display());
                }
                Ok(_) => {
                    println!("  {RED}{BOLD}DRIFT{RESET}    {}", on_disk.display());
                    drift_count += 1;
                }
                Err(_) => {
                    println!("  {RED}{BOLD}MISSING{RESET}  {}", on_disk.display());
                    drift_count += 1;
                }
            },
            Err(e) => {
                println!("  {RED}{BOLD}EMIT FAIL{RESET} {} — {e}", cue_dir.display());
                drift_count += 1;
            }
        }
    }

    println!();
    if drift_count > 0 {
        eprintln!("{RED}{BOLD}terraform check failed{RESET}: {drift_count} project(s) drifted");
        eprintln!(
            "{YELLOW}Run `shaka terraform emit --project <p>` for each drifted project and commit the result.{RESET}"
        );
        std::process::exit(1);
    }
    println!("{GREEN}{BOLD}terraform check passed{RESET}");
}

/// Run the cue → IR → hcl-edit → tofu-fmt pipeline, writing the result
/// to `out_path`. Returns `Ok(())` on success or `Err(reason)` with a
/// human-readable message.
fn render_to_path(cue_dir: &Path, out_path: &Path) -> Result<(), String> {
    let rendered = render_raw(cue_dir)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::write(out_path, rendered.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", out_path.display()))?;
    tofu_fmt(out_path)
}

/// Same pipeline as `render_to_path`, but lands the canonicalized
/// output in memory (via a temp file for `tofu fmt`) so callers can
/// compare it against an on-disk file without disturbing it.
fn render_to_string(cue_dir: &Path) -> Result<String, String> {
    let rendered = render_raw(cue_dir)?;
    let tmp = tempfile::Builder::new()
        .prefix("shaka-terraform-check-")
        .suffix(".tf")
        .tempfile()
        .map_err(|e| format!("could not create tempfile: {e}"))?;
    std::fs::write(tmp.path(), rendered.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", tmp.path().display()))?;
    tofu_fmt(tmp.path())?;
    std::fs::read_to_string(tmp.path())
        .map_err(|e| format!("could not read {}: {e}", tmp.path().display()))
}

fn render_raw(cue_dir: &Path) -> Result<String, String> {
    let json = cue::load(cue_dir).map_err(|e| format!("{e}"))?;
    let module = ir::from_json(&json).map_err(|e| format!("{e}"))?;
    Ok(emit::render(&module))
}

fn tofu_fmt(path: &Path) -> Result<(), String> {
    let status = Command::new("tofu").arg("fmt").arg(path).status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!(
            "tofu fmt {} exited with status {s}",
            path.display()
        )),
        Err(e) => Err(format!(
            "could not spawn tofu (is it on PATH? infra devshells provide it): {e}"
        )),
    }
}

fn die(msg: &str) -> ! {
    eprintln!("{RED}{BOLD}error:{RESET} {msg}");
    std::process::exit(1);
}
