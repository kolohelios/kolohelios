//! Real CUE/HCL fixtures, no Cloudflare/1Password access or VCS mutations.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("tokens")).unwrap();
    std::fs::copy(
        fixtures().join("cloudflare-token/valid/renewable.cue"),
        dir.path().join("tokens/deploy.cue"),
    )
    .unwrap();
    std::fs::copy(
        fixtures().join("token-renewal/main.tf"),
        dir.path().join("main.tf"),
    )
    .unwrap();
    dir
}

fn renew(dir: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shaka"))
        .args([
            "token",
            "cloudflare",
            "renew",
            "--registry-dir",
            "tokens",
            "--terraform-file",
            "main.tf",
        ])
        .args(extra)
        .current_dir(dir)
        .output()
        .unwrap()
}

#[test]
fn dry_run_is_deterministic_and_does_not_write() {
    let dir = setup();
    let before = std::fs::read(dir.path().join("tokens/deploy.cue")).unwrap();
    let tf_before = std::fs::read(dir.path().join("main.tf")).unwrap();
    let future = renew(dir.path(), &["--dry-run", "--now", "2027-01-27T00:00:00Z"]);
    assert!(
        future.status.success(),
        "{}",
        String::from_utf8_lossy(&future.stderr)
    );
    assert!(String::from_utf8_lossy(&future.stdout).contains("Would renew"));
    assert_eq!(
        std::fs::read(dir.path().join("tokens/deploy.cue")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read(dir.path().join("main.tf")).unwrap(),
        tf_before
    );
    let early = renew(dir.path(), &["--dry-run", "--now", "2027-01-01T00:00:00Z"]);
    assert!(early.status.success());
    assert!(String::from_utf8_lossy(&early.stdout).contains("No Cloudflare tokens"));
}

#[test]
fn mismatch_is_rejected_without_partial_writes() {
    let dir = setup();
    let path = dir.path().join("main.tf");
    let original = std::fs::read_to_string(&path)
        .unwrap()
        .replace("2027-02-10", "2027-02-09");
    std::fs::write(&path, &original).unwrap();
    let before = std::fs::read(dir.path().join("tokens/deploy.cue")).unwrap();
    let result = renew(dir.path(), &["--dry-run", "--now", "2027-01-27T00:00:00Z"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("does not match CUE"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    assert_eq!(
        std::fs::read(dir.path().join("tokens/deploy.cue")).unwrap(),
        before
    );
}

#[test]
fn renewal_edits_both_declarations_and_is_idempotent() {
    let dir = setup();
    // Make the fixture overdue regardless of the test runner's date.
    for name in ["tokens/deploy.cue", "main.tf"] {
        let path = dir.path().join(name);
        let source = std::fs::read_to_string(&path)
            .unwrap()
            .replace("2027-02-10", "2000-01-01");
        std::fs::write(path, source).unwrap();
    }
    let result = renew(dir.path(), &[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let cue = std::fs::read_to_string(dir.path().join("tokens/deploy.cue")).unwrap();
    let tf = std::fs::read_to_string(dir.path().join("main.tf")).unwrap();
    assert!(!cue.contains("2000-01-01"));
    assert!(!tf.contains("2000-01-01"));
    assert!(tf.contains("keep-me"));
    assert!(tf.contains("Renewal must preserve"));
    let again = renew(dir.path(), &[]);
    assert!(again.status.success());
    assert!(String::from_utf8_lossy(&again.stdout).contains("No Cloudflare tokens"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("main.tf")).unwrap(),
        tf
    );
}

#[test]
fn verification_keeps_credentials_out_of_output_and_arguments() {
    use std::os::unix::fs::PermissionsExt;
    let dir = setup();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let bash = Command::new("bash")
        .args(["-c", "command -v bash"])
        .output()
        .unwrap();
    assert!(bash.status.success());
    let bash = String::from_utf8(bash.stdout).unwrap();
    for name in ["op", "curl"] {
        let path = bin.join(name);
        // Nix's build sandbox has no /usr/bin/env. Install the real
        // fixture with the devshell's absolute interpreter path.
        let source =
            std::fs::read_to_string(fixtures().join(format!("token-renewal/{name}.sh"))).unwrap();
        std::fs::write(&path, source.replacen("/usr/bin/env bash", bash.trim(), 1)).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    // Fixed future response keeps the success path independent of wall time.
    for (response, curl_failure, success) in [
        ("valid/later.json", false, true),
        ("invalid/unauthorized.json", false, false),
        ("invalid/disabled.json", false, false),
        ("valid/active.json", true, false),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_shaka"))
            .args(["token", "cloudflare", "verify", "--registry-dir", "tokens"])
            .env("PATH", &path)
            .env(
                "VERIFY_RESPONSE_FILE",
                fixtures().join(format!("token-renewal/verification/{response}")),
            )
            .env("VERIFY_CURL_FAILURE", if curl_failure { "1" } else { "0" })
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(
            result.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!String::from_utf8_lossy(&result.stdout).contains("test-credential-do-not-log"));
        assert!(!String::from_utf8_lossy(&result.stderr).contains("test-credential-do-not-log"));
    }
}
