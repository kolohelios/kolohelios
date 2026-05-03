//! Frontmatter parser conformance tests against fixtures on disk.
//!
//! Each `tests/fixtures/frontmatter/valid/*.md` file is expected to parse
//! cleanly; each `tests/fixtures/frontmatter/invalid/*.md` file is expected
//! to fail. Adding a new fixture file extends coverage without touching
//! Rust — see `tools/shaka/tests/schema.rs` for the same shape.

use std::path::{Path, PathBuf};

use blogctl::Post;

fn fixtures_in(subdir: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/frontmatter")
        .join(subdir);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("missing fixture dir: {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "md").unwrap_or(false))
        .collect();
    out.sort();
    out
}

#[test]
fn every_valid_fixture_parses() {
    let fixtures = fixtures_in("valid");
    assert!(!fixtures.is_empty(), "no valid fixtures found");
    for fixture in fixtures {
        let raw = std::fs::read_to_string(&fixture).unwrap();
        let result = Post::parse(&fixture, &raw);
        assert!(
            result.is_ok(),
            "expected {} to parse but got: {:?}",
            fixture.display(),
            result.err()
        );
    }
}

#[test]
fn every_invalid_fixture_is_rejected() {
    let fixtures = fixtures_in("invalid");
    assert!(!fixtures.is_empty(), "no invalid fixtures found");
    for fixture in fixtures {
        let raw = std::fs::read_to_string(&fixture).unwrap();
        let result = Post::parse(&fixture, &raw);
        assert!(
            result.is_err(),
            "expected {} to fail but it parsed",
            fixture.display()
        );
    }
}

#[test]
fn full_fixture_round_trips() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frontmatter/valid/full.md");
    let raw = std::fs::read_to_string(&path).unwrap();
    let post = Post::parse(&path, &raw).expect("parse");
    let rendered = post.render().expect("render");
    let reparsed = Post::parse(&path, &rendered).expect("reparse");
    assert_eq!(reparsed.metadata, post.metadata);
}
