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
    RustCli,
    RustWorker,
    Infra,
    NixLib,
    Document,
}

impl ProjectKind {
    /// Rust-flavored kinds share the cargo/clippy/license rules even
    /// when coverage requirements diverge.
    fn is_rust_flavored(self) -> bool {
        matches!(self, ProjectKind::RustCli | ProjectKind::RustWorker)
    }
}

#[derive(Debug, Deserialize)]
pub struct CoverageThreshold {
    pub fail: u32,
}

#[derive(Debug, Deserialize)]
pub struct Coverage {
    pub line: CoverageThreshold,
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
pub struct CliConfig {
    pub package: bool,
}

#[derive(Debug, Deserialize)]
pub struct CiConfig {
    #[serde(default)]
    pub build: Option<serde_json::Value>,
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
    #[serde(default)]
    pub cli: Option<CliConfig>,
    #[serde(default)]
    pub ci: Option<CiConfig>,
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
struct RustVersionPinned;
struct KoloheliosNixViaFlakehub;
struct KoloheliosHomeViaFlakehub;
struct ValidateRecipeMeaningful;
struct RustCliPackageTrueRequiresCiBuild;
struct RustCliPackageFalseRequiresOverride;

const REQUIRED_RUST_LICENSE: &str = "MIT OR Apache-2.0";
const REQUIRED_RUST_VERSION: &str = "1.95";
const REQUIRED_KOLOHELIOS_NIX_URL: &str =
    "https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz";
const REQUIRED_KOLOHELIOS_HOME_URL: &str = "https://flakehub.com/f/kolohelios/home/*.tar.gz";

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
        meta.kind.is_rust_flavored()
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
        meta.kind.is_rust_flavored()
    }
    fn check(&self, _project_dir: &Path, meta: &ProjectMeta) -> RuleResult {
        let Some(cov) = &meta.coverage else {
            // Rust requires coverage; rust-worker treats it as opt-in
            // (cargo-llvm-cov can't see the wasm-target code, so
            // gating on native-only coverage is misleading).
            return match meta.kind {
                ProjectKind::RustCli => {
                    RuleResult::Fail("rust-cli project missing coverage block".into())
                }
                ProjectKind::RustWorker => RuleResult::Pass,
                _ => RuleResult::Pass,
            };
        };
        // Branch coverage isn't measured at all — `cargo-llvm-cov --branch`
        // requires nightly, and every rust crate is pinned to stable. The
        // schema dropped `coverage.branch.fail` in #671; only line
        // coverage is gated here.
        if cov.line.fail == 0 {
            return RuleResult::Fail(format!(
                "line coverage threshold must be non-zero (line={})",
                cov.line.fail
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
        meta.kind.is_rust_flavored()
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        let cargo = project_dir.join("Cargo.toml");
        let contents = match std::fs::read_to_string(&cargo) {
            Ok(c) => c,
            Err(_) => return RuleResult::Fail("missing Cargo.toml at project root".into()),
        };
        match cargo_package_field(&contents, "license") {
            Some(v) if v == REQUIRED_RUST_LICENSE => RuleResult::Pass,
            Some(v) => RuleResult::Fail(format!(
                "Cargo.toml `[package].license` must be `\"{REQUIRED_RUST_LICENSE}\"` (found `\"{v}\"`)"
            )),
            None => RuleResult::Fail(format!(
                "Cargo.toml must declare `[package].license = \"{REQUIRED_RUST_LICENSE}\"`"
            )),
        }
    }
}

impl Rule for RustVersionPinned {
    fn name(&self) -> &'static str {
        "rust-version-pinned"
    }
    fn applies(&self, meta: &ProjectMeta) -> bool {
        meta.kind.is_rust_flavored()
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        let cargo = project_dir.join("Cargo.toml");
        let contents = match std::fs::read_to_string(&cargo) {
            Ok(c) => c,
            Err(_) => return RuleResult::Fail("missing Cargo.toml at project root".into()),
        };
        match cargo_package_field(&contents, "rust-version") {
            Some(v) if v == REQUIRED_RUST_VERSION => RuleResult::Pass,
            Some(v) => RuleResult::Fail(format!(
                "Cargo.toml `[package].rust-version` must be `\"{REQUIRED_RUST_VERSION}\"` (found `\"{v}\"`)"
            )),
            None => RuleResult::Fail(format!(
                "Cargo.toml must declare `[package].rust-version = \"{REQUIRED_RUST_VERSION}\"`"
            )),
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
        match extract_input_url(&contents, "kolohelios-nix") {
            None => RuleResult::Pass,
            Some(url) if url == REQUIRED_KOLOHELIOS_NIX_URL => RuleResult::Pass,
            Some(url) => RuleResult::Fail(format!(
                "kolohelios-nix input must use FlakeHub URL `{REQUIRED_KOLOHELIOS_NIX_URL}` (found: `{url}`)"
            )),
        }
    }
}

impl Rule for KoloheliosHomeViaFlakehub {
    fn name(&self) -> &'static str {
        "kolohelios-home-via-flakehub"
    }
    fn applies(&self, _meta: &ProjectMeta) -> bool {
        true
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        let flake = project_dir.join("flake.nix");
        let Ok(contents) = std::fs::read_to_string(&flake) else {
            return RuleResult::Pass;
        };
        match extract_input_url(&contents, "home-env") {
            None => RuleResult::Pass,
            Some(url) if url == REQUIRED_KOLOHELIOS_HOME_URL => RuleResult::Pass,
            Some(url) => RuleResult::Fail(format!(
                "home-env input must use FlakeHub URL `{REQUIRED_KOLOHELIOS_HOME_URL}` (found: `{url}`)"
            )),
        }
    }
}

impl Rule for ValidateRecipeMeaningful {
    fn name(&self) -> &'static str {
        "validate-recipe-meaningful"
    }
    fn applies(&self, _meta: &ProjectMeta) -> bool {
        true
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        let justfile = project_dir.join("justfile");
        let contents = match std::fs::read_to_string(&justfile) {
            Ok(c) => c,
            Err(_) => return RuleResult::Fail("missing justfile at project root".into()),
        };
        check_validate_recipe(&contents)
    }
}

// `shaka preflight` runs each project's `just validate` recipe as the per-
// project quality gate. A project whose `validate` is empty or runs only
// placeholders (`@true`, `:`) silently passes CI without exercising any
// real check. This rule guards against that drift independent of
// `generate-justfiles --check`, which only enforces template-equality.
fn check_validate_recipe(contents: &str) -> RuleResult {
    let lines: Vec<&str> = contents.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if let Some(deps_str) = recipe_header_deps(lines[i], "validate") {
            let has_deps = deps_str.split_whitespace().next().is_some();
            let mut real_body = false;
            let mut j = i + 1;
            while j < lines.len() {
                let next = lines[j];
                if next.is_empty() {
                    j += 1;
                    continue;
                }
                if !next.starts_with(' ') && !next.starts_with('\t') {
                    break;
                }
                if !is_placeholder_body_line(next.trim_start()) {
                    real_body = true;
                }
                j += 1;
            }
            return if has_deps || real_body {
                RuleResult::Pass
            } else {
                RuleResult::Fail(
                    "`validate` recipe has no dependencies and no non-placeholder body".into(),
                )
            };
        }
        i += 1;
    }
    RuleResult::Fail("no `validate` recipe found in justfile".into())
}

// Returns the dep list (everything after the recipe header's `:`) if `line`
// is a recipe header for `name`. Rejects variable assignments (`name := ...`)
// and indented lines.
fn recipe_header_deps<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let rest = line.strip_prefix(name)?;
    let first = rest.chars().next()?;
    if first == ':' {
        if rest[1..].starts_with('=') {
            return None;
        }
        return Some(&rest[1..]);
    }
    if !first.is_whitespace() {
        return None;
    }
    let idx = rest.find(':')?;
    if rest[idx + 1..].starts_with('=') {
        return None;
    }
    Some(&rest[idx + 1..])
}

// Body lines that don't count as real work: blank, lone comments, and
// shell no-ops (`true`, `:`, with optional `@` modifier).
fn is_placeholder_body_line(stripped: &str) -> bool {
    let body = stripped.strip_prefix('@').unwrap_or(stripped).trim();
    if body.is_empty() {
        return true;
    }
    if body.starts_with('#') {
        return true;
    }
    matches!(body, "true" | ":")
}

// Looks up a string-valued `[package]` field in a Cargo.toml, tolerating
// any whitespace between key, `=`, and value (taplo's `align_entries`
// pads to the longest key in the table, so substring matching against a
// fixed-width assignment is brittle). Returns the literal string value
// without quotes, or `None` if the key isn't present in `[package]`.
// Other sections (`[dependencies]`, `[lib]`, etc.) are skipped to avoid
// confusing `name`/`version` here with package-level fields.
fn cargo_package_field(contents: &str, key: &str) -> Option<String> {
    let mut in_package = false;
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_package = trimmed.starts_with("[package]");
            continue;
        }
        if !in_package || trimmed.starts_with('#') {
            continue;
        }
        let Some(after_key) = trimmed.strip_prefix(key) else {
            continue;
        };
        // Reject when `key` is a prefix of a longer key (for example
        // `name` matching `name-suffix`).
        let next = after_key.chars().next()?;
        if !next.is_whitespace() && next != '=' {
            continue;
        }
        let Some(after_eq) = after_key.trim_start().strip_prefix('=') else {
            continue;
        };
        let after_eq = after_eq.trim_start();
        let Some(after_quote) = after_eq.strip_prefix('"') else {
            continue;
        };
        let Some(end) = after_quote.find('"') else {
            continue;
        };
        return Some(after_quote[..end].to_string());
    }
    None
}

// Parses an inline `<input_name>.url = "<url>";` declaration, accepting
// both the in-block form (inside `inputs = { ... }`) and the top-level
// `inputs.<input_name>.url = "..."` form. Returns the URL if found,
// ignoring lines that are commented out (whitespace then `#`). Block-form
// (`<input_name> = { url = "..."; }`) is intentionally not supported —
// every current consumer uses the inline form, and callers fail closed
// (return None → Pass via "rule didn't apply") rather than silently
// accepting a malformed declaration.
fn extract_input_url(flake_contents: &str, input_name: &str) -> Option<String> {
    let top_prefix = format!("inputs.{input_name}.url");
    let block_prefix = format!("{input_name}.url");
    for line in flake_contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let Some(after_attr) = trimmed
            .strip_prefix(top_prefix.as_str())
            .or_else(|| trimmed.strip_prefix(block_prefix.as_str()))
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

// A rust-cli with `cli.package: true` (the schema default) produces a
// flake `packages.default`. Without a `ci.build` block, CI silently
// neither builds nor publishes that package — a `nix build` consumer
// later misses cache and re-evaluates from source. Catch the missing
// block at audit time instead.
fn rust_cli_package_enabled(meta: &ProjectMeta) -> bool {
    // `cli.package` defaults to true in the cue schema (`*true | false`).
    // When `cli` is absent (non-rust-cli kinds) the caller's `applies`
    // gate already rejected the project, so the default is only reached
    // for a rust-cli whose cli block round-tripped without a package
    // field — treat that as the schema default.
    meta.cli.as_ref().map(|c| c.package).unwrap_or(true)
}

impl Rule for RustCliPackageTrueRequiresCiBuild {
    fn name(&self) -> &'static str {
        "rust-cli-package-true-requires-ci-build"
    }
    fn applies(&self, meta: &ProjectMeta) -> bool {
        meta.kind == ProjectKind::RustCli && rust_cli_package_enabled(meta)
    }
    fn check(&self, _project_dir: &Path, meta: &ProjectMeta) -> RuleResult {
        let has_build = meta.ci.as_ref().and_then(|c| c.build.as_ref()).is_some();
        if has_build {
            RuleResult::Pass
        } else {
            RuleResult::Fail(
                "rust-cli with cli.package: true must declare ci.build (the flake produces a package; CI should build and publish it)".into(),
            )
        }
    }
}

// Mirror rule: a rust-cli that explicitly opts out of `cli.package`
// fails by default, and the project must add an `audit.overrides`
// entry (severity: "off") naming this rule with a justification. The
// schema enforces `justification: string & !=""`, so the override
// itself carries the "why no package / no build" record. Same opt-out
// shape as `kolohelios-nix-via-flakehub`.
impl Rule for RustCliPackageFalseRequiresOverride {
    fn name(&self) -> &'static str {
        "rust-cli-package-false-requires-override"
    }
    fn applies(&self, meta: &ProjectMeta) -> bool {
        meta.kind == ProjectKind::RustCli && !rust_cli_package_enabled(meta)
    }
    fn check(&self, _project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        RuleResult::Fail(
            "rust-cli with cli.package: false must declare an audit.overrides entry for this rule (severity: \"off\") with a justification explaining why no flake package / ci.build is wired".into(),
        )
    }
}

fn rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(ReadmePresent),
        Box::new(GitignorePresent),
        Box::new(RustHasTests),
        Box::new(RustCoverageThresholdNonzero),
        Box::new(RustLicenseDual),
        Box::new(RustVersionPinned),
        Box::new(KoloheliosNixViaFlakehub),
        Box::new(KoloheliosHomeViaFlakehub),
        Box::new(ValidateRecipeMeaningful),
        Box::new(RustCliPackageTrueRequiresCiBuild),
        Box::new(RustCliPackageFalseRequiresOverride),
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
            kind: ProjectKind::RustCli,
            coverage: Some(Coverage {
                line: CoverageThreshold { fail: 30 },
            }),
            audit: None,
            cli: None,
            ci: None,
        }
    }

    fn infra_meta() -> ProjectMeta {
        ProjectMeta {
            name: "demo".into(),
            kind: ProjectKind::Infra,
            coverage: None,
            audit: None,
            cli: None,
            ci: None,
        }
    }

    fn rust_worker_meta() -> ProjectMeta {
        ProjectMeta {
            name: "demo".into(),
            kind: ProjectKind::RustWorker,
            coverage: None,
            audit: None,
            cli: None,
            ci: None,
        }
    }

    fn rust_cli_meta(package: bool, has_build: bool) -> ProjectMeta {
        ProjectMeta {
            cli: Some(CliConfig { package }),
            ci: if has_build {
                Some(CiConfig {
                    build: Some(serde_json::json!({})),
                })
            } else {
                None
            },
            ..rust_meta()
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
    fn rust_has_tests_applies_to_rust_and_rust_worker_only() {
        assert!(RustHasTests.applies(&rust_meta()));
        assert!(RustHasTests.applies(&rust_worker_meta()));
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
    fn coverage_threshold_applies_to_rust_and_rust_worker_only() {
        assert!(RustCoverageThresholdNonzero.applies(&rust_meta()));
        assert!(RustCoverageThresholdNonzero.applies(&rust_worker_meta()));
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
            }),
            ..rust_meta()
        };
        assert!(matches!(
            RustCoverageThresholdNonzero.check(tmp.path(), &meta),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn coverage_threshold_fails_when_block_missing_on_rust_cli() {
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
    fn coverage_threshold_passes_when_block_missing_on_rust_worker() {
        // Coverage is opt-in on rust-worker; absence must not fail.
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            RustCoverageThresholdNonzero.check(tmp.path(), &rust_worker_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn coverage_threshold_fails_for_zero_threshold_on_rust_worker() {
        // When a rust-worker app opts in, zero values are still rejected.
        let tmp = TempDir::new().unwrap();
        let meta = ProjectMeta {
            coverage: Some(Coverage {
                line: CoverageThreshold { fail: 0 },
            }),
            ..rust_worker_meta()
        };
        assert!(matches!(
            RustCoverageThresholdNonzero.check(tmp.path(), &meta),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn rust_license_dual_applies_to_rust_and_rust_worker_only() {
        assert!(RustLicenseDual.applies(&rust_meta()));
        assert!(RustLicenseDual.applies(&rust_worker_meta()));
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
    fn rust_license_dual_passes_for_taplo_aligned_license() {
        // `taplo fmt --check` with `align_entries = true` pads to the
        // widest key in `[package]`, so once `rust-version` is in play
        // every other key gets multi-space padding before `=`. The
        // substring check used to be `r#"license = "MIT OR Apache-2.0""#`
        // (one space) and broke as soon as the table was wider; this
        // guards against regressing to that form.
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nedition      = \"2021\"\nlicense      = \"MIT OR Apache-2.0\"\nname         = \"x\"\nrust-version = \"1.95\"\nversion      = \"0.1.0\"\n",
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
    fn kolohelios_home_via_flakehub_passes_when_no_flake_nix() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            KoloheliosHomeViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn kolohelios_home_via_flakehub_passes_when_flake_does_not_reference_input() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs\"; outputs = { ... }: { }; }\n",
        )
        .unwrap();
        assert_eq!(
            KoloheliosHomeViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn kolohelios_home_via_flakehub_passes_for_canonical_url() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{\n  inputs = {\n    home-env.url = \"https://flakehub.com/f/kolohelios/home/*.tar.gz\";\n    nixpkgs.follows = \"kolohelios-nix/nixpkgs\";\n  };\n}\n",
        )
        .unwrap();
        assert_eq!(
            KoloheliosHomeViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn kolohelios_home_via_flakehub_fails_for_path_input() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{\n  inputs = {\n    home-env.url = \"path:../home\";\n  };\n}\n",
        )
        .unwrap();
        match KoloheliosHomeViaFlakehub.check(tmp.path(), &infra_meta()) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("path:../home"), "got: {msg}");
                assert!(msg.contains("FlakeHub URL"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn kolohelios_home_via_flakehub_fails_for_github_url() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{\n  inputs.home-env.url = \"github:kolohelios/home\";\n}\n",
        )
        .unwrap();
        assert!(matches!(
            KoloheliosHomeViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn kolohelios_home_via_flakehub_ignores_comment_only_mentions() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{\n  # home-env.url could be a path: input from a sibling flake\n  inputs.nixpkgs.url = \"github:NixOS/nixpkgs\";\n}\n",
        )
        .unwrap();
        assert_eq!(
            KoloheliosHomeViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn validate_recipe_passes_when_validate_has_deps() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("justfile"),
            "test:\n    cargo test\n\nvalidate: test\n",
        )
        .unwrap();
        assert_eq!(
            ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn validate_recipe_passes_when_validate_has_real_body() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("justfile"), "validate:\n    cargo test\n").unwrap();
        assert_eq!(
            ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn validate_recipe_fails_when_justfile_missing() {
        let tmp = TempDir::new().unwrap();
        match ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("missing justfile"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn validate_recipe_fails_when_recipe_missing() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("justfile"), "test:\n    cargo test\n").unwrap();
        match ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("no `validate` recipe"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn validate_recipe_fails_for_at_true_only_body() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("justfile"), "validate:\n    @true\n").unwrap();
        match ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("placeholder"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn validate_recipe_fails_for_colon_only_body() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("justfile"), "validate:\n    :\n").unwrap();
        assert!(matches!(
            ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn validate_recipe_fails_for_empty_body_no_deps() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("justfile"), "validate:\n").unwrap();
        assert!(matches!(
            ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn validate_recipe_fails_when_only_comment_body() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("justfile"),
            "validate:\n    # nothing real to do\n",
        )
        .unwrap();
        assert!(matches!(
            ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn validate_recipe_ignores_recipe_named_validates() {
        // `validates:` must not be matched as a `validate` header.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("justfile"), "validates:\n    cargo test\n").unwrap();
        assert!(matches!(
            ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn validate_recipe_ignores_variable_assignment() {
        // `validate := ...` is a variable, not a recipe.
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("justfile"),
            "validate := \"never\"\n\ntest:\n    cargo test\n",
        )
        .unwrap();
        assert!(matches!(
            ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn validate_recipe_handles_validate_with_params() {
        // `validate ARGS:` is a recipe header with parameters.
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("justfile"),
            "validate args=\"\":\n    cargo test {{args}}\n",
        )
        .unwrap();
        assert_eq!(
            ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn validate_recipe_recognizes_real_recipe_after_blank_lines() {
        // Blank lines inside the recipe body don't terminate it.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("justfile"), "validate:\n\n    cargo test\n").unwrap();
        assert_eq!(
            ValidateRecipeMeaningful.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_version_pinned_applies_to_rust_and_rust_worker_only() {
        assert!(RustVersionPinned.applies(&rust_meta()));
        assert!(RustVersionPinned.applies(&rust_worker_meta()));
        assert!(!RustVersionPinned.applies(&infra_meta()));
    }

    #[test]
    fn rust_version_pinned_passes_for_canonical_msrv() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"MIT OR Apache-2.0\"\nrust-version = \"1.95\"\n",
        )
        .unwrap();
        assert_eq!(
            RustVersionPinned.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_version_pinned_passes_for_taplo_aligned_msrv() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nedition      = \"2021\"\nlicense      = \"MIT OR Apache-2.0\"\nname         = \"x\"\nrust-version = \"1.95\"\nversion      = \"0.1.0\"\n",
        )
        .unwrap();
        assert_eq!(
            RustVersionPinned.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_version_pinned_fails_when_cargo_toml_missing() {
        let tmp = TempDir::new().unwrap();
        match RustVersionPinned.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("missing Cargo.toml"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_version_pinned_fails_when_field_absent() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"MIT OR Apache-2.0\"\n",
        )
        .unwrap();
        match RustVersionPinned.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("must declare"), "got: {msg}");
                assert!(msg.contains("rust-version"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_version_pinned_fails_for_wrong_version() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nrust-version = \"1.80\"\n",
        )
        .unwrap();
        match RustVersionPinned.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("1.95"), "got: {msg}");
                assert!(msg.contains("1.80"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_version_pinned_ignores_rust_version_in_dependencies() {
        // A dep called `rust-version` (unlikely but defensible) in
        // `[dependencies]` must not be picked up as the `[package]` field.
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n[dependencies]\nrust-version = \"99.0\"\n",
        )
        .unwrap();
        match RustVersionPinned.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("must declare"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn cargo_package_field_finds_aligned_value() {
        let toml = "[package]\nedition      = \"2021\"\nrust-version = \"1.95\"\n";
        assert_eq!(
            cargo_package_field(toml, "rust-version"),
            Some("1.95".into())
        );
        assert_eq!(cargo_package_field(toml, "edition"), Some("2021".into()));
    }

    #[test]
    fn cargo_package_field_rejects_prefix_match() {
        // `name` must not match `name-suffix`.
        let toml = "[package]\nname-suffix = \"actual\"\n";
        assert_eq!(cargo_package_field(toml, "name"), None);
    }

    #[test]
    fn cargo_package_field_only_reads_package_section() {
        let toml = "[package]\nname = \"good\"\n\n[lib]\nname = \"bad\"\n";
        assert_eq!(cargo_package_field(toml, "name"), Some("good".into()));
    }

    #[test]
    fn cargo_package_field_skips_comments() {
        let toml = "[package]\n# rust-version = \"99.0\"\nrust-version = \"1.95\"\n";
        assert_eq!(
            cargo_package_field(toml, "rust-version"),
            Some("1.95".into())
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

    #[test]
    fn rust_cli_package_true_requires_ci_build_applies_only_to_rust_cli_with_package() {
        assert!(RustCliPackageTrueRequiresCiBuild.applies(&rust_cli_meta(true, false)));
        // default-true when cli is absent on a rust-cli kind
        assert!(RustCliPackageTrueRequiresCiBuild.applies(&rust_meta()));
        assert!(!RustCliPackageTrueRequiresCiBuild.applies(&rust_cli_meta(false, false)));
        assert!(!RustCliPackageTrueRequiresCiBuild.applies(&rust_worker_meta()));
        assert!(!RustCliPackageTrueRequiresCiBuild.applies(&infra_meta()));
    }

    #[test]
    fn rust_cli_package_true_requires_ci_build_passes_when_build_set() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            RustCliPackageTrueRequiresCiBuild.check(tmp.path(), &rust_cli_meta(true, true)),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_cli_package_true_requires_ci_build_fails_when_build_missing() {
        let tmp = TempDir::new().unwrap();
        match RustCliPackageTrueRequiresCiBuild.check(tmp.path(), &rust_cli_meta(true, false)) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("ci.build"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_cli_package_true_requires_ci_build_fails_when_ci_block_absent() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(
            RustCliPackageTrueRequiresCiBuild.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn rust_cli_package_false_requires_override_applies_only_when_package_false() {
        assert!(RustCliPackageFalseRequiresOverride.applies(&rust_cli_meta(false, false)));
        assert!(!RustCliPackageFalseRequiresOverride.applies(&rust_cli_meta(true, false)));
        // default-true means absent cli does not trip this rule
        assert!(!RustCliPackageFalseRequiresOverride.applies(&rust_meta()));
        assert!(!RustCliPackageFalseRequiresOverride.applies(&rust_worker_meta()));
        assert!(!RustCliPackageFalseRequiresOverride.applies(&infra_meta()));
    }

    #[test]
    fn rust_cli_package_false_requires_override_always_fails_when_applies() {
        let tmp = TempDir::new().unwrap();
        match RustCliPackageFalseRequiresOverride.check(tmp.path(), &rust_cli_meta(false, false)) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("audit.overrides"), "got: {msg}");
                assert!(msg.contains("justification"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
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
