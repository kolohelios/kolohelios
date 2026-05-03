//! Integration tests for `shaka domain inventory`.
//!
//! Each test pairs a snapshot JSON fixture with a registry directory fixture
//! (real CUE files under `tests/fixtures/domain/registry/<case>/`) and asserts
//! exit code + diff content from the spawned shaka binary.

use std::path::{Path, PathBuf};
use std::process::Command;

const SHAKA_BIN: &str = env!("CARGO_BIN_EXE_shaka");

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/domain")
}

fn snapshot(name: &str) -> PathBuf {
    fixtures_root().join("snapshot").join(name)
}

fn registry(case: &str) -> PathBuf {
    fixtures_root().join("registry").join(case)
}

fn run_inventory(input: &Path, registry_dir: &Path) -> std::process::Output {
    Command::new(SHAKA_BIN)
        .args(["domain", "inventory", "--input"])
        .arg(input)
        .arg("--registry-dir")
        .arg(registry_dir)
        .output()
        .expect("failed to spawn shaka")
}

fn stdout_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn matching_registry_exits_zero() {
    let out = run_inventory(&snapshot("two-domains.json"), &registry("matching"));
    let stdout = stdout_of(&out);
    let stderr = stderr_of(&out);
    assert!(
        out.status.success(),
        "expected success, got {:?}\nstdout: {stdout}\nstderr: {stderr}",
        out.status.code()
    );
    assert!(stdout.contains("inventory matches"), "stdout: {stdout}");
}

#[test]
fn drifted_registry_exits_nonzero_with_diff() {
    let out = run_inventory(&snapshot("two-domains.json"), &registry("drifted"));
    let stdout = stdout_of(&out);
    assert!(!out.status.success(), "stdout: {stdout}");
    assert!(stdout.contains("inventory drift"), "stdout: {stdout}");
    // snapshot has alpha+beta; drifted registry has alpha+gamma.
    // Expect: + beta.example (in snapshot, missing from registry)
    //         - gamma.example (in registry, missing from snapshot)
    assert!(stdout.contains("+ beta.example"), "stdout: {stdout}");
    assert!(stdout.contains("- gamma.example"), "stdout: {stdout}");
    assert!(!stdout.contains("alpha.example"), "stdout: {stdout}");
}

#[test]
fn empty_registry_special_cases_with_message() {
    let out = run_inventory(&snapshot("two-domains.json"), &registry("empty"));
    let stdout = stdout_of(&out);
    let stderr = stderr_of(&out);
    assert!(!out.status.success(), "stdout: {stdout}\nstderr: {stderr}");
    assert!(stderr.contains("registry is empty"), "stderr: {stderr}");
    assert!(stdout.contains("+ alpha.example"), "stdout: {stdout}");
    assert!(stdout.contains("+ beta.example"), "stdout: {stdout}");
}

#[test]
fn missing_registry_dir_treated_as_empty() {
    let tmp = tempfile::TempDir::new().unwrap();
    let nonexistent = tmp.path().join("does-not-exist");
    let out = run_inventory(&snapshot("two-domains.json"), &nonexistent);
    let stderr = stderr_of(&out);
    assert!(!out.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("registry is empty"), "stderr: {stderr}");
}

#[test]
fn duplicate_registry_names_fail_loud() {
    let out = run_inventory(&snapshot("two-domains.json"), &registry("duplicate"));
    let stderr = stderr_of(&out);
    assert!(!out.status.success(), "stderr: {stderr}");
    assert!(
        stderr.contains("duplicate names in registry"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("alpha.example"), "stderr: {stderr}");
}

#[test]
fn duplicate_snapshot_names_fail_loud() {
    let out = run_inventory(&snapshot("duplicate.json"), &registry("matching"));
    let stderr = stderr_of(&out);
    assert!(!out.status.success(), "stderr: {stderr}");
    assert!(
        stderr.contains("duplicate names in snapshot"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("alpha.example"), "stderr: {stderr}");
}

#[test]
fn real_hover_snapshot_against_empty_registry_lists_all() {
    let snap = fixtures_root().join("hover/snapshot-2026-05-02.json");
    let out = run_inventory(&snap, &registry("empty"));
    let stdout = stdout_of(&out);
    assert!(!out.status.success(), "stdout: {stdout}");
    // 52 entries in the sanitized snapshot, all reported as adds.
    let add_lines = stdout.lines().filter(|l| l.contains("+ ")).count();
    assert_eq!(
        add_lines, 52,
        "expected 52 add lines, got {add_lines}\nstdout: {stdout}"
    );
}

#[test]
fn help_includes_refresh_snippet() {
    let out = Command::new(SHAKA_BIN)
        .args(["domain", "inventory", "--help"])
        .output()
        .expect("failed to spawn shaka");
    assert!(out.status.success());
    let stdout = stdout_of(&out);
    assert!(stdout.contains("REFRESH PROCEDURE"), "stdout: {stdout}");
    assert!(
        stdout.contains("hover.com/control_panel"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("Blob"), "stdout: {stdout}");
    assert!(stdout.contains("XProtect"), "stdout: {stdout}");
}
