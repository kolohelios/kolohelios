//! Integration tests for `shaka repo send` from inside a jj workspace.
//!
//! Verifies that `repo send --dry-run` resolves the correct bookmark when
//! invoked from a non-default workspace.  We use `--dry-run` so no actual
//! push or `gh` call is made.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const SHAKA: &str = env!("CARGO_BIN_EXE_shaka");

// ── helpers ──────────────────────────────────────────────────────────────────

fn jj(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new("jj")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("failed to spawn jj")
}

fn jj_ok(cwd: &Path, args: &[&str]) {
    let out = jj(cwd, args);
    assert!(
        out.status.success(),
        "jj {} failed\nstdout: {}\nstderr: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn shaka(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(SHAKA)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("failed to spawn shaka")
}

/// Initialise a colocated jj repo at `repo` with minimal user config so jj
/// does not fall back to the host's global config.
fn setup_repo(repo: &Path) {
    jj_ok(repo, &["git", "init", "--colocate"]);
    jj_ok(
        repo,
        &["config", "set", "--repo", "user.email", "test@test.test"],
    );
    jj_ok(repo, &["config", "set", "--repo", "user.name", "Test"]);
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// `shaka repo send --dry-run --bookmark <name>` invoked from a non-default
/// workspace prints the expected `would run:` lines that reference the
/// bookmark and does not attempt any actual push or PR creation.
#[test]
fn dry_run_from_non_default_workspace() {
    let parent = TempDir::new().expect("tempdir");

    // Set up the primary (default) workspace.
    let default_repo = parent.path().join("repo");
    std::fs::create_dir(&default_repo).unwrap();
    setup_repo(&default_repo);

    // Give the default workspace a described, non-empty commit so it is not
    // empty (required: send rejects an empty @).
    jj_ok(
        &default_repo,
        &["describe", "-m", "feat(shaka): default workspace work"],
    );

    // Create a sibling workspace at a known path.
    let ws_path = parent.path().join("repo-extra");
    jj_ok(
        &default_repo,
        &[
            "workspace",
            "add",
            "--name",
            "extra",
            ws_path.to_str().unwrap(),
        ],
    );

    // Describe the workspace's @ so repo send has something to work with.
    jj_ok(
        &ws_path,
        &["describe", "-m", "fix(shaka): repair from workspace"],
    );

    // Verify we really are resolving @ in the workspace (not default).
    let desc_out = jj(
        &ws_path,
        &["log", "-r", "@", "-T", "description", "--no-graph"],
    );
    let desc = String::from_utf8_lossy(&desc_out.stdout);
    assert!(
        desc.contains("fix(shaka): repair from workspace"),
        "unexpected @ description in workspace: {desc}",
    );

    // Run `shaka repo send --dry-run --bookmark <name>` from the workspace.
    let bookmark = "fix/shaka-repair-from-workspace";
    let out = shaka(
        &ws_path,
        &["repo", "send", "--dry-run", "--bookmark", bookmark],
    );
    assert!(
        out.status.success(),
        "shaka repo send --dry-run failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let stdout = String::from_utf8_lossy(&out.stdout);

    // Dry-run output must reference the correct bookmark for the workspace @.
    assert!(
        stdout.contains(&format!("jj bookmark set {bookmark}")),
        "expected bookmark set in dry-run output; got: {stdout}",
    );
    assert!(
        stdout.contains(&format!("jj git push --allow-new --bookmark {bookmark}")),
        "expected push line in dry-run output; got: {stdout}",
    );
}

/// Sending from inside a workspace without a description should fail with a
/// clear error — same behaviour as the default workspace.
#[test]
fn send_fails_without_description_in_workspace() {
    let parent = TempDir::new().expect("tempdir");

    let default_repo = parent.path().join("repo");
    std::fs::create_dir(&default_repo).unwrap();
    setup_repo(&default_repo);

    let ws_path = parent.path().join("repo-nodesc");
    jj_ok(
        &default_repo,
        &[
            "workspace",
            "add",
            "--name",
            "nodesc",
            ws_path.to_str().unwrap(),
        ],
    );

    // @ in the new workspace has no description — send should reject it.
    let out = shaka(
        &ws_path,
        &["repo", "send", "--dry-run", "--bookmark", "fix/whatever"],
    );
    assert!(
        !out.status.success(),
        "expected shaka repo send to fail with no description",
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no description"),
        "expected 'no description' error; got: {stderr}",
    );
}
