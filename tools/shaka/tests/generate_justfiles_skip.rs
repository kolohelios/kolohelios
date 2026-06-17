//! Integration: `generate.skip` waives a project from generate-justfiles (#924).
//!
//! A rust-worker Cargo workspace hand-maintains its justfile until shaka grows
//! workspace-aware generation (#922). `generate.skip [{step: "justfile"}]` must
//! exclude it from BOTH the write pass (the hand-maintained file is never
//! overwritten) and the `--check` drift pass (it never counts as drift), while a
//! non-waived project still drifts. The drift check runs once at the repo level,
//! so this exercises the real end-to-end path the way preflight/CI does.

use std::path::Path;
use std::process::{Command, Output};

const SHAKA_BIN: &str = env!("CARGO_BIN_EXE_shaka");
const JUNK: &str = "# hand-maintained — not the generated template\ndefault:\n    @echo waived\n";

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(SHAKA_BIN)
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn shaka")
}

/// Stage a domain-free repo (cue.mod + LICENSE so the schema's domain import
/// resolves via shaka's stub) with two rust-worker projects: `apps/waived`
/// carries `generate.skip` + a hand-maintained justfile; `apps/plain` does not.
fn stage() -> tempfile::TempDir {
    let root = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("cue.mod")).unwrap();
    std::fs::write(
        root.path().join("cue.mod/module.cue"),
        "module: \"kolohelios.com\"\nlanguage: version: \"v0.15.1\"\n",
    )
    .unwrap();
    std::fs::write(root.path().join("LICENSE"), "MIT OR Apache-2.0\n").unwrap();

    let waived = root.path().join("apps/waived");
    std::fs::create_dir_all(&waived).unwrap();
    std::fs::write(
        waived.join("project.cue"),
        "package project\n\n#Project & {\n\tname: \"waived\"\n\tkind: \"rust-worker\"\n\tworker: {}\n\tgenerate: {\n\t\tskip: [{step: \"justfile\", reason: \"hand-maintained until #922\"}]\n\t}\n}\n",
    )
    .unwrap();
    std::fs::write(waived.join("justfile"), JUNK).unwrap();

    let plain = root.path().join("apps/plain");
    std::fs::create_dir_all(&plain).unwrap();
    std::fs::write(
        plain.join("project.cue"),
        "package project\n\n#Project & {\n\tname: \"plain\"\n\tkind: \"rust-worker\"\n\tworker: {}\n}\n",
    )
    .unwrap();

    root
}

#[test]
fn waiver_protects_justfile_from_write_and_check() {
    let repo = stage();

    // Write pass: the waived project's hand-maintained justfile is untouched,
    // and the run reports it as WAIVED rather than writing over it.
    let gen = run(repo.path(), &["project", "generate-justfiles"]);
    assert!(
        gen.status.success(),
        "generate-justfiles failed: {}",
        String::from_utf8_lossy(&gen.stderr)
    );
    let waived_jf = std::fs::read_to_string(repo.path().join("apps/waived/justfile")).unwrap();
    assert_eq!(
        waived_jf, JUNK,
        "the waiver must not overwrite the hand-maintained justfile"
    );
    assert!(
        String::from_utf8_lossy(&gen.stdout).contains("WAIVED"),
        "expected a WAIVED line: {}",
        String::from_utf8_lossy(&gen.stdout)
    );

    // --check now passes: every generated item was just written, and the
    // waived project is excluded despite its drifting hand-maintained justfile.
    let check = run(repo.path(), &["project", "generate-justfiles", "--check"]);
    assert!(
        check.status.success(),
        "--check should pass when the only drift is waived:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr),
    );
    assert!(String::from_utf8_lossy(&check.stdout).contains("WAIVED"));
}

#[test]
fn unwaived_drift_still_fails() {
    let repo = stage();
    // Generate everything correctly first (waiving apps/waived).
    assert!(run(repo.path(), &["project", "generate-justfiles"])
        .status
        .success());

    // Drift the NON-waived project; --check must fail on it (the waiver is
    // surgical, not a global mute).
    std::fs::write(repo.path().join("apps/plain/justfile"), "drifted\n").unwrap();
    let check = run(repo.path(), &["project", "generate-justfiles", "--check"]);
    assert!(
        !check.status.success(),
        "unwaived drift must fail the check: {}",
        String::from_utf8_lossy(&check.stdout)
    );
    let out = String::from_utf8_lossy(&check.stdout);
    assert!(
        out.contains("DRIFT"),
        "expected DRIFT for apps/plain: {out}"
    );
    assert!(
        out.contains("WAIVED"),
        "apps/waived should still be waived: {out}"
    );
}
