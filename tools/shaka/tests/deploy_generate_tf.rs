//! Integration tests for `shaka deploy generate-tf`.
//!
//! Each case under `tests/fixtures/deploy/<case>/` has:
//!
//! - `inputs/` — a stand-in repo root (with `apps/`, `infra/`, etc.)
//! - `expected/` — the `.tf` files the generator should produce in
//!   `infra/cloudflare-deploy/terraform/generated/`
//!
//! The test copies inputs to a tempdir, runs the built `shaka deploy
//! generate-tf` binary against it, and diffs the generated dir against
//! `expected/`. The `--check` mode is exercised in a second pass to
//! confirm a clean tree reports no drift.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const SHAKA: &str = env!("CARGO_BIN_EXE_shaka");
const GENERATED_DIR: &str = "infra/cloudflare-deploy/terraform/generated";

/// Recursive copy, with one rewrite: any file named `<name>.fixture`
/// is materialized as `<name>` in the destination. Fixtures use
/// `project.cue.fixture` so `shaka project schema-check`'s stray
/// finder ignores them (it only fires on files literally named
/// `project.cue`); the test materializes them as `project.cue` so the
/// generator sees a normal project tree.
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

fn collect_tf_files(dir: &Path) -> BTreeMap<String, String> {
    if !dir.is_dir() {
        return BTreeMap::new();
    }
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "tf").unwrap_or(false) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let content = std::fs::read_to_string(&path).expect("read tf file");
            out.insert(name, content);
        }
    }
    out
}

fn cases() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deploy");
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
fn every_fixture_produces_expected_tf() {
    let cases = cases();
    assert!(!cases.is_empty(), "no fixtures found");

    for case in cases {
        let label = case.file_name().unwrap().to_string_lossy().into_owned();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = tmp.path();

        // Stage the case's inputs as if they were the repo root.
        copy_dir_all(&case.join("inputs"), root).expect("copy inputs");

        // The substrate the generator writes into. The substrate's own
        // contents aren't under test here — we only care about what
        // lands in `generated/`.
        std::fs::create_dir_all(root.join(GENERATED_DIR)).expect("create generated dir");

        // Generate.
        let status = Command::new(SHAKA)
            .args(["deploy", "generate-tf"])
            .current_dir(root)
            .status()
            .expect("spawn shaka");
        assert!(
            status.success(),
            "[{label}] shaka deploy generate-tf failed"
        );

        // Diff against expected.
        let actual = collect_tf_files(&root.join(GENERATED_DIR));
        let expected = collect_tf_files(&case.join("expected"));
        assert_eq!(
            actual, expected,
            "[{label}] generated TF does not match expected"
        );

        // Re-run in --check mode against a clean tree; must succeed.
        let check_status = Command::new(SHAKA)
            .args(["deploy", "generate-tf", "--check"])
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
    // Pick any fixture with at least one expected .tf file.
    let case = cases()
        .into_iter()
        .find(|c| {
            collect_tf_files(&c.join("expected"))
                .iter()
                .next()
                .is_some()
        })
        .expect("need at least one fixture with output");

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let root = tmp.path();
    copy_dir_all(&case.join("inputs"), root).expect("copy inputs");
    std::fs::create_dir_all(root.join(GENERATED_DIR)).expect("create generated dir");

    Command::new(SHAKA)
        .args(["deploy", "generate-tf"])
        .current_dir(root)
        .status()
        .expect("spawn shaka")
        .success()
        .then_some(())
        .expect("initial generate must succeed");

    // Tamper with the first generated file.
    let generated = root.join(GENERATED_DIR);
    let first = std::fs::read_dir(&generated)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().map(|x| x == "tf").unwrap_or(false))
        .expect("at least one generated .tf");
    let mut content = std::fs::read_to_string(&first).unwrap();
    content.push_str("\n# drifted\n");
    std::fs::write(&first, content).unwrap();

    // --check must now fail.
    let status = Command::new(SHAKA)
        .args(["deploy", "generate-tf", "--check"])
        .current_dir(root)
        .status()
        .expect("spawn shaka --check");
    assert!(!status.success(), "--check should fail on drifted tree");
}

#[test]
fn check_mode_detects_orphan_files() {
    let case = cases()
        .into_iter()
        .find(|c| c.file_name().map(|n| n == "single-worker").unwrap_or(false))
        .expect("single-worker fixture");

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let root = tmp.path();
    copy_dir_all(&case.join("inputs"), root).expect("copy inputs");
    let generated = root.join(GENERATED_DIR);
    std::fs::create_dir_all(&generated).expect("create generated dir");

    Command::new(SHAKA)
        .args(["deploy", "generate-tf"])
        .current_dir(root)
        .status()
        .expect("spawn shaka")
        .success()
        .then_some(())
        .expect("initial generate must succeed");

    // Inject an orphan file the generator wouldn't produce.
    std::fs::write(generated.join("orphan.tf"), "# stale\n").unwrap();

    let status = Command::new(SHAKA)
        .args(["deploy", "generate-tf", "--check"])
        .current_dir(root)
        .status()
        .expect("spawn shaka --check");
    assert!(
        !status.success(),
        "--check should fail when an orphan exists"
    );
}
