//! Integration tests for the labels.cue schema.
//!
//! Runs `cue vet -c` against fixtures in `tests/fixtures/labels/{valid,invalid}/`.
//! Each fixture is a complete `labels.cue` that should be accepted or rejected
//! by the shipped schema. The test walks both directories and asserts every
//! fixture in `valid/` passes and every fixture in `invalid/` fails — adding a
//! new fixture is enough to extend coverage.

use std::path::{Path, PathBuf};
use std::process::Command;

const SCHEMA_PATH: &str = "schema/labels-schema.cue";

fn cue_vet(schema: &Path, labels: &Path) -> bool {
    Command::new("cue")
        .arg("vet")
        .arg("-c")
        .arg(schema)
        .arg(labels)
        .output()
        .expect("failed to spawn cue (is it on PATH?)")
        .status
        .success()
}

fn fixtures_in(subdir: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/labels")
        .join(subdir);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("missing fixture dir: {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "cue").unwrap_or(false))
        .collect();
    out.sort();
    out
}

#[test]
fn every_valid_fixture_is_accepted() {
    let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_PATH);
    let fixtures = fixtures_in("valid");
    assert!(!fixtures.is_empty(), "no valid fixtures found");
    for fixture in fixtures {
        assert!(
            cue_vet(&schema, &fixture),
            "expected {} to pass cue vet but it failed",
            fixture.display()
        );
    }
}

#[test]
fn every_invalid_fixture_is_rejected() {
    let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_PATH);
    let fixtures = fixtures_in("invalid");
    assert!(!fixtures.is_empty(), "no invalid fixtures found");
    for fixture in fixtures {
        assert!(
            !cue_vet(&schema, &fixture),
            "expected {} to fail cue vet but it passed",
            fixture.display()
        );
    }
}
