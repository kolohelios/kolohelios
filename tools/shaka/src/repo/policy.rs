//! Loader for `.shaka/repo.cue` — the per-repo policy file that drives
//! `shaka repo audit`.
//!
//! `load(start)` walks upward from `start` looking for `.shaka/repo.cue`,
//! validates and exports it via `cue export` against the embedded
//! `repo-policy-schema.cue`, and returns a typed `RepoPolicy`. Optional
//! groups (`branchProtection`, `rulesets`, `security`, `issues`) come back
//! as `None` when omitted, so consumers can opt out of audit dimensions
//! they don't care about.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

pub const SCHEMA: &str = include_str!("../../schema/repo-policy-schema.cue");

const POLICY_FILE_REL: &str = ".shaka/repo.cue";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepoPolicy {
    pub default_branch: String,
    pub merge: MergePolicy,
    #[serde(default)]
    pub issues: Option<bool>,
    #[serde(default)]
    pub branch_protection: Option<BranchProtectionPolicy>,
    #[serde(default)]
    pub rulesets: Option<RulesetsPolicy>,
    #[serde(default)]
    pub security: Option<SecurityPolicy>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergePolicy {
    pub rebase: bool,
    pub merge: bool,
    pub squash: bool,
    pub delete_branch_on_merge: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BranchProtectionPolicy {
    pub required_checks: Vec<String>,
    pub strict_status_checks: bool,
    pub allow_force_push: bool,
    pub allow_deletion: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RulesetsPolicy {
    pub require_deletion: bool,
    pub require_non_fast_forward: bool,
    pub require_status_checks: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecurityPolicy {
    pub dependabot_alerts: bool,
}

pub fn find_policy_file(start: &Path) -> Option<PathBuf> {
    let mut current: PathBuf = start.canonicalize().ok()?;
    loop {
        let candidate = current.join(POLICY_FILE_REL);
        if candidate.is_file() {
            return Some(candidate);
        }
        let parent = current.parent()?;
        current = parent.to_path_buf();
    }
}

pub fn load(start: &Path) -> Result<RepoPolicy, String> {
    let policy_path = find_policy_file(start).ok_or_else(|| {
        format!(
            "no {POLICY_FILE_REL} found in {} or any ancestor",
            start.display()
        )
    })?;

    // Per-process schema temp file — same pattern as project::schema_check.
    let schema_path =
        write_schema().map_err(|e| format!("could not write repo-policy schema: {e}"))?;

    let output = Command::new("cue")
        .arg("export")
        .arg("--out")
        .arg("json")
        .arg(&schema_path)
        .arg(&policy_path)
        .output()
        .map_err(|e| format!("failed to spawn cue: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!(
            "cue export failed for {}: {detail}",
            policy_path.display()
        ));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("could not parse {}: {e}", policy_path.display()))
}

fn write_schema() -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "shaka-repo-policy-schema-{}.cue",
        std::process::id()
    ));
    let mut f = std::fs::File::create(&path)?;
    f.write_all(SCHEMA.as_bytes())?;
    Ok(path)
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
    fn find_policy_file_finds_at_start() {
        let tmp = TempDir::new().unwrap();
        write(&tmp.path().join(".shaka/repo.cue"), "");
        let found = find_policy_file(tmp.path()).unwrap();
        assert!(found.ends_with(".shaka/repo.cue"));
    }

    #[test]
    fn find_policy_file_walks_up() {
        let tmp = TempDir::new().unwrap();
        write(&tmp.path().join(".shaka/repo.cue"), "");
        let nested = tmp.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        let found = find_policy_file(&nested).unwrap();
        assert!(found.ends_with(".shaka/repo.cue"));
    }

    #[test]
    fn find_policy_file_returns_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        assert!(find_policy_file(tmp.path()).is_none());
    }
}
