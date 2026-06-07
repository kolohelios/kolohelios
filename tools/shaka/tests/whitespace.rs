//! Integration tests for `shaka whitespace check|fix`.
//!
//! Each test stages a small set of files into a fresh jj-colocated repo
//! inside a tempdir and runs the shaka binary against it. The fixtures
//! are written programmatically rather than checked in: byte-level
//! differences (BOM, CRLF, trailing whitespace) don't survive editor
//! normalization reliably and are easier to inspect as named byte
//! literals than as files. The pure-scanner unit tests in
//! `src/whitespace.rs` cover content semantics; these tests cover the
//! wiring (CLI args, jj-based file enumeration, exit codes, output
//! mentions the right files).

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

const SHAKA_BIN: &str = env!("CARGO_BIN_EXE_shaka");

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
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| tempfile::TempDir::new().expect("home tempdir"))
        .path()
}

struct Repo {
    root: tempfile::TempDir,
}

impl Repo {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let root = tempfile::TempDir::new().unwrap();
        for (name, bytes) in files {
            let path = root.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, bytes).unwrap();
        }
        run(root.path(), &["jj", "git", "init", "--colocate"]);
        // Disable any line-ending normalization on the colocated git so
        // the test bytes survive snapshot round-trips intact.
        run(root.path(), &["git", "config", "core.autocrlf", "false"]);
        run(root.path(), &["git", "config", "core.safecrlf", "false"]);
        // Snapshot the seeded files into `@`, then advance `@` past
        // them. Sibling workspaces created later parent on `@-`, so the
        // seeded files have to be a committed ancestor — not in primary's
        // current `@` — for them to be visible across workspaces.
        run(root.path(), &["jj", "new"]);
        Repo { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn shaka(&self, args: &[&str]) -> std::process::Output {
        Command::new(SHAKA_BIN)
            .args(args)
            .current_dir(self.root.path())
            .env("HOME", writable_home())
            .output()
            .expect("failed to spawn shaka")
    }

    fn read(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.path().join(name)).unwrap()
    }
}

fn run(cwd: &Path, argv: &[&str]) {
    let status = Command::new(argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .env("HOME", writable_home())
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", argv[0]));
    assert!(status.success(), "{:?} failed", argv);
}

fn stdout_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn check_passes_on_clean_repo() {
    let repo = Repo::new(&[("a.txt", b"hello\n"), ("b.txt", b"world\n")]);
    let out = repo.shaka(&["whitespace", "check"]);
    let stdout = stdout_of(&out);
    assert!(
        out.status.success(),
        "expected success, got {:?}\nstdout: {stdout}\nstderr: {}",
        out.status.code(),
        stderr_of(&out),
    );
    assert!(
        stdout.contains("whitespace check passed"),
        "stdout: {stdout}"
    );
}

#[test]
fn check_fails_on_trailing_whitespace() {
    let repo = Repo::new(&[("bad.txt", b"hello \nworld\n")]);
    let out = repo.shaka(&["whitespace", "check"]);
    assert!(!out.status.success());
    let combined = format!("{}{}", stdout_of(&out), stderr_of(&out));
    assert!(combined.contains("bad.txt"), "output: {combined}");
    assert!(
        combined.contains("trailing whitespace"),
        "output: {combined}"
    );
}

#[test]
fn check_fails_on_crlf() {
    let repo = Repo::new(&[("bad.txt", b"hello\r\nworld\n")]);
    let out = repo.shaka(&["whitespace", "check"]);
    assert!(!out.status.success());
    let combined = format!("{}{}", stdout_of(&out), stderr_of(&out));
    assert!(combined.contains("CRLF"), "output: {combined}");
}

#[test]
fn check_fails_on_bom() {
    let mut bytes = b"\xef\xbb\xbf".to_vec();
    bytes.extend_from_slice(b"hello\n");
    let repo = Repo::new(&[("bad.txt", &bytes)]);
    let out = repo.shaka(&["whitespace", "check"]);
    assert!(!out.status.success());
    let combined = format!("{}{}", stdout_of(&out), stderr_of(&out));
    assert!(combined.contains("BOM"), "output: {combined}");
}

#[test]
fn check_fails_on_missing_trailing_newline() {
    let repo = Repo::new(&[("bad.txt", b"hello")]);
    let out = repo.shaka(&["whitespace", "check"]);
    assert!(!out.status.success());
    let combined = format!("{}{}", stdout_of(&out), stderr_of(&out));
    assert!(
        combined.contains("missing trailing newline"),
        "output: {combined}"
    );
}

#[test]
fn check_skips_binary_files() {
    let repo = Repo::new(&[
        ("img.bin", b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR"),
        ("text.txt", b"hello\n"),
    ]);
    let out = repo.shaka(&["whitespace", "check"]);
    let stdout = stdout_of(&out);
    assert!(
        out.status.success(),
        "binary file was incorrectly flagged\nstdout: {stdout}\nstderr: {}",
        stderr_of(&out)
    );
}

#[test]
fn check_skips_gitignored_files() {
    // jj auto-snapshots files in the working tree, so the only durable
    // way to keep a file out of `@`'s tree is to .gitignore it. Verify
    // that whitespace check honors that exclusion.
    let repo = Repo::new(&[
        ("a.txt", b"hello\n"),
        (".gitignore", b"build/\n"),
        ("build/garbage.txt", b"trailing \n"),
    ]);
    let out = repo.shaka(&["whitespace", "check"]);
    assert!(
        out.status.success(),
        "ignored file was incorrectly flagged\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        stderr_of(&out)
    );
}

#[test]
fn check_respects_path_arg_scope() {
    let repo = Repo::new(&[
        ("good/clean.txt", b"hello\n"),
        ("bad/dirty.txt", b"trailing \n"),
    ]);
    // Scope to good/ — should pass even though bad/ has issues.
    let out = repo.shaka(&["whitespace", "check", "good"]);
    assert!(
        out.status.success(),
        "path-scoped check leaked into unrelated dir\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        stderr_of(&out)
    );
    // Scope to bad/ — should fail.
    let out = repo.shaka(&["whitespace", "check", "bad"]);
    assert!(!out.status.success());
}

#[test]
fn fix_rewrites_files_in_place() {
    let mut bom = b"\xef\xbb\xbf".to_vec();
    bom.extend_from_slice(b"hello \r\n\n\n");
    let repo = Repo::new(&[
        ("a.txt", b"hello"),
        ("b.txt", bom.as_slice()),
        ("c.txt", b"already-clean\n"),
    ]);
    let out = repo.shaka(&["whitespace", "fix"]);
    assert!(
        out.status.success(),
        "fix exited non-zero\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        stderr_of(&out)
    );
    assert_eq!(repo.read("a.txt"), b"hello\n");
    assert_eq!(repo.read("b.txt"), b"hello\n");
    assert_eq!(repo.read("c.txt"), b"already-clean\n");

    // After fix, check should pass.
    let out = repo.shaka(&["whitespace", "check"]);
    assert!(
        out.status.success(),
        "check failed after fix\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        stderr_of(&out)
    );
}

#[test]
fn check_runs_inside_jj_sibling_workspace() {
    // Regression for #210: in a jj sibling workspace, `.git` isn't
    // reachable on the filesystem because the colocated `.git` lives
    // only under the primary tree. shaka must enumerate via jj rather
    // than walking up for git.
    let repo = Repo::new(&[("a.txt", b"trailing \n")]);
    let parent = tempfile::TempDir::new().unwrap();
    let sibling = parent.path().join("ws-sibling");
    run(
        repo.path(),
        &[
            "jj",
            "workspace",
            "add",
            "--name",
            "ws-sibling",
            sibling.to_str().unwrap(),
        ],
    );

    let out = Command::new(SHAKA_BIN)
        .args(["whitespace", "check"])
        .current_dir(&sibling)
        .output()
        .expect("failed to spawn shaka");
    assert!(
        !out.status.success(),
        "expected failure on bad whitespace from sibling\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let combined = format!("{}{}", stdout_of(&out), stderr_of(&out));
    assert!(combined.contains("a.txt"), "output: {combined}");
    assert!(
        combined.contains("trailing whitespace"),
        "output: {combined}"
    );
}

#[test]
fn check_finds_files_added_only_in_sibling() {
    // The crux of #210: a file that exists only in the sibling
    // workspace's `@` is invisible to primary's git index. Using jj for
    // enumeration ensures shaka still scans it.
    let repo = Repo::new(&[("clean.txt", b"hello\n")]);
    let parent = tempfile::TempDir::new().unwrap();
    let sibling = parent.path().join("ws-sibling");
    run(
        repo.path(),
        &[
            "jj",
            "workspace",
            "add",
            "--name",
            "ws-sibling",
            sibling.to_str().unwrap(),
        ],
    );

    std::fs::write(sibling.join("polluted.txt"), b"trailing \n").unwrap();

    let out = Command::new(SHAKA_BIN)
        .args(["whitespace", "check"])
        .current_dir(&sibling)
        .output()
        .expect("failed to spawn shaka");
    assert!(
        !out.status.success(),
        "sibling-only file went undetected\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let combined = format!("{}{}", stdout_of(&out), stderr_of(&out));
    assert!(combined.contains("polluted.txt"), "output: {combined}");
}

#[test]
fn check_default_path_scans_cwd_subtree() {
    let repo = Repo::new(&[
        ("project/a.txt", b"hello\n"),
        ("project/sub/b.txt", b"trailing \n"),
        ("other/c.txt", b"trailing \n"),
    ]);
    // Run from inside project/ — should fail on sub/b.txt but ignore other/.
    let out = Command::new(SHAKA_BIN)
        .args(["whitespace", "check"])
        .current_dir(repo.path().join("project"))
        .output()
        .expect("failed to spawn shaka");
    assert!(!out.status.success(), "expected failure inside project/");
    let combined = format!("{}{}", stdout_of(&out), stderr_of(&out));
    assert!(combined.contains("sub/b.txt"), "output: {combined}");
    assert!(
        !combined.contains("other/c.txt"),
        "leaked outside cwd: {combined}"
    );
}
