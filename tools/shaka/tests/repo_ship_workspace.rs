//! Integration tests for `shaka repo ship` from inside a jj workspace.
//!
//! Verifies that `repo ship --dry-run` resolves the correct bookmark when
//! invoked from a non-default workspace and that its dry-run preview now
//! delegates the push/PR steps to `repo send` (closing the race fixed by
//! #287).

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use tempfile::TempDir;

const SHAKA: &str = env!("CARGO_BIN_EXE_shaka");

// ── helpers ──────────────────────────────────────────────────────────────────

/// A writable `HOME` for every spawned `jj`/`git`/`shaka` process.
///
/// The nix sandbox runs the build's check phase with `HOME=/homeless-shelter`
/// on a read-only filesystem. `jj` writes per-repo state under
/// `$HOME/.config/jj/repos/<hash>`, so a read-only `HOME` makes every
/// jj-backed test fail with "Read-only file system". Pointing `HOME` at a
/// process-wide writable tempdir (created once, shared via `.env` so there is
/// no `set_var` race across parallel tests) keeps the tests hermetic and
/// sandbox-safe.
fn writable_home() -> &'static Path {
    static HOME: OnceLock<TempDir> = OnceLock::new();
    HOME.get_or_init(|| TempDir::new().expect("home tempdir"))
        .path()
}

fn jj(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new("jj")
        .args(args)
        .current_dir(cwd)
        .env("HOME", writable_home())
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
        .env("HOME", writable_home())
        .output()
        .expect("failed to spawn shaka")
}

fn setup_repo(repo: &Path) {
    jj_ok(repo, &["git", "init", "--colocate"]);
    jj_ok(
        repo,
        &["config", "set", "--repo", "user.email", "test@test.test"],
    );
    jj_ok(repo, &["config", "set", "--repo", "user.name", "Test"]);
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// `shaka repo ship --dry-run --skip-preflight --bookmark <name>` invoked from
/// a non-default workspace must (a) preview ship's own steps 1–5 and (b)
/// delegate push/PR steps to `send::run`, whose dry-run includes the
/// race-closing pre-push fetch + rebase + conditional preflight.
#[test]
fn dry_run_from_non_default_workspace() {
    let parent = TempDir::new().expect("tempdir");

    let default_repo = parent.path().join("repo");
    std::fs::create_dir(&default_repo).unwrap();
    setup_repo(&default_repo);

    jj_ok(
        &default_repo,
        &["describe", "-m", "feat(shaka): default workspace work"],
    );

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

    jj_ok(
        &ws_path,
        &["describe", "-m", "refactor(shaka): repair from workspace"],
    );

    let bookmark = "refactor/shaka-repair-from-workspace";
    let out = shaka(
        &ws_path,
        &[
            "repo",
            "ship",
            "--dry-run",
            "--skip-preflight",
            "--bookmark",
            bookmark,
        ],
    );
    assert!(
        out.status.success(),
        "shaka repo ship --dry-run failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let stdout = String::from_utf8_lossy(&out.stdout);

    // Ship's own steps (initial fetch + rebase + lint + diff + preflight skip).
    assert!(
        stdout.contains("shaka commit lint -r main@origin..@"),
        "expected ship commit-lint preview; got: {stdout}",
    );
    assert!(
        stdout.contains("jj diff -r main@origin..@"),
        "expected ship diff preview; got: {stdout}",
    );
    assert!(
        stdout.contains("would skip: shaka preflight"),
        "expected ship preflight-skip preview; got: {stdout}",
    );

    // Delegated send dry-run: bookmark set + push must reference the bookmark.
    assert!(
        stdout.contains(&format!("jj bookmark set {bookmark}")),
        "expected delegated bookmark set; got: {stdout}",
    );
    assert!(
        stdout.contains(&format!("jj git push --allow-new --bookmark {bookmark}")),
        "expected delegated push; got: {stdout}",
    );

    // Ship's own pre-push race-closing rebase comes from send's dry-run, so
    // the dry-run output should contain the fetch/rebase/preflight lines that
    // `repo send --dry-run` emits — confirming the delegation.
    assert!(
        stdout.contains("shaka preflight --since main@origin"),
        "expected delegated conditional preflight preview; got: {stdout}",
    );
}

/// Shipping from inside a workspace without a description should fail with a
/// clear error — same behaviour as `repo send`.
#[test]
fn ship_fails_without_description_in_workspace() {
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

    let out = shaka(
        &ws_path,
        &[
            "repo",
            "ship",
            "--dry-run",
            "--skip-preflight",
            "--bookmark",
            "fix/whatever",
        ],
    );
    assert!(
        !out.status.success(),
        "expected shaka repo ship to fail with no description",
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no description"),
        "expected 'no description' error; got: {stderr}",
    );
}
