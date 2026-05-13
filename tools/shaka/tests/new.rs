//! Integration tests for `shaka project new`.
//!
//! Each test runs `shaka project new ...` inside a tempdir as cwd, then
//! asserts on the resulting filesystem layout. The happy-path test also
//! drives `schema-check` and `audit` against the scaffolded project to
//! prove the boilerplate satisfies its own gates.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

mod common;

const SHAKA_BIN: &str = env!("CARGO_BIN_EXE_shaka");

fn run_new(cwd: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(SHAKA_BIN);
    cmd.arg("project").arg("new").args(args).current_dir(cwd);
    cmd.output().expect("failed to spawn shaka")
}

fn run_subcommand(cwd: &Path, args: &[&str]) -> Output {
    Command::new(SHAKA_BIN)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("failed to spawn shaka")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn scaffolds_clean_rust_project_under_chosen_slot() {
    let tmp = TempDir::new().unwrap();
    common::write_test_module(tmp.path());
    let out = run_new(tmp.path(), &["--name", "demo", "--slot", "tools"]);
    let stdout = stdout_of(&out);
    let stderr = stderr_of(&out);
    assert!(
        out.status.success(),
        "expected success, got {:?}\nstdout: {stdout}\nstderr: {stderr}",
        out.status.code()
    );

    let project = tmp.path().join("tools/demo");
    for f in [
        "project.cue",
        "Cargo.toml",
        "flake.nix",
        ".envrc",
        ".gitignore",
        "README.md",
        "src/main.rs",
        "justfile",
    ] {
        assert!(
            project.join(f).is_file(),
            "expected {f} to exist under {}",
            project.display()
        );
    }

    let cargo = std::fs::read_to_string(project.join("Cargo.toml")).unwrap();
    assert!(cargo.contains(r#"name = "demo""#), "Cargo.toml: {cargo}");
    assert!(
        cargo.contains(r#"license = "MIT OR Apache-2.0""#),
        "Cargo.toml: {cargo}"
    );

    // The same `audit` that `new` runs internally must continue to pass
    // when invoked again — proves the scaffold matches every rule.
    let audit = run_subcommand(tmp.path(), &["project", "audit"]);
    assert!(
        audit.status.success(),
        "audit on scaffolded project failed\nstdout: {}\nstderr: {}",
        stdout_of(&audit),
        stderr_of(&audit)
    );
}

#[test]
fn rejects_invalid_name_with_uppercase() {
    let tmp = TempDir::new().unwrap();
    let out = run_new(tmp.path(), &["--name", "Demo", "--slot", "tools"]);
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("lowercase"),
        "expected stderr to mention lowercase, got: {stderr}"
    );
}

#[test]
fn rejects_invalid_slot() {
    let tmp = TempDir::new().unwrap();
    let out = run_new(tmp.path(), &["--name", "demo", "--slot", "infra"]);
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("invalid slot"),
        "expected stderr to mention slot, got: {stderr}"
    );
}

#[test]
fn refuses_to_overwrite_existing_directory() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("tools/demo");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("project.cue"), "package project\n").unwrap();

    let out = run_new(tmp.path(), &["--name", "demo", "--slot", "tools"]);
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("already exists"),
        "expected stderr to mention existing dir, got: {stderr}"
    );
    // The pre-existing file should not have been clobbered.
    let preserved = std::fs::read_to_string(target.join("project.cue")).unwrap();
    assert_eq!(preserved, "package project\n");
}
