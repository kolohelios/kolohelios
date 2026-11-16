//! Exercise the shipped CLI without allowing any remote generation.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use blogctl::cli::{Cli, Command as CliCommand};
use blogctl::commands::novelai::NovelaiAction;
use clap::Parser;

use blogctl::writing::Store;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .import_experiment(&fixture("writing/valid/chapter.json"))
        .unwrap();
    (dir, store)
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_blogctl"));
    command.arg("novelai").env_remove("NOVELAI_API_TOKEN");
    command
}

#[test]
fn request_preview_needs_no_valid_credentials_and_writes_nothing() {
    let (dir, store) = setup();
    for variant in ["workshop", "woodland", "ordinary"] {
        let output = command()
            .args([
                "generate",
                "synthetic-chapter",
                "--variant",
                variant,
                "--dry-run",
                "--model",
                "model-a",
                "--workdir",
            ])
            .arg(dir.path())
            .env("NOVELAI_API_TOKEN", "invalid token with spaces")
            .env("PATH", "/no/credential/tools")
            .current_dir(std::env::temp_dir())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let preview: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(preview["model"].as_str(), Some("model-a"));
        assert_eq!(preview["n"].as_u64(), Some(1));
        assert_eq!(preview["best_of"].as_u64(), Some(1));
        assert_eq!(preview["stream"].as_bool(), Some(false));
        assert_eq!(preview["max_tokens"].as_u64(), Some(600));
        let (system, user) = store
            .experiment("synthetic-chapter")
            .unwrap()
            .prepare(variant)
            .unwrap()
            .editorial_messages();
        assert_eq!(preview["messages"][0]["role"], "system");
        assert_eq!(preview["messages"][0]["content"], system);
        assert_eq!(preview["messages"][1]["role"], "user");
        assert_eq!(preview["messages"][1]["content"], user);
        assert_eq!(preview["enable_thinking"], false);
        assert!(preview.get("prompt").is_none());
    }
    assert!(!dir.path().join(".writing/candidates").exists());
    assert!(!dir.path().join(".writing/provider-responses").exists());
}

#[test]
fn legacy_transport_is_explicit_and_preview_preserves_the_old_request() {
    let (dir, _store) = setup();
    let output = command()
        .args([
            "generate",
            "synthetic-chapter",
            "--variant",
            "workshop",
            "--model",
            "model-a",
            "--transport",
            "completion",
            "--dry-run",
            "--workdir",
        ])
        .arg(dir.path())
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert!(output.status.success());
    let preview: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected: serde_json::Value =
        blogctl::writing::read_json(&fixture("novelai/workshop-request.json")).unwrap();
    assert_eq!(preview, expected);
    assert!(!dir.path().join(".writing/candidates").exists());
}

#[test]
fn bounded_parameters_preview_exactly_and_invalid_values_fail_locally() {
    let (dir, _) = setup();
    let output = command()
        .args([
            "generate",
            "synthetic-chapter",
            "--variant",
            "workshop",
            "--model",
            "model-a",
            "--max-tokens",
            "1200",
            "--temperature",
            "0.25",
            "--dry-run",
            "--workdir",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let preview: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(preview["max_tokens"], 1200);
    assert_eq!(preview["temperature"], 0.25);

    for flags in [
        ["--max-tokens", "0"],
        ["--max-tokens", "8193"],
        ["--temperature", "-0.1"],
        ["--temperature", "2.1"],
        ["--temperature", "NaN"],
    ] {
        let output = command()
            .args([
                "generate",
                "synthetic-chapter",
                "--variant",
                "workshop",
                "--model",
                "model-a",
                "--workdir",
            ])
            .arg(dir.path())
            .args(flags)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!dir.path().join(".writing/candidates").exists());
    }
}

#[test]
fn invalid_lineage_fails_before_credentials_network_or_writes() {
    let (dir, _) = setup();
    let output = command()
        .args([
            "generate",
            "synthetic-chapter",
            "--variant",
            "workshop",
            "--model",
            "model-a",
            "--artifact-kind",
            "draft",
            "--parent",
            "synthetic-chapter/missing-parent",
            "--workdir",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("candidate not found"));
    assert!(!error.contains("NOVELAI_API_TOKEN"));
    assert!(!dir.path().join(".writing/candidates").exists());
}

#[test]
fn missing_credentials_fail_without_network_or_sensitive_output() {
    let output = command().arg("models").output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("NOVELAI_API_TOKEN is missing"));
    assert!(error.contains("1Password"));
    let (dir, _) = setup();
    let output = command()
        .args([
            "generate",
            "synthetic-chapter",
            "--variant",
            "workshop",
            "--model",
            "model-a",
            "--workdir",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("NOVELAI_API_TOKEN is missing"));
    assert!(!dir.path().join(".writing/candidates").exists());
}

#[test]
fn invalid_or_unresolved_credentials_fail_without_echoing_values() {
    for token in ["invalid token marker", "op://Private/NovelAI/credential"] {
        let output = command()
            .arg("models")
            .env("NOVELAI_API_TOKEN", token)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("NOVELAI_API_TOKEN"));
        assert!(!error.contains(token));
    }
}

#[test]
fn generate_runs_by_default_and_preview_is_explicit() {
    let args = [
        "blogctl",
        "novelai",
        "generate",
        "synthetic-chapter",
        "--variant",
        "workshop",
        "--model",
        "model-a",
        "--workdir",
        "/private/test",
    ];
    let parsed = Cli::try_parse_from(args).unwrap();
    assert!(
        matches!(parsed.command, CliCommand::Novelai(args) if matches!(args.action, NovelaiAction::Generate { dry_run: false, .. }))
    );
    let parsed = Cli::try_parse_from(args.into_iter().chain(["--dry-run"])).unwrap();
    assert!(
        matches!(parsed.command, CliCommand::Novelai(args) if matches!(args.action, NovelaiAction::Generate { dry_run: true, .. }))
    );
}

#[test]
fn unknown_variants_missing_models_and_obsolete_flags_fail() {
    let (dir, _) = setup();
    for args in [
        vec!["--variant", "missing", "--model", "model-a"],
        vec!["--variant", "workshop"],
        vec!["--variant", "workshop", "--model", "model-a", "--yes"],
        vec!["--variant", "workshop", "--model", "model-a", "--run"],
    ] {
        let output = command()
            .args(["generate", "synthetic-chapter", "--workdir"])
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
    assert!(!dir.path().join(".writing/candidates").exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
