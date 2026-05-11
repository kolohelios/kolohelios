//! Invoke `cue export` to evaluate user-authored CUE in a project's
//! `cue/` directory against the terraform schema, returning JSON ready
//! to be walked into the IR (see `ir.rs`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use snafu::{ResultExt, Snafu};

const SCHEMA: &str = include_str!("../../schema/terraform-schema.cue");

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum CueError {
    #[snafu(display("could not write schema to temp file: {source}"))]
    WriteSchema { source: std::io::Error },
    #[snafu(display("cue directory does not exist: {dir}"))]
    MissingCueDir { dir: String },
    #[snafu(display("no .cue files found in {dir}"))]
    NoCueFiles { dir: String },
    #[snafu(display("could not enumerate cue directory {dir}: {source}"))]
    ReadCueDir { dir: String, source: std::io::Error },
    #[snafu(display("failed to spawn cue: {source}"))]
    SpawnCue { source: std::io::Error },
    #[snafu(display("cue export failed:\n{stderr}"))]
    CueExport { stderr: String },
    #[snafu(display("cue export produced invalid JSON: {source}"))]
    ParseJson { source: serde_json::Error },
}

/// Write the embedded schema to a temp file. Caller is responsible for
/// the lifetime of the returned path; we use a process-id-suffixed name
/// to keep concurrent invocations from clobbering each other.
fn write_schema() -> Result<PathBuf, CueError> {
    let path =
        std::env::temp_dir().join(format!("shaka-terraform-schema-{}.cue", std::process::id()));
    let mut f = std::fs::File::create(&path).context(WriteSchemaSnafu)?;
    f.write_all(SCHEMA.as_bytes()).context(WriteSchemaSnafu)?;
    Ok(path)
}

/// Collect `*.cue` files in `dir`, sorted.
fn cue_files(dir: &Path) -> Result<Vec<PathBuf>, CueError> {
    let entries = std::fs::read_dir(dir).with_context(|_| ReadCueDirSnafu {
        dir: dir.display().to_string(),
    })?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("cue"))
        .collect();
    files.sort();
    Ok(files)
}

/// Evaluate the CUE files in `cue_dir` against the terraform schema,
/// returning the resolved JSON document. The schema is the embedded copy
/// of `tools/shaka/schema/terraform-schema.cue`.
pub fn load(cue_dir: &Path) -> Result<serde_json::Value, CueError> {
    if !cue_dir.is_dir() {
        return MissingCueDirSnafu {
            dir: cue_dir.display().to_string(),
        }
        .fail();
    }
    let files = cue_files(cue_dir)?;
    if files.is_empty() {
        return NoCueFilesSnafu {
            dir: cue_dir.display().to_string(),
        }
        .fail();
    }

    let schema_path = write_schema()?;

    let output = Command::new("cue")
        .args(["export", "--out", "json"])
        .arg(&schema_path)
        .args(&files)
        .output()
        .context(SpawnCueSnafu)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return CueExportSnafu { stderr }.fail();
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        // Empty module (schema permits this — all top-level fields are
        // optional). Represent as an empty JSON object.
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    serde_json::from_str(&stdout).context(ParseJsonSnafu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn load_errors_when_cue_dir_missing() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("nope");
        match load(&missing) {
            Err(CueError::MissingCueDir { .. }) => {}
            other => panic!("expected MissingCueDir, got {other:?}"),
        }
    }

    #[test]
    fn load_errors_when_no_cue_files() {
        let tmp = TempDir::new().unwrap();
        match load(tmp.path()) {
            Err(CueError::NoCueFiles { .. }) => {}
            other => panic!("expected NoCueFiles, got {other:?}"),
        }
    }

    #[test]
    fn load_returns_empty_object_for_schema_only_module() {
        let tmp = TempDir::new().unwrap();
        // Just `package terraform` with no fields — schema accepts this.
        fs::write(tmp.path().join("empty.cue"), "package terraform\n").unwrap();
        let v = load(tmp.path()).expect("cue export should succeed");
        assert!(v.is_object());
        assert!(v.as_object().unwrap().is_empty());
    }

    #[test]
    fn load_parses_minimal_resource_module() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("module.cue"),
            r#"package terraform

resources: [{
    type: "linode_instance"
    name: "devbox"
    attributes: {
        label: "devbox"
        region: {"$ref": "var.region"}
    }
}]
"#,
        )
        .unwrap();
        let v = load(tmp.path()).expect("cue export should succeed");
        let resources = v["resources"].as_array().expect("resources array");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0]["type"], "linode_instance");
        assert_eq!(resources[0]["attributes"]["label"], "devbox");
        assert_eq!(resources[0]["attributes"]["region"]["$ref"], "var.region");
    }

    #[test]
    fn load_rejects_invalid_module_per_schema() {
        let tmp = TempDir::new().unwrap();
        // resources requires both `type` and `name`; this only has `type`.
        fs::write(
            tmp.path().join("bad.cue"),
            r#"package terraform

resources: [{type: "linode_instance"}]
"#,
        )
        .unwrap();
        match load(tmp.path()) {
            Err(CueError::CueExport { .. }) => {}
            other => panic!("expected CueExport, got {other:?}"),
        }
    }
}
