//! Integration tests for `shaka project audit`.
//!
//! Each test stages fixtures from `tests/fixtures/audit/<name>/` into a slot
//! (`apps/<name>`) inside a tempdir and runs the shaka binary against that
//! tempdir as cwd. Exit code + a substring of stdout/stderr are the contract.

use std::path::{Path, PathBuf};
use std::process::Command;

const SHAKA_BIN: &str = env!("CARGO_BIN_EXE_shaka");

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audit")
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

struct Staged {
    root: tempfile::TempDir,
}

impl Staged {
    fn new(fixtures: &[&str]) -> Self {
        let root = tempfile::TempDir::new().unwrap();
        let slot = root.path().join("apps");
        std::fs::create_dir_all(&slot).unwrap();
        for name in fixtures {
            let src = fixtures_root().join(name);
            let dst = slot.join(name);
            copy_dir(&src, &dst);
        }
        Staged { root }
    }

    fn run_audit(&self) -> std::process::Output {
        Command::new(SHAKA_BIN)
            .args(["project", "audit"])
            .current_dir(self.root.path())
            .output()
            .expect("failed to spawn shaka")
    }
}

fn stdout_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn clean_projects_pass() {
    let staged = Staged::new(&["clean-rust", "clean-infra"]);
    let out = staged.run_audit();
    let stdout = stdout_of(&out);
    assert!(
        out.status.success(),
        "expected success, got exit {:?}\nstdout: {stdout}",
        out.status.code()
    );
    assert!(stdout.contains("audit passed"), "stdout: {stdout}");
}

#[test]
fn missing_cue_is_top_level_failure() {
    let staged = Staged::new(&["missing-cue"]);
    let out = staged.run_audit();
    assert!(!out.status.success());
    let stdout = stdout_of(&out);
    assert!(stdout.contains("missing project.cue"), "stdout: {stdout}");
}

#[test]
fn missing_readme_fails_audit() {
    let staged = Staged::new(&["missing-readme"]);
    let out = staged.run_audit();
    assert!(!out.status.success());
    let stdout = stdout_of(&out);
    assert!(stdout.contains("readme-present"), "stdout: {stdout}");
}

#[test]
fn rust_project_without_tests_fails_audit() {
    let staged = Staged::new(&["missing-tests"]);
    let out = staged.run_audit();
    assert!(!out.status.success());
    let stdout = stdout_of(&out);
    assert!(stdout.contains("rust-has-tests"), "stdout: {stdout}");
}

#[test]
fn zero_coverage_threshold_fails_audit() {
    let staged = Staged::new(&["zero-coverage"]);
    let out = staged.run_audit();
    assert!(!out.status.success());
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("rust-coverage-threshold-nonzero"),
        "stdout: {stdout}"
    );
}

#[test]
fn wrong_license_fails_audit() {
    let staged = Staged::new(&["wrong-license"]);
    let out = staged.run_audit();
    assert!(!out.status.success());
    let stdout = stdout_of(&out);
    assert!(stdout.contains("rust-license-dual"), "stdout: {stdout}");
}

#[test]
fn rust_only_rules_skip_for_infra() {
    let staged = Staged::new(&["clean-infra"]);
    let out = staged.run_audit();
    let stdout = stdout_of(&out);
    assert!(out.status.success(), "stdout: {stdout}");
    // clean-infra has no src/, no tests/ — but rust-* rules don't apply to infra.
    assert!(stdout.contains("audit passed"), "stdout: {stdout}");
}

#[test]
fn flakehub_pinned_kolohelios_nix_passes_audit() {
    let staged = Staged::new(&["flake-flakehub-input"]);
    let out = staged.run_audit();
    let stdout = stdout_of(&out);
    assert!(out.status.success(), "stdout: {stdout}");
    assert!(stdout.contains("audit passed"), "stdout: {stdout}");
}

#[test]
fn path_pinned_kolohelios_nix_fails_audit() {
    let staged = Staged::new(&["flake-path-input"]);
    let out = staged.run_audit();
    assert!(!out.status.success());
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("kolohelios-nix-via-flakehub"),
        "stdout: {stdout}"
    );
}
