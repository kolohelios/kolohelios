//! Exercise the shipped CLI without allowing any remote generation.
use std::fs;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
        assert_eq!(
            preview["prompt"].as_str().unwrap(),
            store
                .experiment("synthetic-chapter")
                .unwrap()
                .prepare(variant)
                .unwrap()
                .prompt()
        );
        if variant == "workshop" {
            let expected: serde_json::Value =
                blogctl::writing::read_json(&fixture("novelai/workshop-request.json")).unwrap();
            assert_eq!(preview, expected);
        }
    }
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
            "--run",
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
fn piped_generate_cannot_authorize_a_request_without_a_controlling_terminal() {
    let (dir, _) = setup();
    let mut command = command();
    command
        .args([
            "generate",
            "synthetic-chapter",
            "--variant",
            "workshop",
            "--model",
            "model-a",
            "--run",
            "--workdir",
        ])
        .arg(dir.path())
        .env("NOVELAI_API_TOKEN", "synthetic-test-token")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: setsid is an async-signal-safe syscall. Starting a new session
    // removes the inherited controlling terminal, regardless of the test runner.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    // The child may reject authorization and close stdin before this write.
    let _ = child.stdin.take().unwrap().write_all(b"GENERATE\n");
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("controlling terminal"));
    assert!(error.contains("no request was sent"));
    assert!(!error.contains("synthetic-test-token"));
    assert!(!dir.path().join(".writing/candidates").exists());
}

#[test]
fn unknown_variants_missing_models_and_unsupported_approval_flags_fail() {
    let (dir, _) = setup();
    for args in [
        vec!["--variant", "missing", "--model", "model-a"],
        vec!["--variant", "workshop"],
        vec!["--variant", "workshop", "--model", "model-a", "--yes"],
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
