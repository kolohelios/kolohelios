//! Loader for `.shaka/labels.cue` — the canonical GitHub label set
//! that drives `shaka issue label sync` and `shaka issue audit`.
//!
//! `load(start)` walks upward from `start` looking for
//! `.shaka/labels.cue`, validates and exports it via `cue export`
//! against the embedded `labels-schema.cue`, and returns a typed
//! `LabelSet`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

pub const SCHEMA: &str = include_str!("../../schema/labels-schema.cue");

const LABELS_FILE_REL: &str = ".shaka/labels.cue";

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub name: String,
    pub color: String,
    pub description: String,
    pub scope: bool,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LabelSet {
    pub labels: Vec<Label>,
}

impl LabelSet {
    pub fn scope_names(&self) -> Vec<&str> {
        self.labels
            .iter()
            .filter(|l| l.scope)
            .map(|l| l.name.as_str())
            .collect()
    }
}

pub fn find_labels_file(start: &Path) -> Option<PathBuf> {
    let mut current: PathBuf = start.canonicalize().ok()?;
    loop {
        let candidate = current.join(LABELS_FILE_REL);
        if candidate.is_file() {
            return Some(candidate);
        }
        let parent = current.parent()?;
        current = parent.to_path_buf();
    }
}

pub fn load(start: &Path) -> Result<LabelSet, String> {
    let labels_path = find_labels_file(start).ok_or_else(|| {
        format!(
            "no {LABELS_FILE_REL} found in {} or any ancestor",
            start.display()
        )
    })?;
    let cue = std::fs::read_to_string(&labels_path)
        .map_err(|e| format!("could not read {}: {e}", labels_path.display()))?;
    load_from_string(&cue).map_err(|e| format!("{}: {e}", labels_path.display()))
}

pub fn load_from_string(cue: &str) -> Result<LabelSet, String> {
    let tmp = tempfile::TempDir::new().map_err(|e| format!("could not create tempdir: {e}"))?;
    let schema_path = tmp.path().join("schema.cue");
    std::fs::write(&schema_path, SCHEMA)
        .map_err(|e| format!("could not write labels schema: {e}"))?;
    let labels_path = tmp.path().join("labels.cue");
    std::fs::write(&labels_path, cue)
        .map_err(|e| format!("could not write temp labels file: {e}"))?;

    let output = Command::new("cue")
        .arg("export")
        .arg("--out")
        .arg("json")
        .arg(&schema_path)
        .arg(&labels_path)
        .output()
        .map_err(|e| format!("failed to spawn cue: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("cue export failed: {detail}"));
    }

    serde_json::from_slice(&output.stdout).map_err(|e| format!("could not parse labels: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn find_labels_file_finds_at_start() {
        let tmp = TempDir::new().unwrap();
        write(&tmp.path().join(".shaka/labels.cue"), "");
        let found = find_labels_file(tmp.path()).unwrap();
        assert!(found.ends_with(".shaka/labels.cue"));
    }

    #[test]
    fn find_labels_file_walks_up() {
        let tmp = TempDir::new().unwrap();
        write(&tmp.path().join(".shaka/labels.cue"), "");
        let nested = tmp.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        let found = find_labels_file(&nested).unwrap();
        assert!(found.ends_with(".shaka/labels.cue"));
    }

    #[test]
    fn find_labels_file_returns_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        assert!(find_labels_file(tmp.path()).is_none());
    }

    // Schema-level acceptance/rejection coverage lives in
    // `tests/labels_schema.rs` via fixture files. This smoke test
    // exists to exercise the Rust deserialization path end-to-end.
    #[test]
    fn load_from_string_parses_label_set() {
        let cue = r#"package labels

#LabelSet & {
    labels: [
        {name: "shaka", color: "5319e7", description: "shaka CLI", scope: true},
        {name: "bug",   color: "d73a4a", description: "broken",    scope: false},
    ]
}
"#;
        let set = load_from_string(cue).expect("parses");
        assert_eq!(set.labels.len(), 2);
        assert_eq!(set.labels[0].name, "shaka");
        assert!(set.labels[0].scope);
        assert_eq!(set.labels[1].name, "bug");
        assert!(!set.labels[1].scope);
        assert_eq!(set.scope_names(), vec!["shaka"]);
    }
}
