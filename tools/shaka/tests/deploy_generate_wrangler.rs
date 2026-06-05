//! Integration tests for `shaka deploy generate-wrangler`.
//!
//! Each case under `tests/fixtures/wrangler/<case>/` has:
//!
//! - `inputs/` — a stand-in repo root (`apps/`, `cue.mod/`, the domain
//!   registry the schema imports)
//! - `expected/` — the `wrangler.toml` files the generator should
//!   produce, at the same `apps/<name>/wrangler.toml` paths it writes
//!   them (empty when the case expects no output)
//!
//! The test copies inputs to a tempdir, runs the built `shaka deploy
//! generate-wrangler` binary against it, and diffs every `wrangler.toml`
//! under the tree against `expected/`. A second `--check` pass confirms
//! a freshly-generated tree reports no drift.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const SHAKA: &str = env!("CARGO_BIN_EXE_shaka");

/// Recursive copy, with one rewrite: any file named `<name>.fixture`
/// is materialized as `<name>` in the destination. Project files use
/// `project.cue.fixture` so `shaka project schema-check`'s stray
/// finder ignores them; the test materializes them as `project.cue`
/// so the generator sees a normal project tree.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let file_name = entry.file_name();
        let dst_name = match file_name.to_str() {
            Some(s) if s.ends_with(".fixture") => {
                std::ffi::OsString::from(&s[..s.len() - ".fixture".len()])
            }
            _ => file_name,
        };
        let to = dst.join(dst_name);
        if from.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Walk `base` recursively and return every `wrangler.toml`, keyed by
/// its path relative to `base`, with the file content as the value.
fn collect_wrangler_files(base: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    collect_into(base, base, &mut out);
    out
}

fn collect_into(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_into(base, &path, out);
        } else if path
            .file_name()
            .map(|n| n == "wrangler.toml")
            .unwrap_or(false)
        {
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let content = std::fs::read_to_string(&path).expect("read wrangler.toml");
            out.insert(rel, content);
        }
    }
}

fn cases() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wrangler");
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
fn every_fixture_produces_expected_wrangler() {
    let cases = cases();
    assert!(!cases.is_empty(), "no fixtures found");

    for case in cases {
        let label = case.file_name().unwrap().to_string_lossy().into_owned();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = tmp.path();

        // Stage the case's inputs as if they were the repo root.
        copy_dir_all(&case.join("inputs"), root).expect("copy inputs");

        // Generate.
        let status = Command::new(SHAKA)
            .args(["deploy", "generate-wrangler"])
            .current_dir(root)
            .status()
            .expect("spawn shaka");
        assert!(
            status.success(),
            "[{label}] shaka deploy generate-wrangler failed"
        );

        // Diff every wrangler.toml against expected.
        let actual = collect_wrangler_files(root);
        let expected = collect_wrangler_files(&case.join("expected"));
        assert_eq!(
            actual, expected,
            "[{label}] generated wrangler.toml does not match expected"
        );

        // Re-run in --check mode against a clean tree; must succeed.
        let check_status = Command::new(SHAKA)
            .args(["deploy", "generate-wrangler", "--check"])
            .current_dir(root)
            .status()
            .expect("spawn shaka --check");
        assert!(
            check_status.success(),
            "[{label}] --check failed on a freshly-generated tree"
        );
    }
}

#[test]
fn check_mode_detects_drift() {
    let case = cases()
        .into_iter()
        .find(|c| {
            c.file_name()
                .map(|n| n == "build-cron-vars")
                .unwrap_or(false)
        })
        .expect("build-cron-vars fixture");

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let root = tmp.path();
    copy_dir_all(&case.join("inputs"), root).expect("copy inputs");

    Command::new(SHAKA)
        .args(["deploy", "generate-wrangler"])
        .current_dir(root)
        .status()
        .expect("spawn shaka")
        .success()
        .then_some(())
        .expect("initial generate must succeed");

    // Tamper with a generated file.
    let generated = root.join("apps/pollen-alert/wrangler.toml");
    let mut content = std::fs::read_to_string(&generated).unwrap();
    content.push_str("\n# drifted\n");
    std::fs::write(&generated, content).unwrap();

    let status = Command::new(SHAKA)
        .args(["deploy", "generate-wrangler", "--check"])
        .current_dir(root)
        .status()
        .expect("spawn shaka --check");
    assert!(!status.success(), "--check should fail on drifted tree");
}

#[test]
fn check_ignores_handwritten_wrangler_without_wrangler_block() {
    // A project with a hand-maintained wrangler.toml but no `wrangler:`
    // block must not be flagged: the generator manages only opted-in
    // projects, so the file is neither rewritten nor treated as an
    // orphan. --check passes on the untouched inputs alone.
    let case = cases()
        .into_iter()
        .find(|c| {
            c.file_name()
                .map(|n| n == "handwritten-untouched")
                .unwrap_or(false)
        })
        .expect("handwritten-untouched fixture");

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let root = tmp.path();
    copy_dir_all(&case.join("inputs"), root).expect("copy inputs");

    let status = Command::new(SHAKA)
        .args(["deploy", "generate-wrangler", "--check"])
        .current_dir(root)
        .status()
        .expect("spawn shaka --check");
    assert!(
        status.success(),
        "--check must ignore a hand-written wrangler.toml with no wrangler block"
    );
}
