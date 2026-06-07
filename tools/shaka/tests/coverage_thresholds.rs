//! Integration test for `shaka project coverage-thresholds` (#832).
//!
//! The generated `coverage` recipe reads its gate from this subcommand
//! instead of `cue export ../../tools/shaka/schema/project-schema.cue`,
//! so the recipe works in an external repo with no kolohelios source
//! tree. These tests prove the subcommand exports the cwd project's
//! `project.cue` as JSON using the bundled CUE closure pointed at by
//! `SHAKA_CUE_MODULE_DIR` — the same resolver `schema-check` uses for a
//! nix-packaged binary run outside any CUE module.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

const SHAKA_BIN: &str = env!("CARGO_BIN_EXE_shaka");
const MODULE_DIR_ENV: &str = "SHAKA_CUE_MODULE_DIR";

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}

/// Stage the bundled CUE module closure the packaged binary points at:
/// a `cue.mod`, a stub domain registry, and the real schema files under
/// the module-relative `tools/shaka/schema/` path.
fn stage_module(dir: &Path) {
    common::write_test_module(dir);
    let real_schema = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
    copy_dir(&real_schema, &dir.join("tools/shaka/schema"));
}

fn coverage_thresholds(work: &Path, module: &Path) -> std::process::Output {
    Command::new(SHAKA_BIN)
        .args(["project", "coverage-thresholds"])
        .current_dir(work)
        .env(MODULE_DIR_ENV, module)
        .output()
        .expect("failed to spawn shaka")
}

#[test]
fn emits_coverage_block_outside_a_cue_module() {
    let module = tempfile::TempDir::new().unwrap();
    stage_module(module.path());

    // A consumer checkout that is NOT a CUE module and has no kolohelios
    // source tree — exactly what a generated coverage recipe faces in an
    // external repo. The project.cue carries a coverage gate.
    let work = tempfile::TempDir::new().unwrap();
    fs::write(
        work.path().join("project.cue"),
        "package project\n\
         #Project & { name: \"foo\", kind: \"rust-cli\", \
         cli: { binaryName: \"foo\" }, coverage: { line: { fail: 80 } } }\n",
    )
    .unwrap();

    let out = coverage_thresholds(work.path(), module.path());
    assert!(
        out.status.success(),
        "coverage-thresholds should succeed; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr),
    );

    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("output must be valid JSON");
    // The recipe reads `.coverage.line.fail`; assert it survives the
    // round-trip so the generated gate keeps working.
    assert_eq!(json["coverage"]["line"]["fail"], 80);
}

#[test]
fn fails_when_no_project_cue() {
    let module = tempfile::TempDir::new().unwrap();
    stage_module(module.path());

    let work = tempfile::TempDir::new().unwrap();
    let out = coverage_thresholds(work.path(), module.path());
    assert!(
        !out.status.success(),
        "coverage-thresholds must fail loudly with no project.cue rather \
         than feed empty input to jq",
    );
}
