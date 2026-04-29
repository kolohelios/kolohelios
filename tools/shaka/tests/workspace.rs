//! Integration tests for `shaka workspace`.
//!
//! Sets up a real jj-colocated repo in a tempdir and exercises the
//! `new|list|forget` lifecycle end-to-end via the compiled binary.
//! `--force` is used on `forget` so the test doesn't depend on a
//! `main@origin` revset (none exists in a freshly-init'd repo).

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const SHAKA: &str = env!("CARGO_BIN_EXE_shaka");

fn jj_init_colocated(repo: &Path) {
    Command::new("jj")
        .args(["git", "init", "--colocate"])
        .current_dir(repo)
        .status()
        .expect("failed to spawn jj")
        .success()
        .then_some(())
        .expect("jj git init failed");
}

fn shaka(repo: &Path, args: &[&str]) -> std::process::Output {
    Command::new(SHAKA)
        .args(args)
        .current_dir(repo)
        .output()
        .expect("failed to spawn shaka")
}

fn assert_success(out: &std::process::Output, ctx: &str) {
    assert!(
        out.status.success(),
        "{ctx}: shaka exited {:?}\nstdout: {}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn new_creates_workspace_and_directory() {
    let parent = TempDir::new().expect("tempdir");
    let repo = parent.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    jj_init_colocated(&repo);

    let out = shaka(&repo, &["workspace", "new", "feat-foo"]);
    assert_success(&out, "workspace new");

    let workspace_path = parent.path().join("repo-feat-foo");
    assert!(
        workspace_path.is_dir(),
        "expected workspace dir at {}",
        workspace_path.display()
    );
    assert!(
        workspace_path.join(".jj").is_dir(),
        "expected .jj inside workspace dir"
    );

    let list = shaka(&repo, &["workspace", "list"]);
    assert_success(&list, "workspace list");
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("default"), "list missing default: {stdout}");
    assert!(
        stdout.contains("feat-foo"),
        "list missing feat-foo: {stdout}"
    );
}

#[test]
fn forget_removes_workspace_and_directory() {
    let parent = TempDir::new().expect("tempdir");
    let repo = parent.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    jj_init_colocated(&repo);

    let new_out = shaka(&repo, &["workspace", "new", "scratch"]);
    assert_success(&new_out, "workspace new");
    let workspace_path = parent.path().join("repo-scratch");
    assert!(workspace_path.is_dir());

    let forget_out = shaka(&repo, &["workspace", "forget", "scratch", "--force"]);
    assert_success(&forget_out, "workspace forget --force");

    assert!(
        !workspace_path.exists(),
        "expected workspace dir to be removed: {}",
        workspace_path.display()
    );

    let list = shaka(&repo, &["workspace", "list"]);
    assert_success(&list, "workspace list after forget");
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        !stdout.contains("scratch"),
        "scratch still listed after forget: {stdout}"
    );
}

#[test]
fn new_rejects_existing_path() {
    let parent = TempDir::new().expect("tempdir");
    let repo = parent.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    jj_init_colocated(&repo);

    let workspace_path = parent.path().join("repo-collide");
    std::fs::create_dir(&workspace_path).unwrap();

    let out = shaka(&repo, &["workspace", "new", "collide"]);
    assert!(!out.status.success(), "expected failure on path collision");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("already exists"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn forget_refuses_default_workspace() {
    let parent = TempDir::new().expect("tempdir");
    let repo = parent.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    jj_init_colocated(&repo);

    let out = shaka(&repo, &["workspace", "forget", "default"]);
    assert!(!out.status.success(), "expected failure on default forget");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("default"), "unexpected stderr: {stderr}");
}
