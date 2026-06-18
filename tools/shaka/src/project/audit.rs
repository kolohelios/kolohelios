use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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
    /// The rule's precondition isn't met for this project, so it has
    /// nothing to verify. Distinct from `Pass`: a rule that detects its
    /// input is absent must not claim to have verified it. N/A results are
    /// skipped by the runner and don't count toward a project's applied
    /// rules. This lets input-scoped rules (e.g. the FlakeHub-input rules)
    /// stay quiet on projects that legitimately don't declare the input,
    /// including external repos.
    NotApplicable,
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
    RustLib,
    WasmApp,
    Infra,
    NixLib,
    Document,
}

impl ProjectKind {
    /// Rust-flavored kinds share the cargo/clippy/license rules even
    /// when coverage requirements diverge.
    fn is_rust_flavored(self) -> bool {
        matches!(
            self,
            ProjectKind::RustCli
                | ProjectKind::RustWorker
                | ProjectKind::RustLib
                | ProjectKind::WasmApp
        )
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
struct RustCargoRequiredFields;
struct RustClippyLintsDeclared;
struct RustUnwrapDenied;
struct LicensePresent;
struct KoloheliosNixViaFlakehub;
struct KoloheliosHomeViaFlakehub;
struct ValidateRecipeMeaningful;
struct RustCliPackageTrueRequiresCiBuild;
struct RustCliPackageFalseRequiresOverride;

const REQUIRED_CARGO_FIELDS: [&str; 3] = ["name", "version", "edition"];
// In a virtual workspace the shared package fields live in
// `[workspace.package]` and members inherit them via `<field>.workspace =
// true`. `name` is never workspace-shared (each member declares its own,
// and cargo enforces that), so the workspace-root manifest only owns
// `version` and `edition`.
const WORKSPACE_REQUIRED_CARGO_FIELDS: [&str; 2] = ["version", "edition"];
const LICENSE_FILENAMES: [&str; 4] = ["LICENSE", "LICENSE.md", "LICENSE-MIT", "LICENSE-APACHE"];
const REQUIRED_RUST_LICENSE: &str = "MIT OR Apache-2.0";
// Appended to `rust-license-dual` failures so the wall a proprietary crate
// hits also signposts the way through it: the general `audit.overrides`
// escape hatch (the same mechanism `rust-cli-package-false-requires-override`
// relies on). A genuinely all-rights-reserved crate opts out here, then
// declares `license-file` in Cargo.toml instead of the dual SPDX.
const PROPRIETARY_OPT_OUT_HINT: &str = " — a proprietary/all-rights-reserved crate may disable \
this rule via an `audit.overrides` entry (severity: \"off\") with a justification, then declare \
`[package].license-file` in Cargo.toml instead of the dual SPDX";
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
                ProjectKind::RustLib => {
                    RuleResult::Fail("rust-lib project missing coverage block".into())
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

// Every rust crate declares the repo's dual `MIT OR Apache-2.0` license by
// default, matching the root LICENSE pair every project inherits. A
// genuinely proprietary crate (e.g. a private external consumer) can't honor
// that — it opts out through the general `audit.overrides` mechanism rather
// than a bespoke field, mirroring how `cli.package: false` opts out of
// `rust-cli-package-false-requires-override`. The failure message carries
// the signpost (`PROPRIETARY_OPT_OUT_HINT`) so the path is discoverable at
// the point of failure. See #880.
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
                "Cargo.toml `[package].license` must be `\"{REQUIRED_RUST_LICENSE}\"` (found `\"{v}\"`){PROPRIETARY_OPT_OUT_HINT}"
            )),
            None => RuleResult::Fail(format!(
                "Cargo.toml must declare `[package].license = \"{REQUIRED_RUST_LICENSE}\"`{PROPRIETARY_OPT_OUT_HINT}"
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
        let header = package_table_header(&contents);
        match cargo_table_field(&contents, header, "rust-version") {
            Some(v) if v == REQUIRED_RUST_VERSION => RuleResult::Pass,
            Some(v) => RuleResult::Fail(format!(
                "Cargo.toml `{header}.rust-version` must be `\"{REQUIRED_RUST_VERSION}\"` (found `\"{v}\"`)"
            )),
            None => RuleResult::Fail(format!(
                "Cargo.toml must declare `{header}.rust-version = \"{REQUIRED_RUST_VERSION}\"`"
            )),
        }
    }
}

// Every rust crate's `Cargo.toml` must declare the fields cargo itself
// requires to build a package: `name`, `version`, `edition`. A single-crate
// project declares them in `[package]`; a virtual workspace declares the
// shared `version`/`edition` in `[workspace.package]` (members inherit them,
// and supply their own `name`), so the workspace manifest is checked there.
impl Rule for RustCargoRequiredFields {
    fn name(&self) -> &'static str {
        "rust-cargo-required-fields"
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
        let (header, fields): (&str, &[&str]) = if is_virtual_workspace(&contents) {
            ("[workspace.package]", &WORKSPACE_REQUIRED_CARGO_FIELDS)
        } else {
            ("[package]", &REQUIRED_CARGO_FIELDS)
        };
        let missing: Vec<&str> = fields
            .iter()
            .copied()
            .filter(|field| {
                cargo_table_field(&contents, header, field).is_none_or(|v| v.is_empty())
            })
            .collect();
        if missing.is_empty() {
            RuleResult::Pass
        } else {
            RuleResult::Fail(format!(
                "Cargo.toml `{header}` missing or empty required field(s): {} ({})",
                missing.join(", "),
                cargo.display()
            ))
        }
    }
}

// Every rust crate must pin its clippy lint levels rather than relying on
// whatever clippy defaults a given toolchain ships. The repo convention
// (written by `shaka project new`) is a `[lints.clippy]` table in
// `Cargo.toml`; a root `clippy.toml`/`.clippy.toml` also counts. rustfmt is
// intentionally *not* checked here — the repo runs `cargo fmt` on defaults
// with no `rustfmt.toml`, so there is no artifact to assert. A virtual
// workspace declares the shared lints in `[workspace.lints.clippy]` (members
// inherit via `lints.workspace = true`), so the workspace manifest is checked
// there instead of `[lints.clippy]`.
impl Rule for RustClippyLintsDeclared {
    fn name(&self) -> &'static str {
        "rust-clippy-lints-declared"
    }
    fn applies(&self, meta: &ProjectMeta) -> bool {
        meta.kind.is_rust_flavored()
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        if project_dir.join("clippy.toml").is_file() || project_dir.join(".clippy.toml").is_file() {
            return RuleResult::Pass;
        }
        let cargo = project_dir.join("Cargo.toml");
        let contents = match std::fs::read_to_string(&cargo) {
            Ok(c) => c,
            Err(_) => return RuleResult::Fail("missing Cargo.toml at project root".into()),
        };
        let header = if is_virtual_workspace(&contents) {
            "[workspace.lints.clippy]"
        } else {
            "[lints.clippy]"
        };
        if cargo_has_table(&contents, header) {
            RuleResult::Pass
        } else {
            RuleResult::Fail(format!(
                "no clippy config: declare `{header}` in Cargo.toml or add a clippy.toml ({})",
                cargo.display()
            ))
        }
    }
}

// Every project must be covered by a license. The repo keeps a single dual
// `LICENSE-MIT`/`LICENSE-APACHE` pair at its root (matching the
// `MIT OR Apache-2.0` each crate declares in Cargo.toml), inherited by every
// project rather than duplicated per project — the lightweight solo-dev
// choice. A project passes if it has its own LICENSE file or inherits one
// from an ancestor up to the repo root.
// Non-test code must not call `.unwrap()`. Enforcement is a crate-root
// `#![cfg_attr(not(test), deny(clippy::unwrap_used))]` attribute rather than a
// `[lints.clippy]` table entry: the table applies the lint to *all* cargo
// targets, which would also fire on the `.unwrap()` calls that pepper unit
// tests and integration-test helpers (`allow-unwrap-in-tests` only exempts
// `#[test]`/`#[cfg(test)]` contexts, not plain helper fns living in a `tests/`
// crate). The `not(test)` cfg-gate scopes the deny to the normal build,
// leaving every flavor of test code untouched. The attribute must live in
// each crate root that exists (`src/main.rs` and/or `src/lib.rs`): a bin and
// a lib compile as separate crates and don't share crate-level attributes.
//
// A virtual workspace has no crate root of its own, and the attribute can't be
// hoisted to `[workspace.lints]` (that is the all-targets table this rule
// avoids). So the deny must live in *every member's* crate root: members are
// enumerated via `cargo metadata` and each is checked exactly like a single
// crate.
impl Rule for RustUnwrapDenied {
    fn name(&self) -> &'static str {
        "rust-unwrap-denied"
    }
    fn applies(&self, meta: &ProjectMeta) -> bool {
        meta.kind.is_rust_flavored()
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        // A missing/unreadable manifest is not a workspace — fall through to the
        // single-crate path, which reports "no crate root" as before.
        let manifest = std::fs::read_to_string(project_dir.join("Cargo.toml")).unwrap_or_default();
        if !is_virtual_workspace(&manifest) {
            return check_crate_roots(project_dir);
        }
        let members = workspace_member_dirs(project_dir);
        if members.is_empty() {
            return RuleResult::Fail(
                "could not enumerate workspace members to check for the unwrap-deny attribute \
                 (cargo metadata)"
                    .into(),
            );
        }
        for member in &members {
            match check_crate_roots(member) {
                RuleResult::Pass => {}
                fail => return fail,
            }
        }
        RuleResult::Pass
    }
}

impl Rule for LicensePresent {
    fn name(&self) -> &'static str {
        "license-present"
    }
    fn applies(&self, _meta: &ProjectMeta) -> bool {
        true
    }
    fn check(&self, project_dir: &Path, _meta: &ProjectMeta) -> RuleResult {
        if license_in_self_or_ancestors(project_dir) {
            RuleResult::Pass
        } else {
            RuleResult::Fail(format!(
                "no license: add a LICENSE file or inherit one from the repo root ({})",
                project_dir.display()
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
            return RuleResult::NotApplicable;
        };
        match extract_input_url(&contents, "kolohelios-nix") {
            None => RuleResult::NotApplicable,
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
            return RuleResult::NotApplicable;
        };
        match extract_input_url(&contents, "home-env") {
            None => RuleResult::NotApplicable,
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

// Looks up a string-valued field in the `[package]` table of a Cargo.toml.
// Thin wrapper over `cargo_table_field` for the common single-crate case.
fn cargo_package_field(contents: &str, key: &str) -> Option<String> {
    cargo_table_field(contents, "[package]", key)
}

// True when the project-root manifest is a *virtual* workspace: it declares
// `[workspace]` but no `[package]` of its own. Such a manifest carries the
// shared package metadata in `[workspace.package]` / `[workspace.lints]`
// rather than `[package]`, so the rust audit rules resolve there instead
// (#908). A root crate that is *also* a workspace root (`[package]` plus
// `[workspace]`) is not virtual — its own `[package]` is authoritative.
fn is_virtual_workspace(contents: &str) -> bool {
    cargo_has_table(contents, "[workspace]") && !cargo_has_table(contents, "[package]")
}

// The table header that owns the package metadata for this manifest:
// `[workspace.package]` for a virtual workspace, `[package]` otherwise.
fn package_table_header(contents: &str) -> &'static str {
    if is_virtual_workspace(contents) {
        "[workspace.package]"
    } else {
        "[package]"
    }
}

// Looks up a string-valued field in the given table `header` (for example
// `[package]` or `[workspace.package]`) of a Cargo.toml, tolerating any
// whitespace between key, `=`, and value (taplo's `align_entries` pads to
// the longest key in the table, so substring matching against a fixed-width
// assignment is brittle). Returns the literal string value without quotes,
// or `None` if the key isn't present in that table. Other sections
// (`[dependencies]`, `[lib]`, sub-tables like `[package.metadata]`, etc.)
// are skipped so a `name`/`version` elsewhere isn't mistaken for the field.
fn cargo_table_field(contents: &str, header: &str, key: &str) -> Option<String> {
    let mut in_table = false;
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            // `header` includes its closing `]`, so this won't confuse
            // `[package]` with a sub-table like `[package.metadata]`.
            in_table = trimmed.starts_with(header);
            continue;
        }
        if !in_table || trimmed.starts_with('#') {
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

// True if `contents` declares the exact TOML table header `header` (for
// example `[lints.clippy]`) on its own line, tolerating leading indentation
// and a trailing comment. Commented-out headers and sub-tables
// (`[lints.clippy.foo]`) don't match.
// True if `contents` carries a crate-root inner attribute that denies (or
// forbids) `clippy::unwrap_used` outside test builds. Whitespace is stripped
// before matching so rustfmt spacing variants compare equal; comment lines are
// skipped so a commented-out attribute doesn't satisfy the rule.
fn has_unwrap_deny_attr(contents: &str) -> bool {
    contents.lines().any(|line| {
        if line.trim_start().starts_with("//") {
            return false;
        }
        let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        compact.contains("#![cfg_attr(not(test),deny(clippy::unwrap_used))]")
            || compact.contains("#![cfg_attr(not(test),forbid(clippy::unwrap_used))]")
    })
}

// Check the crate root(s) in `dir` (`src/main.rs` and/or `src/lib.rs`) for the
// non-test unwrap-deny attribute. A bin and a lib compile as separate crates,
// so the deny must appear in each root that exists. `Fail` when `dir` has no
// crate root at all, or any root lacks the attribute. Used for both a
// single-crate project and each member of a virtual workspace.
fn check_crate_roots(dir: &Path) -> RuleResult {
    let roots: Vec<&str> = ["src/main.rs", "src/lib.rs"]
        .into_iter()
        .filter(|r| dir.join(r).is_file())
        .collect();
    if roots.is_empty() {
        return RuleResult::Fail(format!(
            "no crate root (src/main.rs or src/lib.rs) to carry the unwrap-deny attribute ({})",
            dir.display()
        ));
    }
    for root in roots {
        let path = dir.join(root);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return RuleResult::Fail(format!("could not read {}", path.display()));
        };
        if !has_unwrap_deny_attr(&contents) {
            return RuleResult::Fail(format!(
                "{root} must deny `unwrap()` in non-test code: add \
                 `#![cfg_attr(not(test), deny(clippy::unwrap_used))]` at the crate root ({})",
                path.display()
            ));
        }
    }
    RuleResult::Pass
}

// Member crate directories of a virtual workspace at `project_dir`, via
// `cargo metadata --no-deps` (offline, members only). Returns empty on any
// failure — cargo absent, a metadata error, or a parse error — which the caller
// surfaces as "could not enumerate". Using cargo metadata (rather than parsing
// `members` by hand) resolves globs like `members = ["crates/*"]` correctly.
fn workspace_member_dirs(project_dir: &Path) -> Vec<PathBuf> {
    let Ok(output) = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"))
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(meta) = serde_json::from_slice::<CargoMetadata>(&output.stdout) else {
        return Vec::new();
    };
    meta.packages
        .iter()
        .filter_map(|p| Path::new(&p.manifest_path).parent().map(Path::to_path_buf))
        .collect()
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoMetadataPackage>,
}

#[derive(Deserialize)]
struct CargoMetadataPackage {
    manifest_path: String,
}

fn cargo_has_table(contents: &str, header: &str) -> bool {
    contents.lines().any(|line| {
        let Some(rest) = line.trim_start().strip_prefix(header) else {
            return false;
        };
        let rest = rest.trim_start();
        rest.is_empty() || rest.starts_with('#')
    })
}

// True if `dir` or any ancestor up to and including the repo root holds a
// recognized LICENSE file. The walk stops once it has checked the repo root
// (the first ancestor containing `.jj`/`.git`), so it never reaches a stray
// license outside the tree.
fn license_in_self_or_ancestors(dir: &Path) -> bool {
    for ancestor in dir.ancestors() {
        if LICENSE_FILENAMES.iter().any(|f| ancestor.join(f).is_file()) {
            return true;
        }
        if ancestor.join(".jj").exists() || ancestor.join(".git").exists() {
            break;
        }
    }
    false
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
        Box::new(RustCargoRequiredFields),
        Box::new(RustClippyLintsDeclared),
        Box::new(RustUnwrapDenied),
        Box::new(LicensePresent),
        Box::new(KoloheliosNixViaFlakehub),
        Box::new(KoloheliosHomeViaFlakehub),
        Box::new(ValidateRecipeMeaningful),
        Box::new(RustCliPackageTrueRequiresCiBuild),
        Box::new(RustCliPackageFalseRequiresOverride),
    ]
}

pub fn run() {
    if let Err(e) = schema_check::project_schema_path() {
        eprintln!("{RED}{BOLD}error:{RESET} could not resolve project schema: {e}");
        std::process::exit(1);
    }

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
        audit_project(project, &rules, &severities, &mut failures);
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

    let meta = match load_meta(&cue_path) {
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
            // The rule's precondition wasn't met (e.g. the input it scopes
            // to isn't declared); undo the applied++ so it's reported as
            // N/A, not as a verified pass.
            RuleResult::NotApplicable => applied -= 1,
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

fn load_meta(project_cue: &Path) -> Result<ProjectMeta, String> {
    let output = schema_check::cue_project(&["export", "--out", "json"], project_cue)
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
    fn kolohelios_nix_via_flakehub_na_when_no_flake_nix() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            KoloheliosNixViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::NotApplicable
        );
    }

    #[test]
    fn kolohelios_nix_via_flakehub_na_when_flake_does_not_reference_input() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs\"; outputs = { ... }: { }; }\n",
        )
        .unwrap();
        assert_eq!(
            KoloheliosNixViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::NotApplicable
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
            RuleResult::NotApplicable
        );
    }

    #[test]
    fn kolohelios_home_via_flakehub_na_when_no_flake_nix() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            KoloheliosHomeViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::NotApplicable
        );
    }

    #[test]
    fn kolohelios_home_via_flakehub_na_when_flake_does_not_reference_input() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("flake.nix"),
            "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs\"; outputs = { ... }: { }; }\n",
        )
        .unwrap();
        assert_eq!(
            KoloheliosHomeViaFlakehub.check(tmp.path(), &infra_meta()),
            RuleResult::NotApplicable
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
            RuleResult::NotApplicable
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
    fn rust_cargo_required_fields_passes_when_all_present() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        assert_eq!(
            RustCargoRequiredFields.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_cargo_required_fields_fails_when_cargo_toml_missing() {
        let tmp = TempDir::new().unwrap();
        match RustCargoRequiredFields.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("missing Cargo.toml"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_cargo_required_fields_names_each_missing_field() {
        let tmp = TempDir::new().unwrap();
        // Only `name` present — `version` and `edition` absent.
        fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        match RustCargoRequiredFields.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("version"), "got: {msg}");
                assert!(msg.contains("edition"), "got: {msg}");
                assert!(!msg.contains("name"), "name was present, got: {msg}");
                assert!(msg.contains("Cargo.toml"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_cargo_required_fields_treats_empty_value_as_missing() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"\"\nedition = \"2021\"\n",
        )
        .unwrap();
        match RustCargoRequiredFields.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("version"), "got: {msg}");
                assert!(!msg.contains("edition"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_cargo_required_fields_does_not_apply_to_non_rust() {
        assert!(!RustCargoRequiredFields.applies(&infra_meta()));
    }

    #[test]
    fn rust_clippy_lints_declared_passes_with_lints_clippy_table() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\n\n[lints.clippy]\nmod_module_files = \"deny\"\n",
        )
        .unwrap();
        assert_eq!(
            RustClippyLintsDeclared.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_clippy_lints_declared_passes_with_clippy_toml_and_no_table() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::write(
            tmp.path().join("clippy.toml"),
            "too-many-arguments-threshold = 10\n",
        )
        .unwrap();
        assert_eq!(
            RustClippyLintsDeclared.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_clippy_lints_declared_fails_when_neither_present() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        match RustClippyLintsDeclared.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("[lints.clippy]"), "got: {msg}");
                assert!(msg.contains("clippy.toml"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_clippy_lints_declared_ignores_commented_and_sub_tables() {
        let tmp = TempDir::new().unwrap();
        // A commented header and a sub-table must not satisfy the rule.
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\n# [lints.clippy]\n[lints.clippy.foo]\nbar = 1\n",
        )
        .unwrap();
        assert!(matches!(
            RustClippyLintsDeclared.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn rust_clippy_lints_declared_fails_when_cargo_toml_missing() {
        let tmp = TempDir::new().unwrap();
        match RustClippyLintsDeclared.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("missing Cargo.toml"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_clippy_lints_declared_does_not_apply_to_non_rust() {
        assert!(!RustClippyLintsDeclared.applies(&infra_meta()));
    }

    // ---- Cargo workspace support (#908) ----

    // A buzzingo-shaped virtual workspace manifest: `[workspace]` only,
    // shared package metadata in `[workspace.package]` / `[workspace.lints]`,
    // and no root `[package]`.
    const VIRTUAL_WORKSPACE_CARGO: &str = "\
[workspace]
members = [\"crates/*\"]
resolver = \"2\"

[workspace.package]
version = \"0.1.0\"
edition = \"2021\"
rust-version = \"1.95\"
license = \"MIT OR Apache-2.0\"

[workspace.lints.clippy]
unwrap_used = \"deny\"
";

    #[test]
    fn is_virtual_workspace_detects_workspace_only_manifest() {
        assert!(is_virtual_workspace(VIRTUAL_WORKSPACE_CARGO));
        // A plain crate is not a workspace.
        assert!(!is_virtual_workspace("[package]\nname = \"x\"\n"));
        // A root crate that is *also* a workspace root owns its [package],
        // so it is not virtual.
        assert!(!is_virtual_workspace(
            "[workspace]\nmembers = []\n\n[package]\nname = \"x\"\n"
        ));
    }

    #[test]
    fn rust_version_pinned_passes_for_virtual_workspace() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), VIRTUAL_WORKSPACE_CARGO).unwrap();
        assert_eq!(
            RustVersionPinned.check(tmp.path(), &rust_worker_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_version_pinned_fails_for_workspace_without_rust_version() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        match RustVersionPinned.check(tmp.path(), &rust_worker_meta()) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("must declare"), "got: {msg}");
                assert!(msg.contains("[workspace.package]"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_cargo_required_fields_passes_for_virtual_workspace() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), VIRTUAL_WORKSPACE_CARGO).unwrap();
        assert_eq!(
            RustCargoRequiredFields.check(tmp.path(), &rust_worker_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_cargo_required_fields_fails_for_workspace_missing_edition() {
        let tmp = TempDir::new().unwrap();
        // `name` is intentionally absent — it's per-member, not a
        // workspace-root field — and must not be reported missing.
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        match RustCargoRequiredFields.check(tmp.path(), &rust_worker_meta()) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("edition"), "got: {msg}");
                assert!(!msg.contains("name"), "name is per-member, got: {msg}");
                assert!(msg.contains("[workspace.package]"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_clippy_lints_declared_passes_for_virtual_workspace() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), VIRTUAL_WORKSPACE_CARGO).unwrap();
        assert_eq!(
            RustClippyLintsDeclared.check(tmp.path(), &rust_worker_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_clippy_lints_declared_fails_for_workspace_without_lints() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        match RustClippyLintsDeclared.check(tmp.path(), &rust_worker_meta()) {
            RuleResult::Fail(msg) => {
                assert!(msg.contains("[workspace.lints.clippy]"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    const UNWRAP_DENY_ATTR: &str = "#![cfg_attr(not(test), deny(clippy::unwrap_used))]";

    fn write_root(dir: &Path, name: &str, body: &str) {
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join(name), body).unwrap();
    }

    #[test]
    fn rust_unwrap_denied_passes_with_attr_in_main() {
        let tmp = TempDir::new().unwrap();
        write_root(
            tmp.path(),
            "main.rs",
            &format!("{UNWRAP_DENY_ATTR}\n\nfn main() {{}}\n"),
        );
        assert_eq!(
            RustUnwrapDenied.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_unwrap_denied_passes_with_attr_in_lib_only_crate() {
        let tmp = TempDir::new().unwrap();
        write_root(tmp.path(), "lib.rs", &format!("{UNWRAP_DENY_ATTR}\n"));
        assert_eq!(
            RustUnwrapDenied.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_unwrap_denied_requires_attr_in_every_root() {
        // A bin and a lib compile as separate crates; the deny must be in both.
        let tmp = TempDir::new().unwrap();
        write_root(tmp.path(), "lib.rs", &format!("{UNWRAP_DENY_ATTR}\n"));
        write_root(tmp.path(), "main.rs", "fn main() {}\n");
        match RustUnwrapDenied.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("main.rs"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_unwrap_denied_accepts_forbid_and_extra_whitespace() {
        let tmp = TempDir::new().unwrap();
        write_root(
            tmp.path(),
            "lib.rs",
            "#![cfg_attr( not(test) ,  forbid( clippy::unwrap_used ) )]\n",
        );
        assert_eq!(
            RustUnwrapDenied.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_unwrap_denied_ignores_commented_attr() {
        let tmp = TempDir::new().unwrap();
        write_root(tmp.path(), "lib.rs", &format!("// {UNWRAP_DENY_ATTR}\n"));
        assert!(matches!(
            RustUnwrapDenied.check(tmp.path(), &rust_meta()),
            RuleResult::Fail(_)
        ));
    }

    #[test]
    fn rust_unwrap_denied_fails_when_no_crate_root() {
        let tmp = TempDir::new().unwrap();
        match RustUnwrapDenied.check(tmp.path(), &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("no crate root"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn rust_unwrap_denied_does_not_apply_to_non_rust() {
        assert!(!RustUnwrapDenied.applies(&infra_meta()));
    }

    /// Write a virtual-workspace member crate with the given `lib.rs` body.
    fn write_member(root: &Path, name: &str, lib_body: &str) {
        let src = root.join("crates").join(name).join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            root.join("crates").join(name).join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        fs::write(src.join("lib.rs"), lib_body).unwrap();
    }

    #[test]
    fn rust_unwrap_denied_passes_for_workspace_when_all_members_deny() {
        // `members = ["crates/*"]` exercises glob resolution via cargo metadata.
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        write_member(tmp.path(), "core", &format!("{UNWRAP_DENY_ATTR}\n"));
        write_member(tmp.path(), "server", &format!("{UNWRAP_DENY_ATTR}\n"));
        assert_eq!(
            RustUnwrapDenied.check(tmp.path(), &rust_worker_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn rust_unwrap_denied_fails_for_workspace_when_a_member_lacks_deny() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/core\", \"crates/server\"]\n",
        )
        .unwrap();
        write_member(tmp.path(), "core", &format!("{UNWRAP_DENY_ATTR}\n"));
        write_member(tmp.path(), "server", "// no deny here\nfn f() {}\n");
        match RustUnwrapDenied.check(tmp.path(), &rust_worker_meta()) {
            RuleResult::Fail(msg) => {
                assert!(
                    msg.contains("server"),
                    "should name the offending member: {msg}"
                );
                assert!(msg.contains("unwrap"), "got: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn license_present_passes_with_own_license() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("LICENSE"), "MIT OR Apache-2.0\n").unwrap();
        assert_eq!(
            LicensePresent.check(tmp.path(), &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn license_present_passes_when_inherited_from_ancestor() {
        // Root holds the dual license; the project dir inherits it.
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(root.path().join("LICENSE-MIT"), "MIT\n").unwrap();
        fs::write(root.path().join("LICENSE-APACHE"), "Apache-2.0\n").unwrap();
        let project = root.path().join("apps/demo");
        fs::create_dir_all(&project).unwrap();
        assert_eq!(
            LicensePresent.check(&project, &rust_meta()),
            RuleResult::Pass
        );
    }

    #[test]
    fn license_present_fails_when_no_license_to_repo_root() {
        // `.git` bounds the ancestor walk at the synthetic repo root, which
        // carries no license — so the rule fails deterministically.
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        let project = root.path().join("apps/demo");
        fs::create_dir_all(&project).unwrap();
        match LicensePresent.check(&project, &rust_meta()) {
            RuleResult::Fail(msg) => assert!(msg.contains("no license"), "got: {msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn license_present_applies_to_every_kind() {
        assert!(LicensePresent.applies(&rust_meta()));
        assert!(LicensePresent.applies(&infra_meta()));
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
