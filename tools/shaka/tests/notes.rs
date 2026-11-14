//! Offline CLI/credential-boundary coverage; no 1Password or GitHub calls.
#![cfg(unix)]
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/notes")
}
fn run(scenario: &str, args: &[&str]) -> Output {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let bash = Command::new("bash")
        .args(["-c", "command -v bash"])
        .output()
        .unwrap();
    let bash = String::from_utf8(bash.stdout).unwrap();
    for tool in ["op", "curl", "gh"] {
        let script = std::fs::read_to_string(fixtures().join(format!("{tool}.sh"))).unwrap();
        let path = bin.join(tool);
        std::fs::write(
            &path,
            script.replacen("#!/usr/bin/env bash", &format!("#!{}", bash.trim()), 1),
        )
        .unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_shaka"));
    command
        .args(["notes", args[0], "--project"])
        .arg(fixtures().join("project"))
        .args(&args[1..])
        .env("PATH", path)
        .env("NOTES_TEST_SCENARIO", scenario)
        .env("NOTES_TEST_COUNTER", temp.path().join("counter"))
        .env("NOTES_TEST_FIXTURES", fixtures());
    command.output().unwrap()
}
#[test]
fn clean_stale_paginated_and_repaired_scans() {
    for (scenario, args, success, count) in [
        ("clean", vec!["verify"], true, 1),
        ("stale", vec!["verify"], false, 1),
        ("pagination", vec!["verify"], true, 2),
        ("repair", vec!["verify", "--repair", "--wait"], true, 1),
    ] {
        let output = run(scenario, &args);
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["checked"], count);
        assert_eq!(report["clean"], success);
    }
}
#[test]
fn failures_withhold_credentials_and_insecure_urls_are_rejected() {
    for args in [
        vec!["verify"],
        vec!["verify", "--url", "http://not-secure.example"],
    ] {
        let output = run("failure", &args);
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.contains("test-only-session-secret"));
        assert!(!String::from_utf8(output.stdout)
            .unwrap()
            .contains("session="));
    }
}
#[test]
fn hooks_create_or_update_without_exposing_the_session_key() {
    for scenario in ["hook-create", "hook-update"] {
        let output = run(scenario, &["webhook-install"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8(output.stdout)
            .unwrap()
            .contains("test-only-session-secret"));
    }
}
#[test]
fn search_uses_the_authenticated_api() {
    for args in [
        vec!["search", "stable prose"],
        vec!["search", "stable prose", "--semantic"],
    ] {
        let output = run("search", &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["hits"][0]["id"], "target");
    }
}
