//! Integration test for the packaged-binary schema resolver (#741).
//!
//! A nix-built `shaka` has no `tools/shaka/` source at runtime and is
//! typically run in a repo that is not itself a CUE module. These tests
//! prove `shaka project schema-check` still validates `project.cue`
//! files in such a cwd by pointing `SHAKA_CUE_MODULE_DIR` at a bundled
//! closure — the layout the generated flake ships under
//! `$out/share/shaka/cue/`.

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
    common::write_test_module(dir); // cue.mod + stub registry + infra stub project
    let real_schema = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
    copy_dir(&real_schema, &dir.join("tools/shaka/schema"));
}

fn write_project(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn schema_check(work: &Path, module: Option<&Path>) -> std::process::Output {
    let mut cmd = Command::new(SHAKA_BIN);
    cmd.args(["project", "schema-check"]).current_dir(work);
    match module {
        Some(dir) => {
            cmd.env(MODULE_DIR_ENV, dir);
        }
        None => {
            cmd.env_remove(MODULE_DIR_ENV);
        }
    }
    cmd.output().expect("failed to spawn shaka")
}

#[test]
fn resolves_bundled_module_outside_a_cue_module() {
    let module = tempfile::TempDir::new().unwrap();
    stage_module(module.path());

    // A consumer checkout that is NOT a CUE module — no `cue.mod`
    // anywhere above it, exactly what a nix-packaged shaka faces.
    let work = tempfile::TempDir::new().unwrap();
    write_project(
        work.path(),
        "apps/foo/project.cue",
        "package project\n#Project & { name: \"foo\", kind: \"infra\" }\n",
    );

    // Without the bundled module, resolution finds no `cue.mod` and fails.
    let without = schema_check(work.path(), None);
    assert!(
        !without.status.success(),
        "expected schema-check to fail with no cue.mod and no {MODULE_DIR_ENV}"
    );

    // With it, the same project validates.
    let with = schema_check(work.path(), Some(module.path()));
    assert!(
        with.status.success(),
        "schema-check should pass via {MODULE_DIR_ENV}; stderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&with.stderr),
        String::from_utf8_lossy(&with.stdout),
    );
}

#[test]
fn bundled_module_still_rejects_invalid_project() {
    let module = tempfile::TempDir::new().unwrap();
    stage_module(module.path());

    let work = tempfile::TempDir::new().unwrap();
    // `kind: "bogus"` matches no variant of the schema's `#Project`
    // disjunction — proving the bundled schema is actually evaluated,
    // not trivially skipped.
    write_project(
        work.path(),
        "apps/foo/project.cue",
        "package project\n#Project & { name: \"foo\", kind: \"bogus\" }\n",
    );

    let out = schema_check(work.path(), Some(module.path()));
    assert!(
        !out.status.success(),
        "schema-check should reject an invalid kind; stdout:\n{}",
        String::from_utf8_lossy(&out.stdout),
    );
}
