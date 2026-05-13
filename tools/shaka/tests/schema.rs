//! Integration tests for the project.cue schema.
//!
//! Runs `cue vet -c` against fixtures in `tests/fixtures/schema/{valid,invalid}/`.
//! Each fixture is a complete `project.cue` that should be accepted or rejected
//! by the shipped schema. The test walks both directories and asserts every
//! fixture in `valid/` passes and every fixture in `invalid/` fails — adding a
//! new fixture is enough to extend coverage.

use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

const SCHEMA_PATH: &str = "schema/project-schema.cue";

/// Vet `project` against `schema`, with cue's cwd set to a temp dir
/// that carries a `cue.mod` and a real-ish domain registry. The
/// schema imports the registry to constrain hostname fields, so cue
/// needs both the module root (found by walking up from cwd) and a
/// registry whose `#KnownHostnames` actually enumerates known zones
/// — a wildcard stub would let invalid fixtures (intentionally using
/// unregistered hostnames) slip through. Local `cargo test` happens
/// to be inside the real module, but `nix build`'s sandbox is not —
/// this setup makes the constraint fire in both environments.
fn cue_vet(schema: &Path, project: &Path) -> bool {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    common::write_test_module_with_registry(tmp.path(), &["kolohelios.com"]);
    Command::new("cue")
        .arg("vet")
        .arg("-c")
        .arg(schema)
        .arg(project)
        .current_dir(tmp.path())
        .output()
        .expect("failed to spawn cue (is it on PATH?)")
        .status
        .success()
}

fn fixtures_in(subdir: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/schema")
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
