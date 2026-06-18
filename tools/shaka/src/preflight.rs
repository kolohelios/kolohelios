use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde::Serialize;

use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

/// Set in `--json` mode: checks capture subprocess output into their
/// `detail` instead of streaming it to the terminal, so stdout stays a
/// clean JSON document and failure detail carries the tool's own output.
static CAPTURE_OUTPUT: AtomicBool = AtomicBool::new(false);

fn capture_output() -> bool {
    CAPTURE_OUTPUT.load(Ordering::Relaxed)
}

/// Combine a child process's stdout and stderr into one trimmed string
/// for a failed check's `detail`.
fn combined_output(out: &Output) -> String {
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        if !s.trim().is_empty() {
            s.push('\n');
        }
        s.push_str(&err);
    }
    s.trim().to_string()
}

enum CheckResult {
    Pass,
    Fail { detail: String },
}

struct Check {
    name: &'static str,
    /// Glob patterns (with `*` and `**`) for paths that should trigger this check.
    /// Empty list means "always run regardless of changed files".
    paths: &'static [&'static str],
    /// Repo-root-relative paths that must exist on disk for this check to
    /// apply. Empty means "universal" — the check runs in any repo. When
    /// non-empty and NONE of the listed paths exist, the check is skipped
    /// cleanly (reported as skipped, not run, not failed). This lets a
    /// kolohelios-specific check (e.g. `shaka domain check`, scoped to
    /// `infra/cloudflare-dns/domains`) sit in the shared `CHECKS` suite yet
    /// stay dormant in an external repo that has nothing for it to act on.
    required_paths: &'static [&'static str],
    run: fn() -> CheckResult,
}

/// Machine-readable preflight result, emitted under `--json`. Stable
/// enough for CI parsers and assistant fix-routing to depend on.
#[derive(Serialize)]
struct PreflightReport {
    checks: Vec<CheckReport>,
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    success: bool,
}

#[derive(Serialize)]
struct CheckReport {
    name: String,
    /// `"repo"` for cross-project checks, `"project"` for a per-project
    /// `just validate`.
    kind: &'static str,
    /// `"pass"`, `"fail"`, or `"skipped"` (path scope didn't intersect
    /// `--since`).
    status: &'static str,
    duration_ms: u128,
    /// Tool stderr / failure detail; present only for failed checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// Path scope that selects this check: glob patterns for repo
    /// checks, the project directory for project checks.
    paths: Vec<String>,
}

const CHECKS: &[Check] = &[
    Check {
        name: "shaka project schema-check",
        // `**/project.cue` catches stray placements at any depth so the
        // depth-2 invariant (enforced inside schema-check) fires on
        // additions like `apps/project.cue` or `apps/foo/sub/project.cue`
        // under `--since`. `*/*/project.cue` (canonical layout) is already
        // covered by `**/project.cue` but kept for readability.
        paths: &["tools/shaka/**", "*/*/project.cue", "**/project.cue"],
        required_paths: &[],
        run: shaka_project_schema_check,
    },
    Check {
        name: "shaka domain schema-check",
        paths: &[
            "tools/shaka/schema/domain/**",
            "tools/shaka/src/domain/**",
            "infra/cloudflare-dns/domains/**",
        ],
        required_paths: &["infra/cloudflare-dns/domains"],
        run: shaka_domain_schema_check,
    },
    Check {
        // Drift between CUE-declared expected state and live WHOIS / NS
        // delegation. Network-dependent (whois + dig), so it only runs
        // when the registry or the check itself changed — registry
        // edits should re-verify the live world, and check-code edits
        // should re-test against the live world.
        name: "shaka domain check",
        paths: &[
            "tools/shaka/src/domain/check.rs",
            "tools/shaka/schema/domain/**",
            "infra/cloudflare-dns/domains/**",
        ],
        required_paths: &["infra/cloudflare-dns/domains"],
        run: shaka_domain_check,
    },
    Check {
        name: "shaka token cloudflare schema-check",
        paths: &[
            "tools/shaka/schema/cloudflare-token.cue",
            "tools/shaka/src/token/**",
            "infra/cloudflare-tokens/tokens/**",
        ],
        required_paths: &["infra/cloudflare-tokens/tokens"],
        run: shaka_token_cloudflare_schema_check,
    },
    Check {
        name: "shaka project generate-justfiles --check",
        paths: &["tools/shaka/**", "*/*/project.cue", "*/*/justfile"],
        required_paths: &[],
        run: shaka_project_generate_justfiles_check,
    },
    Check {
        // Drift between a rust-cli project's `cli:` block and its
        // committed `flake.nix`. Triggers on changes to the generator,
        // any project.cue, or any flake.nix — same shape as the
        // generate-justfiles drift check.
        name: "shaka project generate-flakes --check",
        paths: &["tools/shaka/**", "*/*/project.cue", "*/*/flake.nix"],
        required_paths: &[],
        run: shaka_project_generate_flakes_check,
    },
    Check {
        // Any change to a project.cue may add/remove/edit a deploy block,
        // and any change to the generator or the output dir may drift the
        // committed TF away from what the generator would produce now.
        name: "shaka deploy generate-tf --check",
        paths: &[
            "tools/shaka/**",
            "*/*/project.cue",
            "infra/cloudflare-deploy/terraform/generated/**",
        ],
        required_paths: &["infra/cloudflare-deploy/terraform/generated"],
        run: shaka_deploy_generate_tf_check,
    },
    Check {
        // Any change to a project.cue may add/remove/edit a wrangler
        // block, and any change to the generator may drift a committed
        // wrangler.toml away from what the generator would produce now.
        // Only projects with a wrangler block are managed, so the
        // hand-maintained wrangler.toml files stay out of scope until
        // they adopt the block.
        name: "shaka deploy generate-wrangler --check",
        paths: &["tools/shaka/**", "*/*/project.cue", "*/*/wrangler.toml"],
        required_paths: &[],
        run: shaka_deploy_generate_wrangler_check,
    },
    Check {
        // Drift between a project's `cue/` source and its committed
        // `terraform/module.tf`. Triggers on changes to the generator,
        // the schema, any project's CUE source, or any project's
        // emitted module.tf — mirroring the deploy check above.
        name: "shaka terraform check",
        paths: &["tools/shaka/**", "*/*/cue/**", "*/*/terraform/module.tf"],
        required_paths: &[],
        run: shaka_terraform_check,
    },
    Check {
        // Drift between a project's `ci:` block and the committed
        // wrapper workflow under `.github/workflows/`. Triggers on
        // changes to the generator, any project.cue, or any workflow
        // file.
        name: "shaka ci generate-workflows --check",
        paths: &["tools/shaka/**", "*/*/project.cue", ".github/workflows/**"],
        required_paths: &[],
        run: shaka_ci_generate_workflows_check,
    },
    Check {
        // Catches workflow files that bypass generation entirely
        // (sibling concern to generate-workflows --check, which only
        // sees files the generator already accounts for).
        name: "shaka ci audit-workflows",
        paths: &["tools/shaka/**", "*/*/project.cue", ".github/workflows/**"],
        required_paths: &[],
        run: shaka_ci_audit_workflows,
    },
    Check {
        name: "shaka project audit",
        // Any project-internal change can flip an audit outcome — README
        // presence, .gitignore presence, source files for the rust-has-tests
        // scan. `*/*/**` covers all of them; the audit itself lives under
        // `tools/shaka/**`, which `*/*/**` already matches.
        paths: &["*/*/**"],
        required_paths: &[],
        run: shaka_project_audit,
    },
    Check {
        // Cross-project drift gate: tools/resume/profile.cue is the
        // canonical profile prose; `shaka profile generate` renders it
        // into resume.md's managed regions and the portfolio's
        // data/profile.json. A profile.cue edit triggers tools/resume's
        // validate but not the portfolio's (per-project scoping), so this
        // repo-level check regenerates both and fails on drift regardless
        // of which side a PR touches.
        name: "shaka profile generate --check",
        paths: &[
            "tools/shaka/**",
            "tools/resume/profile.cue",
            "tools/resume/schema/profile.cue",
            "tools/resume/resume.md",
            "apps/kolohelios-portfolio/data/profile.json",
        ],
        required_paths: &[],
        run: shaka_profile_generate_check,
    },
    Check {
        // Cross-project drift gate: kolohelios-portfolio serves the
        // résumé via copies of tools/resume's rendered artifacts under
        // its committed dist/. The portfolio's own build-check only runs
        // when the portfolio's files change (per-project scoping), so a
        // résumé re-render alone would otherwise leave the served copies
        // stale. Triggering on both sides closes that gap. Guarding only
        // the PDF/DOCX is sufficient even for the rendered /resume page:
        // tools/resume's own drift check ties resume.md to its rendered
        // PDF/DOCX, so the markdown can't change without these changing.
        name: "resume synced to portfolio dist",
        paths: &[
            "tools/shaka/**",
            "tools/resume/resume.pdf",
            "tools/resume/resume.docx",
            "apps/kolohelios-portfolio/dist/resume.pdf",
            "apps/kolohelios-portfolio/dist/resume.docx",
        ],
        required_paths: &[],
        run: resume_synced_to_portfolio,
    },
    Check {
        // Whole-repo scan: typos is fast enough that scoping by
        // changed paths isn't worth the complexity, and a fresh
        // entry in typos.toml that suppresses an old false positive
        // should still fire on every PR until cleaned up.
        name: "typos",
        paths: &[],
        required_paths: &[],
        run: typos_check,
    },
    Check {
        name: "actionlint",
        paths: &[".github/workflows/**"],
        required_paths: &[],
        run: actionlint_check,
    },
    Check {
        // Whole-repo scan like `typos`: vale is fast enough that
        // scoping by changed paths isn't worth the complexity, and a
        // change to `.vale.ini` or vendored styles needs to re-lint
        // every markdown file.
        name: "vale",
        paths: &[],
        required_paths: &[],
        run: vale_check,
    },
    Check {
        name: "taplo fmt --check",
        paths: &["**/*.toml", "taplo.toml"],
        required_paths: &[],
        run: taplo_check,
    },
];

pub fn run(keep_going: bool, since: Option<String>, json: bool) {
    // JSON mode emits a single document at the end, so suppress the
    // streaming per-check progress lines and have checks capture their
    // subprocess output into `detail` rather than printing to stdout.
    let stream = !json;
    CAPTURE_OUTPUT.store(json, Ordering::Relaxed);
    let changed = match since.as_deref() {
        Some(r) => match changed_paths(r) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} could not get changed paths: {e}");
                std::process::exit(1);
            }
        },
        None => None,
    };

    // First gate: a check whose `required_paths` are all absent from this
    // repo doesn't apply here at all (e.g. the kolohelios-specific domain /
    // token / deploy checks in an external repo). Skip it cleanly before
    // even considering the `--since` path scope.
    let repo_root = Path::new(".");
    let (repo_applicable, repo_inapplicable): (Vec<&Check>, Vec<&Check>) =
        CHECKS.iter().partition(|c| check_applies(c, repo_root));

    // Second gate: among the applicable checks, the `--since` path scope
    // selects which actually need to run for this change set.
    let (repo_to_run, mut repo_skipped): (Vec<&Check>, Vec<&Check>) =
        repo_applicable.into_iter().partition(|c| match &changed {
            None => true,
            Some(paths) => c.paths.is_empty() || paths.iter().any(|p| matches_any(p, c.paths)),
        });
    repo_skipped.extend(repo_inapplicable);

    let projects: Vec<PathBuf> = crate::project::schema_check::discover(Path::new("."));
    let (candidates, mut project_skipped): (Vec<PathBuf>, Vec<PathBuf>) =
        projects.into_iter().partition(|p| match &changed {
            None => true,
            Some(paths) => paths.iter().any(|cp| under_project(cp, p)),
        });
    // Among the path-selected candidates, `native-app` is a host-built
    // kind (iOS/Android): it has no generated flake or justfile, so there
    // is no `nix develop . --command just validate` to run on the Linux
    // preflight lane. Move it to `skipped` rather than failing on a build
    // this lane can't do — repo-level checks (whitespace, typos, vale,
    // taplo) still cover its files. Reading the kind only for in-scope
    // candidates keeps the common `--since` run from shelling `cue export`
    // for every project. (#941)
    let (project_to_run, host_built): (Vec<PathBuf>, Vec<PathBuf>) =
        candidates.into_iter().partition(|p| {
            crate::project::schema_check::project_kind(p).as_deref() != Some("native-app")
        });
    project_skipped.extend(host_built);

    let total = repo_to_run.len() + project_to_run.len();
    let total_skipped = repo_skipped.len() + project_skipped.len();

    let mut checks: Vec<CheckReport> = Vec::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut bail = false;

    // Path-scope skips: record as `skipped` entries so a JSON consumer
    // sees the full check inventory, not just what ran.
    for c in &repo_skipped {
        checks.push(CheckReport {
            name: c.name.to_string(),
            kind: "repo",
            status: "skipped",
            duration_ms: 0,
            detail: None,
            paths: c.paths.iter().map(|p| (*p).to_string()).collect(),
        });
    }
    for p in &project_skipped {
        checks.push(CheckReport {
            name: project_label(p),
            kind: "project",
            status: "skipped",
            duration_ms: 0,
            detail: None,
            paths: vec![project_label(p)],
        });
    }

    if stream {
        if changed.is_some() && total_skipped > 0 {
            println!(
                "{BOLD}preflight:{RESET} running {} repo + {} project checks ({YELLOW}skipped:{RESET} {} repo, {} projects)",
                repo_to_run.len(),
                project_to_run.len(),
                repo_skipped.len(),
                project_skipped.len()
            );
        } else {
            println!(
                "{BOLD}preflight:{RESET} running {} repo + {} project checks",
                repo_to_run.len(),
                project_to_run.len()
            );
        }
    }

    let mut idx = 0usize;
    for check in &repo_to_run {
        idx += 1;
        if stream {
            let label = format!("[repo {}/{}] {}", idx, repo_to_run.len(), check.name);
            print!("  {label} ... ");
            std::io::stdout().flush().ok();
        }

        let start = Instant::now();
        let result = (check.run)();
        let duration_ms = start.elapsed().as_millis();
        let paths: Vec<String> = check.paths.iter().map(|p| (*p).to_string()).collect();
        match result {
            CheckResult::Pass => {
                if stream {
                    println!("{GREEN}{BOLD}ok{RESET}");
                }
                passed += 1;
                checks.push(CheckReport {
                    name: check.name.to_string(),
                    kind: "repo",
                    status: "pass",
                    duration_ms,
                    detail: None,
                    paths,
                });
            }
            CheckResult::Fail { detail } => {
                if stream {
                    println!("{RED}{BOLD}FAIL{RESET}");
                    if !detail.is_empty() {
                        println!("    {DIM}{detail}{RESET}");
                    }
                }
                failed += 1;
                checks.push(CheckReport {
                    name: check.name.to_string(),
                    kind: "repo",
                    status: "fail",
                    duration_ms,
                    detail: Some(detail),
                    paths,
                });
                if !keep_going {
                    bail = true;
                    break;
                }
            }
        }
    }

    if !bail {
        for (i, project) in project_to_run.iter().enumerate() {
            let display = project_label(project);
            if stream {
                let label = format!(
                    "[proj {}/{}] {} (just validate)",
                    i + 1,
                    project_to_run.len(),
                    display
                );
                print!("  {label} ... ");
                std::io::stdout().flush().ok();
            }

            let start = Instant::now();
            let result = just_validate(project);
            let duration_ms = start.elapsed().as_millis();
            match result {
                CheckResult::Pass => {
                    if stream {
                        println!("{GREEN}{BOLD}ok{RESET}");
                    }
                    passed += 1;
                    checks.push(CheckReport {
                        name: display,
                        kind: "project",
                        status: "pass",
                        duration_ms,
                        detail: None,
                        paths: vec![project_label(project)],
                    });
                }
                CheckResult::Fail { detail } => {
                    if stream {
                        println!("{RED}{BOLD}FAIL{RESET}");
                        if !detail.is_empty() {
                            println!("    {DIM}{detail}{RESET}");
                        }
                    }
                    failed += 1;
                    checks.push(CheckReport {
                        name: display,
                        kind: "project",
                        status: "fail",
                        duration_ms,
                        detail: Some(detail),
                        paths: vec![project_label(project)],
                    });
                    if !keep_going {
                        break;
                    }
                }
            }
        }
    }

    let report = PreflightReport {
        checks,
        total,
        passed,
        failed,
        skipped: total_skipped,
        success: failed == 0,
    };

    crate::output::emit(json, &report, render_human_summary);

    if !report.success {
        std::process::exit(1);
    }
}

/// Trailing summary line for human mode; the per-check lines were already
/// streamed as the checks ran.
fn render_human_summary(report: &PreflightReport) {
    println!();
    if report.success {
        let skipped = if report.skipped > 0 {
            format!(", {} skipped", report.skipped)
        } else {
            String::new()
        };
        println!(
            "{GREEN}{BOLD}preflight passed{RESET} ({}/{}{skipped})",
            report.passed, report.total
        );
    } else {
        eprintln!(
            "{RED}{BOLD}preflight failed{RESET} ({} passed, {} failed of {})",
            report.passed, report.failed, report.total
        );
    }
}

/// Render a project path like "tools/shaka" — drops a leading `./` when present.
fn project_label(p: &Path) -> String {
    p.strip_prefix(".")
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

/// Return true when `changed_path` lies under `project_dir/`.
fn under_project(changed_path: &str, project_dir: &Path) -> bool {
    let prefix = project_label(project_dir);
    let needle = format!("{prefix}/");
    changed_path.starts_with(&needle)
}

fn just_validate(project_dir: &Path) -> CheckResult {
    // Enter the project's own dev shell so `just validate` runs with the
    // project's tools on PATH (e.g. tofu for infra/devbox, cargo +
    // cargo-llvm-cov for tools/shaka). Without this, the recipe inherits
    // whatever shell preflight ran in, which won't have project-specific
    // tools.
    run_command(
        Command::new("nix")
            .args(["develop", ".", "--command", "just", "validate"])
            .current_dir(project_dir),
    )
}

fn changed_paths(since: &str) -> Result<Vec<String>, String> {
    // Resolve via jj rather than git so this works inside a sibling
    // workspace whose `.git` isn't reachable on the filesystem. See
    // issue #210.
    crate::jj::changed_paths(since, "@").map_err(|e| e.to_string())
}

/// Whether a check applies to the repo rooted at `root`. A check with no
/// `required_paths` is universal and always applies; otherwise it applies
/// only if at least one of its required paths exists on disk. This is what
/// lets kolohelios-specific repo checks lie dormant in an external repo
/// instead of failing against nothing.
fn check_applies(check: &Check, root: &Path) -> bool {
    required_paths_present(check.required_paths, root)
}

/// True when `required` is empty (universal) or at least one of its entries
/// exists under `root`.
fn required_paths_present(required: &[&str], root: &Path) -> bool {
    required.is_empty() || required.iter().any(|p| root.join(p).exists())
}

fn matches_any(path: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| matches_pattern(path, p))
}

fn matches_pattern(path: &str, pattern: &str) -> bool {
    let path_parts: Vec<&str> = path.split('/').collect();
    let pat_parts: Vec<&str> = pattern.split('/').collect();
    matches_parts(&path_parts, &pat_parts)
}

fn matches_parts(path: &[&str], pat: &[&str]) -> bool {
    if pat.is_empty() {
        return path.is_empty();
    }
    let head = pat[0];
    if head == "**" {
        if pat.len() == 1 {
            return true;
        }
        for i in 0..=path.len() {
            if matches_parts(&path[i..], &pat[1..]) {
                return true;
            }
        }
        return false;
    }
    if path.is_empty() {
        return false;
    }
    let part_matches = head == "*" || head == path[0];
    if !part_matches {
        return false;
    }
    matches_parts(&path[1..], &pat[1..])
}

fn shaka_project_schema_check() -> CheckResult {
    spawn_self(&["project", "schema-check"])
}

fn shaka_domain_schema_check() -> CheckResult {
    spawn_self(&["domain", "schema-check"])
}

fn shaka_domain_check() -> CheckResult {
    spawn_self(&["domain", "check"])
}

fn shaka_token_cloudflare_schema_check() -> CheckResult {
    spawn_self(&["token", "cloudflare", "schema-check"])
}

fn shaka_project_generate_justfiles_check() -> CheckResult {
    spawn_self(&["project", "generate-justfiles", "--check"])
}

fn shaka_project_generate_flakes_check() -> CheckResult {
    spawn_self(&["project", "generate-flakes", "--check"])
}

fn shaka_deploy_generate_tf_check() -> CheckResult {
    spawn_self(&["deploy", "generate-tf", "--check"])
}

fn shaka_deploy_generate_wrangler_check() -> CheckResult {
    spawn_self(&["deploy", "generate-wrangler", "--check"])
}

fn shaka_terraform_check() -> CheckResult {
    spawn_self(&["terraform", "check"])
}

fn shaka_ci_generate_workflows_check() -> CheckResult {
    spawn_self(&["ci", "generate-workflows", "--check"])
}

fn shaka_ci_audit_workflows() -> CheckResult {
    spawn_self(&["ci", "audit-workflows"])
}

fn shaka_project_audit() -> CheckResult {
    spawn_self(&["project", "audit"])
}

fn shaka_profile_generate_check() -> CheckResult {
    spawn_self(&["profile", "generate", "--check"])
}

/// Source → committed-copy artifact pairs guarded by
/// `resume_synced_to_portfolio`. Paths are repo-root relative, matching
/// the `cwd` preflight runs from.
const RESUME_ARTIFACT_PAIRS: &[(&str, &str)] = &[
    (
        "tools/resume/resume.pdf",
        "apps/kolohelios-portfolio/dist/resume.pdf",
    ),
    (
        "tools/resume/resume.docx",
        "apps/kolohelios-portfolio/dist/resume.docx",
    ),
];

fn resume_synced_to_portfolio() -> CheckResult {
    match resume_artifact_drift(RESUME_ARTIFACT_PAIRS) {
        Ok(stale) if stale.is_empty() => CheckResult::Pass,
        Ok(stale) => {
            let mut detail =
                String::from("resume artifacts differ from the committed portfolio copies:\n");
            for dst in &stale {
                detail.push_str(&format!("  {dst}\n"));
            }
            detail.push_str(
                "rebuild and commit the portfolio: \
                 (in apps/kolohelios-portfolio) \
                 cargo run --features build-site --bin build-site",
            );
            CheckResult::Fail { detail }
        }
        Err(detail) => CheckResult::Fail { detail },
    }
}

/// Byte-compare each (source, committed-copy) pair; returns the
/// committed-copy paths whose bytes differ from their source. An
/// unreadable file on either side is a hard error — a missing copy
/// means the portfolio was never rebuilt after the artifact landed.
fn resume_artifact_drift(pairs: &[(&str, &str)]) -> Result<Vec<String>, String> {
    let mut stale = Vec::new();
    for (src, dst) in pairs {
        let want = std::fs::read(src).map_err(|e| format!("read {src}: {e}"))?;
        let got = std::fs::read(dst).map_err(|e| format!("read {dst}: {e}"))?;
        if want != got {
            stale.push((*dst).to_string());
        }
    }
    Ok(stale)
}

fn typos_check() -> CheckResult {
    run_command(&mut Command::new("typos"))
}

fn vale_check() -> CheckResult {
    // Walk only tracked files via `jj file list`, not the filesystem,
    // so vale doesn't lint markdown buried in `target/`, `.terraform/`,
    // or other gitignored build artifacts. Vale itself doesn't respect
    // `.gitignore`. Same workspace-correctness concerns as
    // `actionlint_check`. See issue #210.
    let tracked = match crate::jj::file_list("@", &[".".to_string()]) {
        Ok(paths) => paths,
        Err(e) => {
            return CheckResult::Fail {
                detail: format!("could not list tracked files: {e}"),
            };
        }
    };
    let md_files: Vec<String> = tracked.into_iter().filter(|p| p.ends_with(".md")).collect();
    if md_files.is_empty() {
        return CheckResult::Pass;
    }

    // JSON mode: the suggestion-level display pass exists only for live
    // human output, so skip it and run a single warning-level pass with
    // captured output feeding the report's `detail` on failure.
    if capture_output() {
        return match Command::new("vale")
            .args(["--minAlertLevel=warning"])
            .args(&md_files)
            .output()
        {
            Ok(out) if out.status.success() => CheckResult::Pass,
            Ok(out) => CheckResult::Fail {
                detail: combined_output(&out),
            },
            Err(e) => CheckResult::Fail {
                detail: format!("failed to spawn vale: {e}"),
            },
        };
    }

    // Two passes so suggestions print but don't gate. Vale's
    // `--minAlertLevel` controls both display and exit code, so to honor
    // "show suggestions, fail on warnings" we display once at suggestion
    // level (with `--no-exit` so it always returns 0) and then re-run
    // silently at warning level for the gate.
    let display = Command::new("vale")
        .args(["--no-exit", "--minAlertLevel=suggestion"])
        .args(&md_files)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();
    if let Err(e) = display {
        return CheckResult::Fail {
            detail: format!("failed to spawn vale (display pass): {e}"),
        };
    }
    let gate = Command::new("vale")
        .args(["--minAlertLevel=warning"])
        .args(&md_files)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match gate {
        Ok(s) if s.success() => CheckResult::Pass,
        Ok(s) => CheckResult::Fail {
            detail: format!("exit code {}", s.code().unwrap_or(-1)),
        },
        Err(e) => CheckResult::Fail {
            detail: format!("failed to spawn vale (gate pass): {e}"),
        },
    }
}

fn taplo_check() -> CheckResult {
    // Driver invokes `taplo fmt --check` from the repo root; taplo
    // auto-discovers `taplo.toml` and applies its include/exclude globs
    // to enumerate files itself. No need to list paths via `jj` here —
    // taplo's `exclude` already covers `target/` and test fixtures, the
    // two cases where filesystem walk would otherwise pick up files we
    // don't want formatted. `RUST_LOG=warn` mutes taplo's INFO-level
    // "found N files" stderr; failures still print at ERROR.
    let mut cmd = Command::new("taplo");
    cmd.args(["fmt", "--check"]);
    cmd.env("RUST_LOG", "warn");
    run_command(&mut cmd)
}

fn actionlint_check() -> CheckResult {
    // Pass workflow paths explicitly so actionlint doesn't walk up
    // looking for a `.git` directory — that walk fails inside a jj
    // sibling workspace whose colocated `.git` lives only in the
    // primary tree. Discovery via `jj file list` also gives us a
    // workspace-correct view of which workflows exist. See issue #210.
    let tracked = match crate::jj::file_list("@", &[".github/workflows".to_string()]) {
        Ok(paths) => paths,
        Err(e) => {
            return CheckResult::Fail {
                detail: format!("could not list workflows: {e}"),
            };
        }
    };
    let workflows: Vec<String> = tracked
        .into_iter()
        .filter(|p| p.ends_with(".yaml") || p.ends_with(".yml"))
        .collect();
    if workflows.is_empty() {
        return CheckResult::Pass;
    }
    let mut cmd = Command::new("actionlint");
    // Suppress two false positives on `actions/create-github-app-token@v3`.
    // The action deprecated `app-id` in favor of `client-id` at runtime
    // (the kolohelios-bot workflows migrated in #303), but actionlint's
    // bundled action metadata is stale — upstream `popular_actions.go`
    // still has no `client-id` for this action and marks `app-id`
    // required for v3, so it flags the runtime-valid input. No released
    // actionlint accepts `client-id` here, and its config file can't
    // override action-input schemas, so `-ignore` is the only lever.
    // Scoped to these exact messages so every other action-input error
    // (including other inputs on the same action) still gates. See #854.
    cmd.args([
        "-ignore",
        r#"missing input "app-id" which is required by action "actions/create-github-app-token"#,
        "-ignore",
        r#"input "client-id" is not defined in action "actions/create-github-app-token"#,
    ]);
    cmd.args(&workflows);
    run_command(&mut cmd)
}

fn spawn_self(args: &[&str]) -> CheckResult {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            return CheckResult::Fail {
                detail: format!("could not locate shaka binary: {e}"),
            };
        }
    };
    let mut cmd = Command::new(exe);
    cmd.args(args);
    run_command(&mut cmd)
}

fn run_command(cmd: &mut Command) -> CheckResult {
    if capture_output() {
        // JSON mode: capture combined output so the report's `detail`
        // carries the tool's own stderr/stdout, and the parent's stdout
        // stays a clean JSON document.
        return match cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
        {
            Ok(out) if out.status.success() => CheckResult::Pass,
            Ok(out) => CheckResult::Fail {
                detail: combined_output(&out),
            },
            Err(e) => CheckResult::Fail {
                detail: format!("failed to spawn: {e}"),
            },
        };
    }

    // Stream stdout/stderr to the parent so progress is visible as it
    // happens. With nix-heavy per-project commands (cargo build, nix
    // develop closures), capturing output keeps the user staring at a
    // silent terminal for minutes. The user sees failures inline now so
    // captured-output replay-on-failure isn't needed.
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());
    match cmd.status() {
        Ok(status) if status.success() => CheckResult::Pass,
        Ok(status) => CheckResult::Fail {
            detail: format!("exit code {}", status.code().unwrap_or(-1)),
        },
        Err(e) => CheckResult::Fail {
            detail: format!("failed to spawn: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_json_shape_is_stable() {
        let report = PreflightReport {
            checks: vec![
                CheckReport {
                    name: "typos".to_string(),
                    kind: "repo",
                    status: "pass",
                    duration_ms: 12,
                    detail: None,
                    paths: vec![],
                },
                CheckReport {
                    name: "tools/shaka".to_string(),
                    kind: "project",
                    status: "fail",
                    duration_ms: 3400,
                    detail: Some("clippy: unused import".to_string()),
                    paths: vec!["tools/shaka".to_string()],
                },
                CheckReport {
                    name: "infra/devbox".to_string(),
                    kind: "project",
                    status: "skipped",
                    duration_ms: 0,
                    detail: None,
                    paths: vec!["infra/devbox".to_string()],
                },
            ],
            total: 2,
            passed: 1,
            failed: 1,
            skipped: 1,
            success: false,
        };
        let v: serde_json::Value = serde_json::to_value(&report).unwrap();

        assert_eq!(v["success"], false);
        assert_eq!(v["total"], 2);
        assert_eq!(v["passed"], 1);
        assert_eq!(v["failed"], 1);
        assert_eq!(v["skipped"], 1);

        // Passing checks omit `detail`; failing checks carry the tool output.
        assert!(v["checks"][0].get("detail").is_none());
        assert_eq!(v["checks"][0]["status"], "pass");
        assert_eq!(v["checks"][1]["status"], "fail");
        assert_eq!(v["checks"][1]["detail"], "clippy: unused import");
        assert_eq!(v["checks"][2]["status"], "skipped");
        assert_eq!(v["checks"][1]["kind"], "project");
    }

    #[test]
    fn matches_exact_path() {
        assert!(matches_pattern("flake.nix", "flake.nix"));
        assert!(!matches_pattern("flake.lock", "flake.nix"));
    }

    #[test]
    fn double_star_at_end_matches_everything_under() {
        assert!(matches_pattern("tools/shaka/src/main.rs", "tools/shaka/**"));
        assert!(matches_pattern("tools/shaka/Cargo.toml", "tools/shaka/**"));
        assert!(matches_pattern("tools/shaka/a/b/c/d.rs", "tools/shaka/**"));
    }

    #[test]
    fn double_star_does_not_cross_unrelated_prefixes() {
        assert!(!matches_pattern(
            "tools/other/src/main.rs",
            "tools/shaka/**"
        ));
        assert!(!matches_pattern("apps/foo/src/main.rs", "tools/shaka/**"));
    }

    #[test]
    fn single_star_matches_one_component() {
        assert!(matches_pattern(
            "tools/shaka/project.cue",
            "*/*/project.cue"
        ));
        assert!(matches_pattern("apps/foo/project.cue", "*/*/project.cue"));
        assert!(!matches_pattern(
            "tools/shaka/src/project.cue",
            "*/*/project.cue"
        ));
        assert!(!matches_pattern("project.cue", "*/*/project.cue"));
    }

    #[test]
    fn single_star_must_match_exactly_one_segment() {
        assert!(!matches_pattern("tools/project.cue", "*/*/project.cue"));
    }

    #[test]
    fn matches_any_returns_false_when_no_pattern_matches() {
        assert!(!matches_any("README.md", &["tools/shaka/**", "infra/**"]));
    }

    #[test]
    fn matches_any_returns_true_when_one_pattern_matches() {
        assert!(matches_any(
            "tools/shaka/Cargo.toml",
            &["infra/**", "tools/shaka/**"]
        ));
    }

    #[test]
    fn empty_path_does_not_match_pattern_with_components() {
        assert!(!matches_pattern("", "tools/shaka/**"));
    }

    #[test]
    fn under_project_matches_files_inside() {
        let project = Path::new("./tools/shaka");
        assert!(under_project("tools/shaka/Cargo.toml", project));
        assert!(under_project("tools/shaka/src/main.rs", project));
    }

    #[test]
    fn under_project_rejects_unrelated_paths() {
        let project = Path::new("./tools/shaka");
        assert!(!under_project("tools/other/Cargo.toml", project));
        assert!(!under_project("flake.nix", project));
        assert!(!under_project("infra/devbox/project.cue", project));
    }

    #[test]
    fn under_project_rejects_sibling_with_shared_prefix() {
        let project = Path::new("./tools/shaka");
        assert!(!under_project("tools/shaka-other/Cargo.toml", project));
    }

    #[test]
    fn project_label_strips_leading_dot() {
        assert_eq!(project_label(Path::new("./tools/shaka")), "tools/shaka");
        assert_eq!(project_label(Path::new("tools/shaka")), "tools/shaka");
    }

    #[test]
    fn resume_artifact_drift_flags_mismatch_keeps_match_errors_on_missing() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let same = dir.path().join("same.bin");
        let diff = dir.path().join("diff.bin");
        std::fs::write(&src, b"resume-bytes").unwrap();
        std::fs::write(&same, b"resume-bytes").unwrap();
        std::fs::write(&diff, b"stale-bytes").unwrap();
        let s = src.to_str().unwrap();

        // Byte-identical copy → no drift.
        assert!(resume_artifact_drift(&[(s, same.to_str().unwrap())])
            .unwrap()
            .is_empty());

        // Differing copy → drift names the committed-copy path.
        let dst = diff.to_str().unwrap();
        assert_eq!(
            resume_artifact_drift(&[(s, dst)]).unwrap(),
            vec![dst.to_string()]
        );

        // Missing copy → hard error (portfolio never rebuilt).
        assert!(resume_artifact_drift(&[(s, "/no/such/copy.bin")]).is_err());
    }

    #[test]
    fn universal_check_always_applies() {
        // No required_paths => applies in any repo, even an empty temp dir.
        let dir = std::env::temp_dir();
        assert!(required_paths_present(&[], &dir));
    }

    #[test]
    fn scoped_check_skips_when_required_paths_absent() {
        // A check scoped to a kolohelios-only dir does not apply in a repo
        // that lacks every one of its required paths.
        let dir = std::env::temp_dir().join("shaka-preflight-test-absent");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!required_paths_present(
            &[
                "infra/cloudflare-dns/domains",
                "infra/cloudflare-tokens/tokens"
            ],
            &dir
        ));
    }

    #[test]
    fn scoped_check_runs_when_any_required_path_present() {
        // If at least one required path exists, the check applies. Build a
        // temp tree containing only the second required path.
        let dir = std::env::temp_dir().join("shaka-preflight-test-present");
        std::fs::create_dir_all(dir.join("infra/cloudflare-tokens/tokens")).unwrap();
        assert!(required_paths_present(
            &[
                "infra/cloudflare-dns/domains",
                "infra/cloudflare-tokens/tokens"
            ],
            &dir
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn check_applies_delegates_to_required_paths() {
        let dir = std::env::temp_dir().join("shaka-preflight-test-delegate");
        std::fs::create_dir_all(&dir).unwrap();
        let universal = Check {
            name: "universal",
            paths: &[],
            required_paths: &[],
            run: || CheckResult::Pass,
        };
        let scoped = Check {
            name: "scoped",
            paths: &[],
            required_paths: &["infra/cloudflare-dns/domains"],
            run: || CheckResult::Pass,
        };
        assert!(check_applies(&universal, &dir));
        assert!(!check_applies(&scoped, &dir));
        std::fs::remove_dir_all(&dir).ok();
    }
}
