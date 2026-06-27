//! Front-matter conformance tests against fixtures on disk.
//!
//! Two halves of one contract are checked together: the CUE schema
//! (`schema/frontmatter.cue`) and the Rust `FrontMatter` struct. Each
//! fixture under `tests/fixtures/frontmatter/{valid,invalid}/` is judged
//! "accepted" or "rejected", and every `valid/` fixture must be accepted,
//! every `invalid/` one rejected. Adding a fixture file extends coverage
//! without touching this test — same shape as `tools/shaka/tests/schema.rs`.
//!
//! - `*.yaml` fixtures are raw front-matter, shelled straight to `cue vet`.
//! - `*.md` fixtures are whole notes: they must parse through
//!   `notes_protocol::frontmatter`, and the parsed metadata, re-serialized
//!   to YAML, must also pass `cue vet`. That round-trip is what keeps the
//!   struct and the schema from drifting.
//!
//! Both halves agree on unknown keys: the schema is open (`...`) and the
//! struct preserves foreign keys (`#[serde(flatten)]`), so a note carrying
//! extra Obsidian metadata is *accepted and preserved* by each — the `.md`
//! round-trip would catch any drift, because a struct that dropped the key
//! would re-serialize to a document missing it. See `valid/extra-keys.yaml`
//! and `valid/note-with-extra-keys.md`.

use std::path::{Path, PathBuf};
use std::process::Command;

use notes_protocol::frontmatter;

const SCHEMA: &str = "schema/frontmatter.cue";
const DEFINITION: &str = "#FrontMatter";

/// `cue vet` `yaml` against the shipped schema definition. The YAML is
/// written to a temp file so both raw `.yaml` fixtures and re-serialized
/// `.md` metadata go through the same path.
fn cue_vet(yaml: &str) -> bool {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let doc = tmp.path().join("doc.yaml");
    std::fs::write(&doc, yaml).expect("write temp yaml");
    let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA);
    Command::new("cue")
        .arg("vet")
        .arg("-d")
        .arg(DEFINITION)
        .arg(&schema)
        .arg(&doc)
        .output()
        .expect("failed to spawn cue (is it on PATH?)")
        .status
        .success()
}

/// Decide whether a fixture is accepted by the full contract (parser +
/// schema). `.yaml` is vetted directly; `.md` must parse and its metadata
/// must re-serialize to schema-valid YAML.
fn is_accepted(path: &Path) -> bool {
    let raw = std::fs::read_to_string(path).expect("read fixture");
    match path.extension().and_then(|e| e.to_str()) {
        Some("yaml") => cue_vet(&raw),
        Some("md") => match frontmatter::parse(&raw) {
            Err(_) => false,
            Ok((fm, _body)) => match serde_yaml_ng::to_string(&fm) {
                Ok(yaml) => cue_vet(&yaml),
                Err(_) => false,
            },
        },
        other => panic!(
            "unexpected fixture extension: {other:?} ({})",
            path.display()
        ),
    }
}

fn fixtures_in(subdir: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/frontmatter")
        .join(subdir);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("missing fixture dir: {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("yaml") | Some("md")
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn every_valid_fixture_is_accepted() {
    let fixtures = fixtures_in("valid");
    assert!(!fixtures.is_empty(), "no valid fixtures found");
    for fixture in fixtures {
        assert!(
            is_accepted(&fixture),
            "expected {} to be accepted but it was rejected",
            fixture.display()
        );
    }
}

#[test]
fn every_invalid_fixture_is_rejected() {
    let fixtures = fixtures_in("invalid");
    assert!(!fixtures.is_empty(), "no invalid fixtures found");
    for fixture in fixtures {
        assert!(
            !is_accepted(&fixture),
            "expected {} to be rejected but it was accepted",
            fixture.display()
        );
    }
}
