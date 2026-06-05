//! Integration tests for the terraform module schema.
//!
//! Walks fixtures under `tests/fixtures/terraform/valid/<case>/cue/` and
//! `tests/fixtures/terraform/invalid/schema/<case>/cue/` and asserts that
//! `cue vet -c` accepts every valid one and rejects every schema-level
//! invalid one.

use std::path::{Path, PathBuf};
use std::process::Command;

const SCHEMA_PATH: &str = "schema/terraform-schema.cue";

fn cue_vet(schema: &Path, cue_dir: &Path) -> bool {
    let cue_files: Vec<PathBuf> = std::fs::read_dir(cue_dir)
        .unwrap_or_else(|_| panic!("missing cue dir: {}", cue_dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("cue"))
        .collect();
    Command::new("cue")
        .arg("vet")
        .arg("-c")
        .arg(schema)
        .args(&cue_files)
        .output()
        .expect("failed to spawn cue (is it on PATH?)")
        .status
        .success()
}

fn fixtures_in(subdir: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/terraform")
        .join(subdir);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("missing fixture dir: {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out
}

#[test]
fn every_valid_fixture_is_accepted_by_schema() {
    let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_PATH);
    let fixtures = fixtures_in("valid");
    assert!(!fixtures.is_empty(), "no valid fixtures found");
    for fixture in fixtures {
        let cue_dir = fixture.join("cue");
        assert!(
            cue_vet(&schema, &cue_dir),
            "expected {} to pass cue vet but it failed",
            cue_dir.display()
        );
    }
}

#[test]
fn every_invalid_schema_fixture_is_rejected_by_schema() {
    let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_PATH);
    let fixtures = fixtures_in("invalid/schema");
    assert!(!fixtures.is_empty(), "no invalid/schema fixtures found");
    for fixture in fixtures {
        let cue_dir = fixture.join("cue");
        assert!(
            !cue_vet(&schema, &cue_dir),
            "expected {} to fail cue vet but it passed",
            cue_dir.display()
        );
    }
}
