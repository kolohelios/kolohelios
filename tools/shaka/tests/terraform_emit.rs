//! Integration tests for `shaka terraform emit`.
//!
//! Each fixture under `tests/fixtures/terraform/valid/<case>/` pairs a
//! `cue/` directory with an `expected.tf` golden. The test stages the
//! fixture into a tempdir, runs the shaka binary, and diffs the produced
//! `terraform/module.tf` against the golden. Fixtures under
//! `tests/fixtures/terraform/invalid/<case>/` must cause emit to exit
//! non-zero.

use std::path::{Path, PathBuf};
use std::process::Command;

const SHAKA_BIN: &str = env!("CARGO_BIN_EXE_shaka");

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/terraform")
}

fn fixtures_in(subdir: &str) -> Vec<PathBuf> {
    let dir = fixtures_root().join(subdir);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("missing fixture dir: {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

#[test]
fn every_valid_fixture_renders_to_its_golden() {
    let fixtures = fixtures_in("valid");
    assert!(!fixtures.is_empty(), "no valid fixtures found");
    for fixture in fixtures {
        let case = fixture.file_name().unwrap().to_string_lossy().into_owned();
        let golden_path = fixture.join("expected.tf");
        let golden = std::fs::read_to_string(&golden_path)
            .unwrap_or_else(|e| panic!("missing golden {}: {e}", golden_path.display()));

        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path().join("project");
        copy_dir(&fixture.join("cue"), &project.join("cue"));

        let output = Command::new(SHAKA_BIN)
            .args(["terraform", "emit", "--project"])
            .arg(&project)
            .output()
            .expect("failed to spawn shaka");
        assert!(
            output.status.success(),
            "case {case}: shaka exited {}; stderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );

        let produced_path = project.join("terraform").join("module.tf");
        let produced = std::fs::read_to_string(&produced_path).unwrap_or_else(|e| {
            panic!(
                "case {case}: emit did not write {}: {e}",
                produced_path.display()
            )
        });

        assert_eq!(
            produced, golden,
            "case {case}: produced output does not match golden\n\
             --- produced (left) ---\n{produced}\n\
             --- golden (right) ---\n{golden}"
        );
    }
}

#[test]
fn every_invalid_fixture_causes_emit_to_fail() {
    // Both schema-level invalids (rejected by `cue export`) and
    // emit-level invalids (pass the schema but fail the emitter's HCL
    // expression parse) must cause emit to exit non-zero.
    let mut fixtures = fixtures_in("invalid/schema");
    fixtures.extend(fixtures_in("invalid/emit"));
    assert!(!fixtures.is_empty(), "no invalid fixtures found");
    for fixture in fixtures {
        let case = fixture
            .strip_prefix(fixtures_root())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path().join("project");
        copy_dir(&fixture.join("cue"), &project.join("cue"));

        let output = Command::new(SHAKA_BIN)
            .args(["terraform", "emit", "--project"])
            .arg(&project)
            .output()
            .expect("failed to spawn shaka");

        assert!(
            !output.status.success(),
            "case {case}: expected shaka to fail but it succeeded; stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

/// Create the minimum filesystem shape `shaka project schema-check`'s
/// discover() requires — a project under a slot, with a `project.cue`
/// declaring `kind: "infra"`. `shaka terraform check` walks via this
/// path so the staged project has to be visible to discover().
fn fake_repo_root(root: &Path, project_name: &str) -> PathBuf {
    let project = root.join("apps").join(project_name);
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("project.cue"),
        format!("package project\n#Project & {{ name: \"{project_name}\", kind: \"infra\" }}\n"),
    )
    .unwrap();
    project
}

#[test]
fn check_passes_when_committed_tf_matches_emit_output() {
    let fixture = fixtures_root().join("valid/full-devbox");
    let tmp = tempfile::TempDir::new().unwrap();
    let project = fake_repo_root(tmp.path(), "devbox");
    copy_dir(&fixture.join("cue"), &project.join("cue"));

    let emit = Command::new(SHAKA_BIN)
        .args(["terraform", "emit", "--project"])
        .arg(&project)
        .current_dir(tmp.path())
        .output()
        .expect("failed to spawn shaka");
    assert!(
        emit.status.success(),
        "emit failed: {}",
        String::from_utf8_lossy(&emit.stderr)
    );

    let check = Command::new(SHAKA_BIN)
        .args(["terraform", "check"])
        .current_dir(tmp.path())
        .output()
        .expect("failed to spawn shaka");
    assert!(
        check.status.success(),
        "check failed unexpectedly:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
}

#[test]
fn check_fails_when_committed_tf_drifted_from_cue() {
    let fixture = fixtures_root().join("valid/full-devbox");
    let tmp = tempfile::TempDir::new().unwrap();
    let project = fake_repo_root(tmp.path(), "devbox");
    copy_dir(&fixture.join("cue"), &project.join("cue"));
    Command::new(SHAKA_BIN)
        .args(["terraform", "emit", "--project"])
        .arg(&project)
        .current_dir(tmp.path())
        .output()
        .expect("emit");
    // Corrupt the committed module.tf so it drifts from CUE.
    let module_tf = project.join("terraform").join("module.tf");
    std::fs::write(&module_tf, "# hand-edited drift\n").unwrap();

    let check = Command::new(SHAKA_BIN)
        .args(["terraform", "check"])
        .current_dir(tmp.path())
        .output()
        .expect("failed to spawn shaka");
    assert!(
        !check.status.success(),
        "expected check to fail on drift but it passed; stdout:\n{}",
        String::from_utf8_lossy(&check.stdout)
    );
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.contains("drifted"),
        "expected 'drifted' in stderr, got: {stderr}"
    );
}

#[test]
fn check_fails_when_committed_tf_missing() {
    let fixture = fixtures_root().join("valid/full-devbox");
    let tmp = tempfile::TempDir::new().unwrap();
    let project = fake_repo_root(tmp.path(), "devbox");
    copy_dir(&fixture.join("cue"), &project.join("cue"));
    // No emit — module.tf was never produced.

    let check = Command::new(SHAKA_BIN)
        .args(["terraform", "check"])
        .current_dir(tmp.path())
        .output()
        .expect("failed to spawn shaka");
    assert!(
        !check.status.success(),
        "expected check to fail when module.tf is missing"
    );
}

#[test]
fn emit_is_idempotent_across_two_runs() {
    // Pick the most comprehensive fixture and emit twice; the second run
    // must produce byte-identical output. Guards against any nondeterminism
    // we might accidentally introduce (HashMap iteration, timestamps, etc.).
    let fixture = fixtures_root().join("valid/full-devbox");
    let tmp = tempfile::TempDir::new().unwrap();
    let project = tmp.path().join("project");
    copy_dir(&fixture.join("cue"), &project.join("cue"));

    let run = || {
        let s = Command::new(SHAKA_BIN)
            .args(["terraform", "emit", "--project"])
            .arg(&project)
            .output()
            .expect("failed to spawn shaka");
        assert!(
            s.status.success(),
            "shaka exited {}: {}",
            s.status,
            String::from_utf8_lossy(&s.stderr)
        );
        std::fs::read_to_string(project.join("terraform").join("module.tf")).unwrap()
    };

    let first = run();
    let second = run();
    assert_eq!(
        first, second,
        "emit is not idempotent — two runs produced different output"
    );
}
