use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::project::schema_check;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

const AUDIT_CONFIG_SCHEMA: &str = include_str!("../../schema/audit-config.cue");
const AUDIT_CONFIG_DATA: &str = include_str!("../../audit-config.cue");

#[derive(Debug, PartialEq, Eq)]
pub enum RuleResult {
    Pass,
    Fail(String),
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Fail,
    Off,
}

#[derive(Debug, Deserialize)]
struct AuditConfigEntry {
    name: String,
    severity: Severity,
}

#[derive(Debug, Deserialize)]
struct AuditConfig {
    rules: Vec<AuditConfigEntry>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectKind {
    Rust,
    Infra,
    NixLib,
}

#[derive(Debug, Deserialize)]
pub struct CoverageThreshold {
    pub fail: u32,
}

#[derive(Debug, Deserialize)]
pub struct Coverage {
    pub line: CoverageThreshold,
    pub branch: CoverageThreshold,
}

#[derive(Debug, Deserialize)]
pub struct AuditOverride {
    pub rule: String,
    pub severity: Severity,
    #[allow(dead_code)]
    pub justification: String,
}

#[derive(Debug, Deserialize)]
pub struct ProjectAuditConfig {
    #[serde(default)]
    pub overrides: Vec<AuditOverride>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectMeta {
    #[allow(dead_code)]
    pub name: String,
    pub kind: ProjectKind,
    #[serde(default)]
    pub coverage: Option<Coverage>,
    #[serde(default)]
    pub audit: Option<ProjectAuditConfig>,
}

pub trait Rule {
    fn name(&self) -> &'static str;
    fn applies(&self, meta: &ProjectMeta) -> bool;
    fn check(&self, project_dir: &Path, meta: &ProjectMeta) -> RuleResult;
}

struct ReadmePresent;
struct GitignorePresent;
struct RustHasTests;
struct RustCoverageThresholdNonzero;
struct RustLicenseDual;
struct KoloheliosNixViaFlakehub;

const REQUIRED_RUST_LICENSE: &str = r#"license = "MIT OR Apache-2.0""#;
const REQUIRED_KOLOHELIOS_NIX_URL: &str =
    "https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz";

impl Rule for ReadmePresent {
    fn name(&self) -> &'static str {
        "readme-present"
    }
    fn applies(&self, _meta: &ProjectMeta) -> bool {
        true
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        if project_dir.join("README.md").is_file() {
            RuleResult::Pass
        } else {
            RuleResult::Fail("missing README.md at project root".into())
        }
    }
}

impl Rule for GitignorePresent {
    fn name(&self) -> &'static str {
        "gitignore-present"
    }
    fn applies(&self, _meta: &ProjectMeta) -> bool {
        true
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        if project_dir.join(".gitignore").is_file() {
            RuleResult::Pass
        } else {
            RuleResult::Fail("missing .gitignore at project root".into())
        }
    }
}

impl Rule for RustHasTests {
    fn name(&self) -> &'static str {
        "rust-has-tests"
    }
    fn applies(&self, meta: &ProjectMeta) -> bool {
        meta.kind == ProjectKind::Rust
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        if has_rust_tests(project_dir) {
            RuleResult::Pass
        } else {
            RuleResult::Fail("no #[cfg(test)] modules and no tests/ directory found".into())
        }
    }
}

impl Rule for RustCoverageThresholdNonzero {
    fn name(&self) -> &'static str {
        "rust-coverage-threshold-nonzero"
    }
    fn applies(&self, meta: &ProjectMeta) -> bool {
        meta.kind == ProjectKind::Rust
    }
    fn check(&self, _project_dir: &Path, meta: &ProjectMeta) -> RuleResult {
        let Some(cov) = &meta.coverage else {
            return RuleResult::Fail("rust project missing coverage block".into());
        };
        if cov.line.fail == 0 || cov.branch.fail == 0 {
            return RuleResult::Fail(format!(
                "coverage thresholds must be non-zero (line={}, branch={})",
                cov.line.fail, cov.branch.fail
            ));
        }
        RuleResult::Pass
    }
}

impl Rule for RustLicenseDual {
    fn name(&self) -> &'static str {
        "rust-license-dual"
    }
    fn applies(&self, meta: &ProjectMeta) -> bool {
        meta.kind == ProjectKind::Rust
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        let cargo = project_dir.join("Cargo.toml");
        let contents = match std::fs::read_to_string(&cargo) {
            Ok(c) => c,
            Err(_) => return RuleResult::Fail("missing Cargo.toml at project root".into()),
        };
        if contents.contains(REQUIRED_RUST_LICENSE) {
            RuleResult::Pass
        } else {
            RuleResult::Fail(format!(
                "Cargo.toml must declare `{REQUIRED_RUST_LICENSE}` (matches the repo's dual license)"
            ))
        }
    }
}

impl Rule for KoloheliosNixViaFlakehub {
    fn name(&self) -> &'static str {
        "kolohelios-nix-via-flakehub"
    }
    fn applies(&self, _meta: &ProjectMeta) -> bool {
        true
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        let flake = project_dir.join("flake.nix");
        let Ok(contents) = std::fs::read_to_string(&flake) else {
            return RuleResult::Pass;
        };
        match extract_kolohelios_nix_url(&contents) {
            None => RuleResult::Pass,
            Some(url) if url == REQUIRED_KOLOHELIOS_NIX_URL => RuleResult::Pass,
            Some(url) => RuleResult::Fail(format!(
                "kolohelios-nix input must use FlakeHub URL `{REQUIRED_KOLOHELIOS_NIX_URL}` (found: `{url}`)"
            )),
        }
    }
}

// Parses an inline `kolohelios-nix.url = "<url>";` declaration, accepting
// both the in-block form (inside `inputs = { ... }`) and the top-level
// `inputs.kolohelios-nix.url = "..."` form. Returns the URL if found,
// ignoring lines that are commented out (whitespace then `#`). Block-form
// (`kolohelios-nix = { url = "..."; }`) is intentionally not supported —
// every current consumer uses the inline form, and the rule fails closed
// (returns None → Pass via "rule didn't apply") rather than silently
// accepting a malformed declaration.
fn extract_kolohelios_nix_url(flake_contents: &str) -> Option<String> {
    for line in flake_contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let Some(after_attr) = trimmed
            .strip_prefix("inputs.kolohelios-nix.url")
            .or_else(|| trimmed.strip_prefix("kolohelios-nix.url"))
        else {
            continue;
        };
        let Some(after_eq) = after_attr.trim_start().strip_prefix('=') else {
            continue;
        };
        let Some(after_quote) = after_eq.trim_start().strip_prefix('"') else {
            continue;
        };
        let Some(end) = after_quote.find('"') else {
            continue;
        };
        return Some(after_quote[..end].to_string());
    }
    None
}

fn rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(ReadmePresent),
        Box::new(GitignorePresent),
        Box::new(RustHasTests),
        Box::new(RustCoverageThresholdNonzero),
        Box::new(RustLicenseDual),
        Box::new(KoloheliosNixViaFlakehub),
    ]
}

pub fn run() {
    let schema_path = match schema_check::write_schema() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} could not write schema: {e}");
            std::process::exit(1);
        }
    };

    let rules = rules();

    let severities = match load_audit_config() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} could not load audit-config.cue: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = validate_severities(&severities, &rules) {
        eprintln!("{RED}{BOLD}error:{RESET} audit-config / rule registry mismatch:\n{e}");
        std::process::exit(1);
    }

    let projects = schema_check::discover(Path::new("."));
    if projects.is_empty() {
        println!("{YELLOW}no projects found{RESET}");
        return;
    }

    println!("{BOLD}audit:{RESET} {} projects", projects.len());

    let mut failures = 0usize;

    for project in &projects {
        audit_project(project, &schema_path, &rules, &severities, &mut failures);
    }

    println!();
    if failures > 0 {
        eprintln!("{RED}{BOLD}audit failed{RESET} ({failures} failure(s))");
        std::process::exit(1);
    }
    println!("{GREEN}{BOLD}audit passed{RESET}");
}

fn audit_project(
    project: &Path,
    schema_path: &Path,
    rules: &[Box<dyn Rule>],
    severities: &HashMap<String, Severity>,
    failures: &mut usize,
) {
    let display = project.display();
    let cue_path = project.join("project.cue");
    if !cue_path.is_file() {
        println!("  {RED}{BOLD}FAIL{RESET}  {display} ({DIM}missing project.cue{RESET})");
        *failures += 1;
        return;
    }

    let meta = match load_meta(schema_path, &cue_path) {
        Ok(m) => m,
        Err(e) => {
            println!("  {RED}{BOLD}ERROR{RESET} {display} ({DIM}{e}{RESET})");
            *failures += 1;
            return;
        }
    };

    let overrides: &[AuditOverride] = meta
        .audit
        .as_ref()
        .map(|a| a.overrides.as_slice())
        .unwrap_or(&[]);

    let effective = match effective_severities(severities, overrides, rules) {
        Ok(map) => map,
        Err(unknown) => {
            println!(
                "  {RED}{BOLD}FAIL{RESET}  {display} ({DIM}unknown rule(s) in audit.overrides: {}{RESET})",
                unknown.join(", ")
            );
            *failures += unknown.len();
            return;
        }
    };

    let mut project_failures: Vec<(String, String)> = Vec::new();
    let mut applied = 0usize;
    for rule in rules {
        if !rule.applies(&meta) {
            continue;
        }
        if effective.get(rule.name()) == Some(&Severity::Off) {
            continue;
        }
        applied += 1;
        match rule.check(project, &meta) {
            RuleResult::Pass => {}
            RuleResult::Fail(msg) => project_failures.push((rule.name().to_string(), msg)),
        }
    }

    if project_failures.is_empty() {
        println!("  {GREEN}{BOLD}ok{RESET}    {display} ({DIM}{applied} rules{RESET})");
    } else {
        println!(
            "  {RED}{BOLD}FAIL{RESET}  {display} ({DIM}{}/{} rules failed{RESET})",
            project_failures.len(),
            applied
        );
        for (rule, msg) in &project_failures {
            println!("    {RED}{rule}{RESET}: {msg}");
        }
        *failures += project_failures.len();
    }
}

fn load_meta(schema_path: &Path, project_cue: &Path) -> Result<ProjectMeta, String> {
    let output = Command::new("cue")
        .arg("export")
        .arg("--out")
        .arg("json")
        .arg(schema_path)
        .arg(project_cue)
        .output()
        .map_err(|e| format!("failed to spawn cue: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("cue export failed: {detail}"));
    }

    serde_json::from_slice(&output.stdout).map_err(|e| format!("could not parse project.cue: {e}"))
}

fn load_audit_config() -> Result<HashMap<String, Severity>, String> {
    let dir = std::env::temp_dir().join(format!("shaka-audit-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create temp dir: {e}"))?;
    let schema_path = dir.join("schema-audit-config.cue");
    let data_path = dir.join("audit-config.cue");
    std::fs::write(&schema_path, AUDIT_CONFIG_SCHEMA)
        .map_err(|e| format!("could not write audit schema: {e}"))?;
    std::fs::write(&data_path, AUDIT_CONFIG_DATA)
        .map_err(|e| format!("could not write audit config: {e}"))?;

    let output = Command::new("cue")
        .arg("export")
        .arg("--out")
        .arg("json")
        .arg(&schema_path)
        .arg(&data_path)
        .output()
        .map_err(|e| format!("failed to spawn cue: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("cue export failed: {detail}"));
    }
    let cfg: AuditConfig = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("could not parse audit-config: {e}"))?;

    let mut map = HashMap::new();
    for entry in cfg.rules {
        if map.insert(entry.name.clone(), entry.severity).is_some() {
            return Err(format!("duplicate audit rule name: {}", entry.name));
        }
    }
    Ok(map)
}

fn effective_severities(
    base: &HashMap<String, Severity>,
    overrides: &[AuditOverride],
    rules: &[Box<dyn Rule>],
) -> Result<HashMap<String, Severity>, Vec<String>> {
    let registry: HashSet<&str> = rules.iter().map(|r| r.name()).collect();
    let mut unknown: Vec<String> = overrides
        .iter()
        .filter(|ov| !registry.contains(ov.rule.as_str()))
        .map(|ov| ov.rule.clone())
        .collect();
    unknown.sort();
    unknown.dedup();
    if !unknown.is_empty() {
        return Err(unknown);
    }
    let mut effective = base.clone();
    for ov in overrides {
        effective.insert(ov.rule.clone(), ov.severity);
    }
    Ok(effective)
}

fn validate_severities(
    severities: &HashMap<String, Severity>,
    rules: &[Box<dyn Rule>],
) -> Result<(), String> {
    let registry: HashSet<&str> = rules.iter().map(|r| r.name()).collect();
    let mut errors = Vec::new();

    let mut unknown: Vec<&String> = severities
        .keys()
        .filter(|k| !registry.contains(k.as_str()))
        .collect();
    unknown.sort();
    for name in unknown {
        errors.push(format!("  unknown rule in audit-config: {name}"));
    }

    let mut missing: Vec<&str> = rules
        .iter()
        .map(|r| r.name())
        .filter(|n| !severities.contains_key(*n))
        .collect();
    missing.sort();
    for name in missing {
        errors.push(format!("  audit-config missing severity for rule: {name}"));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn has_rust_tests(project_dir: &Path) -> bool {
    let tests_dir = project_dir.join("tests");
    if dir_contains_rs(&tests_dir) {
        return true;
    }
    any_rs_file_contains(project_dir, "#[cfg(test)]")
}

fn dir_contains_rs(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            return true;
        }
        if path.is_dir() && dir_contains_rs(&path) {
            return true;
        }
    }
    false
}

fn any_rs_file_contains(dir: &Path, needle: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if matches!(
                name_str.as_ref(),
                "target" | ".git" | ".jj" | "node_modules"
            ) || name_str.starts_with("result")
            {
                continue;
            }
            if any_rs_file_contains(&path, needle) {
                return true;
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if contents.contains(needle) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn rust_meta() -> ProjectMeta {
        ProjectMeta {
            name: "demo".into(),
            kind: ProjectKind::Rust,
            coverage: Some(Coverage {
                line: CoverageThreshold { fail: 30 },
                branch: CoverageThreshold { fail: 20 },
            }),
            audit: None,
        }
    }

    fn infra_meta() -> ProjectMeta {
        ProjectMeta {
            name: "demo".into(),
            kind: ProjectKind::Infra,
            coverage: None,
            audit: None,
        }
    }

    #[test]
    fn readme_present_passes_when_readme_exists() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("README.md"), "# demo").unwrap();
        assert_eq!(
            ReadmePresent.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn readme_present_fails_when_readme_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(
            ReadmePresent.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn readme_present_rejects_directory_named_readme() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("README.md")).unwrap();
        assert!(matches!(
            ReadmePresent.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn gitignore_present_passes_when_file_exists() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".gitignore"), "target/\n").unwrap();
        assert_eq!(
            GitignorePresent.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn gitignore_present_fails_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(
            GitignorePresent.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn rust_has_tests_only_applies_to_rust() {
        assert!(RustHasTests.applies(&rust_meta()));
        assert!(!RustHasTests.applies(&infra_meta()));
    }

    #[test]
    fn rust_has_tests_passes_for_inline_cfg_test() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\n#[cfg(test)]\nmod tests {}\n",
        )
        .unwrap();
        assert_eq!(
            RustHasTests.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_has_tests_passes_for_tests_directory() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("tests")).unwrap();
        fs::write(tmp.path().join("tests/integration.rs"), "// integration\n").unwrap();
        assert_eq!(
            RustHasTests.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_has_tests_passes_for_nested_tests_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("tests/sub")).unwrap();
        fs::write(tmp.path().join("tests/sub/foo.rs"), "// nested\n").unwrap();
        assert_eq!(
            RustHasTests.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_has_tests_fails_when_no_tests_anywhere() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn untested() -> u32 { 42 }\n",
        )
        .unwrap();
        assert!(matches!(
            RustHasTests.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn rust_has_tests_ignores_target_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("target/debug/deps")).unwrap();
        fs::write(
            tmp.path().join("target/debug/deps/build.rs"),
            "// generated build artifact\n#[cfg(test)]\nmod tests {}\n",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src/lib.rs"), "// no tests here\n").unwrap();
        assert!(matches!(
            RustHasTests.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn rust_has_tests_empty_tests_dir_does_not_count() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("tests")).unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src/lib.rs"), "// nothing\n").unwrap();
        assert!(matches!(
            RustHasTests.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn coverage_threshold_only_applies_to_rust() {
        assert!(RustCoverageThresholdNonzero.applies(&rust_meta()));
        assert!(!RustCoverageThresholdNonzero.applies(&infra_meta()));
    }

    #[test]
    fn coverage_threshold_passes_for_nonzero_values() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            RustCoverageThresholdNonzero.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn coverage_threshold_fails_for_zero_line() {
        let tmp = TempDir::new().unwrap();
        let meta = ProjectMeta {
            coverage: Some(Coverage {
                line: CoverageThreshold { fail: 0 },
                branch: CoverageThreshold { fail: 20 },
            }),
            ..rust_meta()
        };
        assert!(matches!(
            RustCoverageThresholdNonzero.check(tmp.path(), &meta),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn coverage_threshold_fails_for_zero_branch() {
        let tmp = TempDir::new().unwrap();
        let meta = ProjectMeta {
            coverage: Some(Coverage {
                line: CoverageThreshold { fail: 30 },
                branch: CoverageThreshold { fail: 0 },
            }),
            ..rust_meta()
        };
        assert!(matches!(
            RustCoverageThresholdNonzero.check(tmp.path(), &meta),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn coverage_threshold_fails_when_block_missing() {
        let tmp = TempDir::new().unwrap();
        let meta = ProjectMeta {
            coverage: None,
            ..rust_meta()
        };
        assert!(matches!(
            RustCoverageThresholdNonzero.check(tmp.path(), &meta),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn rust_license_dual_only_applies_to_rust() {
        assert!(RustLicenseDual.applies(&rust_meta()));
        assert!(!RustLicenseDual.applies(&infra_meta()));
    }

    #[test]
    fn rust_license_dual_passes_for_canonical_license() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"MIT OR Apache-2.0\"\n",
        )
        .unwrap();
        assert_eq!(
            RustLicenseDual.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_license_dual_fails_when_cargo_toml_missing() {
        let tmp = TempDir::new().unwrap();
        match RustLicenseDual.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("missing Cargo.toml"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_license_dual_fails_when_license_field_absent() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        assert!(matches!(
            RustLicenseDual.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn rust_license_dual_fails_for_different_license_value() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"GPL-3.0\"\n",
        )
        .unwrap();
        assert!(matches!(
            RustLicenseDual.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn kolohelios_nix_via_flakehub_passes_when_no_flake_nix() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            KoloheliosNixViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn kolohelios_nix_via_flakehub_passes_when_flake_does_not_reference_input() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs\"; outputs = { ... }: { }; }\n",
        )
        .unwrap();
        assert_eq!(
            KoloheliosNixViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn kolohelios_nix_via_flakehub_passes_for_canonical_url() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{\n  inputs = {\n    kolohelios-nix.url = \"https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz\";\n    nixpkgs.follows = \"kolohelios-nix/nixpkgs\";\n  };\n}\n",
        )
        .unwrap();
        assert_eq!(
            KoloheliosNixViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn kolohelios_nix_via_flakehub_fails_for_path_input() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{\n  inputs = {\n    kolohelios-nix.url = \"path:../../nix/kolohelios-nix\";\n  };\n}\n",
        )
        .unwrap();
        match KoloheliosNixViaFlakehub.check(tmp.path(), &infra_meta()) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("path:../../nix/kolohelios-nix"), "got: {msg}");
                assert!(msg.contains("FlakeHub URL"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn kolohelios_nix_via_flakehub_fails_for_github_url() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{\n  inputs.kolohelios-nix.url = \"github:kolohelios/kolohelios-nix\";\n}\n",
        )
        .unwrap();
        assert!(matches!(
            KoloheliosNixViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn kolohelios_nix_via_flakehub_ignores_comment_only_mentions() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{\n  # kolohelios-nix.url is consumed via path: input from a sibling lib\n  inputs.nixpkgs.url = \"github:NixOS/nixpkgs\";\n}\n",
        )
        .unwrap();
        assert_eq!(
            KoloheliosNixViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_license_dual_rejects_reversed_dual_form() {
        // SPDX considers `MIT OR Apache-2.0` and `Apache-2.0 OR MIT`
        // semantically equivalent, but textual canonicalization keeps the
        // lint check trivial. If we ever need to relax, parse the field.
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"Apache-2.0 OR MIT\"\n",
        )
        .unwrap();
        assert!(matches!(
            RustLicenseDual.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    fn severities_all_fail() -> HashMap<String, Severity> {
        rules()
            .iter()
            .map(|r| (r.name().to_string(), Severity::Fail))
            .collect()
    }

    #[test]
    fn embedded_audit_config_loads_and_matches_registry() {
        let severities = load_audit_config().expect("audit-config.cue must load");
        validate_severities(&severities, &rules())
            .expect("embedded audit-config must cover every registered rule");
    }

    #[test]
    fn validate_severities_errors_on_unknown_rule_name() {
        let mut severities = severities_all_fail();
        severities.insert("ghost-rule".into(), Severity::Fail);
        match validate_severities(&severities, &rules()) {
            Err(msg) => assert!(msg.contains("ghost-rule"), "got: {msg}"),
            Ok(()) => panic!("expected Err for unknown rule"),
        }
    }

    #[test]
    fn validate_severities_errors_on_missing_rule() {
        let mut severities = severities_all_fail();
        severities.remove("readme-present");
        match validate_severities(&severities, &rules()) {
            Err(msg) => assert!(msg.contains("readme-present"), "got: {msg}"),
            Ok(()) => panic!("expected Err for missing rule"),
        }
    }

    #[test]
    fn validate_severities_passes_when_every_rule_has_a_severity() {
        validate_severities(&severities_all_fail(), &rules())
            .expect("all-fail map must satisfy parity check");
    }

    fn ov(rule: &str, severity: Severity) -> AuditOverride {
        AuditOverride {
            rule: rule.into(),
            severity,
            justification: "test".into(),
        }
    }

    #[test]
    fn effective_severities_no_overrides_returns_base() {
        let base = severities_all_fail();
        let got = effective_severities(&base, &[], &rules()).expect("clean parity");
        assert_eq!(got, base);
    }

    #[test]
    fn effective_severities_override_disables_rule() {
        let base = severities_all_fail();
        let overrides = [ov("rust-has-tests", Severity::Off)];
        let got = effective_severities(&base, &overrides, &rules()).expect("clean parity");
        assert_eq!(got["rust-has-tests"], Severity::Off);
        assert_eq!(got["readme-present"], Severity::Fail);
    }

    #[test]
    fn effective_severities_override_can_raise_off_to_fail() {
        let mut base = severities_all_fail();
        base.insert("rust-has-tests".into(), Severity::Off);
        let overrides = [ov("rust-has-tests", Severity::Fail)];
        let got = effective_severities(&base, &overrides, &rules()).expect("clean parity");
        assert_eq!(got["rust-has-tests"], Severity::Fail);
    }

    #[test]
    fn effective_severities_unknown_override_rule_errors() {
        let base = severities_all_fail();
        let overrides = [ov("not-a-real-rule", Severity::Off)];
        match effective_severities(&base, &overrides, &rules()) {
            Err(unknown) => {
                assert_eq!(unknown, vec!["not-a-real-rule"]);
            }
            Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn effective_severities_dedups_unknown_names() {
        let base = severities_all_fail();
        let overrides = [ov("ghost-a", Severity::Off), ov("ghost-a", Severity::Fail)];
        match effective_severities(&base, &overrides, &rules()) {
            Err(unknown) => assert_eq!(unknown, vec!["ghost-a"]),
            Ok(_) => panic!("expected Err"),
        }
    }
}
