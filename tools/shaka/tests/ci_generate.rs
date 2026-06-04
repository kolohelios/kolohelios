//! Integration tests for `shaka ci generate-workflows` and
//! `shaka ci audit-workflows`.
//!
//! Both subcommands `cue export` every discovered `project.cue` and
//! touch `.github/workflows/`, so they need a real temp repo with a
//! `cue.mod` (planted by `common::write_test_module`) and at least one
//! project carrying a `ci.build` block. We stage `apps/widget` from
//! `tests/fixtures/ci-generate/`, then drive the binary against that
//! tempdir as cwd — exit code plus the on-disk workflow file are the
//! contract.

use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

const SHAKA_BIN: &str = env!("CARGO_BIN_EXE_shaka");

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ci-generate")
}

/// Stage `apps/widget` (a buildable rust-cli) into a fresh temp repo
/// with a resolvable cue module, returning the repo root.
fn staged_repo() -> tempfile::TempDir {
    let root = tempfile::TempDir::new().unwrap();
    common::write_test_module(root.path());

    let dst = root.path().join("apps/widget");
    std::fs::create_dir_all(&dst).unwrap();
    // The fixture carries a `.fixture` suffix so the source tree never
    // holds a stray `project.cue` (rejected by schema-check, #95).
    std::fs::copy(
        fixtures_root().join("widget/project.cue.fixture"),
        dst.join("project.cue"),
    )
    .unwrap();

    root
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(SHAKA_BIN)
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn shaka")
}

#[test]
fn generate_workflows_emits_main_yaml_for_a_build_project() {
    let repo = staged_repo();
    let out = run(repo.path(), &["ci", "generate-workflows"]);
    assert!(
        out.status.success(),
        "generate-workflows failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let main_yaml = repo.path().join(".github/workflows/main.yaml");
    let content = std::fs::read_to_string(&main_yaml).expect("main.yaml written");
    // The widget's ci.build block becomes a `build-widget` job that
    // publishes to its declared FlakeHub name.
    assert!(content.contains("build-widget:"), "{content}");
    assert!(content.contains("kolohelios/widget"), "{content}");
}

#[test]
fn generate_workflows_check_passes_after_generating() {
    let repo = staged_repo();
    // First write, then `--check` must agree byte-for-byte.
    assert!(run(repo.path(), &["ci", "generate-workflows"])
        .status
        .success());
    let out = run(repo.path(), &["ci", "generate-workflows", "--check"]);
    assert!(
        out.status.success(),
        "--check drifted right after generate: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn generate_workflows_check_detects_drift() {
    let repo = staged_repo();
    assert!(run(repo.path(), &["ci", "generate-workflows"])
        .status
        .success());
    // Corrupt the generated file; `--check` must now fail.
    let main_yaml = repo.path().join(".github/workflows/main.yaml");
    std::fs::write(&main_yaml, "name: tampered\n").unwrap();
    let out = run(repo.path(), &["ci", "generate-workflows", "--check"]);
    assert!(!out.status.success(), "drift should fail --check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("DRIFT") || combined.contains("drifted"),
        "{combined}"
    );
}

#[test]
fn audit_workflows_accepts_a_generated_main_yaml() {
    let repo = staged_repo();
    assert!(run(repo.path(), &["ci", "generate-workflows"])
        .status
        .success());
    let out = run(repo.path(), &["ci", "audit-workflows"]);
    assert!(
        out.status.success(),
        "audit-workflows rejected a generated file: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn audit_workflows_flags_an_unaccounted_workflow() {
    let repo = staged_repo();
    let wf_dir = repo.path().join(".github/workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    // A hand-dropped workflow that is neither generated nor allowlisted.
    std::fs::write(wf_dir.join("rogue.yml"), "name: rogue\n").unwrap();
    let out = run(repo.path(), &["ci", "audit-workflows"]);
    assert!(
        !out.status.success(),
        "rogue workflow should fail the audit"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("rogue.yml"), "{combined}");
}
