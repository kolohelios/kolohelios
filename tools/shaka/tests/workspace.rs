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
fn new_rejects_name_and_issue_together() {
    let parent = TempDir::new().expect("tempdir");
    let repo = parent.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    jj_init_colocated(&repo);

    // Passing both <name> and --issue must be rejected by clap before any gh
    // call is made, so no network access is needed.
    let out = shaka(&repo, &["workspace", "new", "feat-foo", "--issue", "42"]);
    assert!(
        !out.status.success(),
        "expected failure when both name and --issue are supplied"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflict"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn new_rejects_neither_name_nor_issue() {
    let parent = TempDir::new().expect("tempdir");
    let repo = parent.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    jj_init_colocated(&repo);

    let out = shaka(&repo, &["workspace", "new"]);
    assert!(
        !out.status.success(),
        "expected failure when neither name nor --issue are supplied"
    );
}

#[test]
fn status_lists_all_workspaces() {
    let parent = TempDir::new().expect("tempdir");
    let repo = parent.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    jj_init_colocated(&repo);

    // Create a second workspace
    let new_out = shaka(&repo, &["workspace", "new", "feat-bar"]);
    assert_success(&new_out, "workspace new feat-bar");

    // Human-readable output should mention both workspaces
    let status_out = shaka(&repo, &["workspace", "status"]);
    assert_success(&status_out, "workspace status");
    let stdout = String::from_utf8_lossy(&status_out.stdout);
    assert!(
        stdout.contains("default"),
        "status missing default: {stdout}"
    );
    assert!(
        stdout.contains("feat-bar"),
        "status missing feat-bar: {stdout}"
    );

    // --json output should be parseable JSON with both workspaces
    let json_out = shaka(&repo, &["workspace", "status", "--json"]);
    assert_success(&json_out, "workspace status --json");
    let json_str = String::from_utf8_lossy(&json_out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("workspace status --json is not valid JSON");
    let arr = parsed.as_array().expect("expected JSON array");
    assert!(
        arr.len() >= 2,
        "expected at least 2 workspaces in JSON, got {}: {json_str}",
        arr.len()
    );
    let names: Vec<&str> = arr.iter().filter_map(|v| v["name"].as_str()).collect();
    assert!(
        names.contains(&"default"),
        "JSON missing default workspace: {json_str}"
    );
    assert!(
        names.contains(&"feat-bar"),
        "JSON missing feat-bar workspace: {json_str}"
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

/// `workspace forget` should also remove the persisted issue link so the
/// `.shaka/workspaces/` directory does not accumulate stale entries.
/// Issue #164: the link is what `cleanup` consults post-`repo sync`.
#[test]
fn forget_removes_issue_link() {
    let parent = TempDir::new().expect("tempdir");
    let repo = parent.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    jj_init_colocated(&repo);

    let new_out = shaka(&repo, &["workspace", "new", "i42"]);
    assert_success(&new_out, "workspace new i42");

    // Simulate the link the `--issue` path would have written (we can't
    // exercise `--issue` in tests because it needs `gh` + network).
    let link_dir = repo.join(".shaka").join("workspaces");
    std::fs::create_dir_all(&link_dir).unwrap();
    let link_path = link_dir.join("i42.json");
    std::fs::write(&link_path, r#"{"issue":42}"#).unwrap();
    assert!(link_path.exists(), "fixture link not written");

    let forget_out = shaka(&repo, &["workspace", "forget", "i42", "--force"]);
    assert_success(&forget_out, "workspace forget i42 --force");

    assert!(
        !link_path.exists(),
        "expected issue link to be removed: {}",
        link_path.display()
    );
}

#[test]
fn cleanup_dry_run_does_not_remove_workspace() {
    // Verify that `cleanup --dry-run` exits successfully and leaves the
    // workspace directory intact. We cannot exercise the merged-PR path in
    // integration tests (that requires network + auth), but we can confirm
    // that the command handles a repo with no git remote gracefully (warns and
    // exits 0) and does not touch the workspace directory.
    let parent = TempDir::new().expect("tempdir");
    let repo = parent.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    jj_init_colocated(&repo);

    let new_out = shaka(&repo, &["workspace", "new", "feat-bar"]);
    assert_success(&new_out, "workspace new");
    let workspace_path = parent.path().join("repo-feat-bar");
    assert!(
        workspace_path.is_dir(),
        "workspace dir should exist before cleanup"
    );

    let cleanup_out = shaka(&repo, &["workspace", "cleanup", "--dry-run"]);
    assert_success(&cleanup_out, "workspace cleanup --dry-run");

    // Regardless of what was printed, the workspace directory must still exist.
    assert!(
        workspace_path.is_dir(),
        "cleanup --dry-run must not remove the workspace directory: {}",
        workspace_path.display()
    );
}
