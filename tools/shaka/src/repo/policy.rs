//! Loader for `.shaka/repo.cue` — the per-repo policy file that drives
//! `shaka repo audit`.
//!
//! `load(start)` walks upward from `start` looking for `.shaka/repo.cue`,
//! validates and exports it via `cue export` against the embedded
//! `repo-policy-schema.cue`, and returns a typed `RepoPolicy`. Optional
//! groups (`branchProtection`, `rulesets`, `security`, `issues`) come back
//! as `None` when omitted, so consumers can opt out of audit dimensions
//! they don't care about.

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
    /// Workflow filenames under `.github/workflows/` that are hand-authored
    /// (not emitted by `shaka ci generate-workflows`). `ci audit-workflows`
    /// unions this with its built-in allowlist, so an external consumer can
    /// account for its own hand-authored workflows (e.g. a `just validate`
    /// gate) without hardcoding them in shaka's source.
    #[serde(default)]
    pub hand_authored_workflows: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergePolicy {
    pub rebase: bool,
    pub merge: bool,
    pub squash: bool,
    pub delete_branch_on_merge: bool,
    pub auto_merge: bool,
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
    let cue = std::fs::read_to_string(&policy_path)
        .map_err(|e| format!("could not read {}: {e}", policy_path.display()))?;
    load_from_string(&cue).map_err(|e| format!("{}: {e}", policy_path.display()))
}

/// Parse a `.shaka/repo.cue` from its raw CUE source — used by the
/// audit when `--repo <external>` fetches policy from the target
/// repository instead of walking the cwd.
pub fn load_from_string(cue: &str) -> Result<RepoPolicy, String> {
    // Per-call tempdir so concurrent callers (e.g. parallel cargo
    // tests) don't race on a shared `/tmp/shaka-repo-policy-<pid>/`
    // dir. Auto-cleaned on drop.
    let tmp = tempfile::TempDir::new().map_err(|e| format!("could not create tempdir: {e}"))?;
    let schema_path = tmp.path().join("schema.cue");
    std::fs::write(&schema_path, SCHEMA)
        .map_err(|e| format!("could not write repo-policy schema: {e}"))?;
    let policy_path = tmp.path().join("repo.cue");
    std::fs::write(&policy_path, cue)
        .map_err(|e| format!("could not write temp policy file: {e}"))?;

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
        return Err(format!("cue export failed: {detail}"));
    }

    serde_json::from_slice(&output.stdout).map_err(|e| format!("could not parse policy: {e}"))
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

    #[test]
    fn load_from_string_parses_complete_policy() {
        let cue = r#"package repopolicy

#RepoPolicy & {
    defaultBranch: "main"
    merge: {
        rebase:              true
        merge:               false
        squash:              false
        deleteBranchOnMerge: true
        autoMerge:           true
    }
    branchProtection: {
        requiredChecks: ["Preflight"]
        strictStatusChecks: true
        allowForcePush:     false
        allowDeletion:      false
    }
}
"#;
        let policy = load_from_string(cue).expect("policy parses");
        assert_eq!(policy.default_branch, "main");
        assert!(policy.merge.rebase);
        assert!(policy.merge.auto_merge);
        let bp = policy.branch_protection.expect("branch_protection set");
        assert_eq!(bp.required_checks, vec!["Preflight"]);
    }

    #[test]
    fn load_from_string_rejects_incomplete_policy() {
        // Missing merge.autoMerge — schema requires a concrete value.
        let cue = r#"package repopolicy

#RepoPolicy & {
    defaultBranch: "main"
    merge: {
        rebase:              true
        merge:               false
        squash:              false
        deleteBranchOnMerge: true
    }
}
"#;
        let err = load_from_string(cue).expect_err("incomplete policy rejected");
        assert!(err.contains("autoMerge"), "actual error: {err}");
    }

    fn minimal_policy_with(extra: &str) -> String {
        format!(
            r#"package repopolicy

#RepoPolicy & {{
    defaultBranch: "main"
    merge: {{
        rebase:              true
        merge:               false
        squash:              false
        deleteBranchOnMerge: true
        autoMerge:           true
    }}
{extra}
}}
"#
        )
    }

    #[test]
    fn load_from_string_parses_hand_authored_workflows() {
        let policy = load_from_string(&minimal_policy_with(
            r#"    handAuthoredWorkflows: ["buzzingo-validate.yml"]"#,
        ))
        .expect("policy parses");
        assert_eq!(
            policy.hand_authored_workflows,
            vec!["buzzingo-validate.yml"]
        );
    }

    #[test]
    fn hand_authored_workflows_defaults_to_empty() {
        let policy = load_from_string(&minimal_policy_with("")).expect("policy parses");
        assert!(policy.hand_authored_workflows.is_empty());
    }
}
