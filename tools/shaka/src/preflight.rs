use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

enum CheckResult {
    Pass,
    Fail { detail: String },
}

struct Check {
    name: &'static str,
    /// Glob patterns (with `*` and `**`) for paths that should trigger this check.
    /// Empty list means "always run regardless of changed files".
    paths: &'static [&'static str],
    run: fn() -> CheckResult,
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
        run: shaka_project_schema_check,
    },
    Check {
        name: "shaka domain schema-check",
        paths: &[
            "tools/shaka/schema/domain/**",
            "tools/shaka/src/domain/**",
            "infra/cloudflare-dns/domains/**",
        ],
        run: shaka_domain_schema_check,
    },
    Check {
        name: "shaka token cloudflare schema-check",
        paths: &[
            "tools/shaka/schema/cloudflare-token.cue",
            "tools/shaka/src/token/**",
            "infra/cloudflare-tokens/tokens/**",
        ],
        run: shaka_token_cloudflare_schema_check,
    },
    Check {
        name: "shaka project generate-justfiles --check",
        paths: &["tools/shaka/**", "*/*/project.cue", "*/*/justfile"],
        run: shaka_project_generate_justfiles_check,
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
        run: shaka_deploy_generate_tf_check,
    },
    Check {
        // Drift between a project's `cue/` source and its committed
        // `terraform/module.tf`. Triggers on changes to the generator,
        // the schema, any project's CUE source, or any project's
        // emitted module.tf — mirroring the deploy check above.
        name: "shaka terraform check",
        paths: &["tools/shaka/**", "*/*/cue/**", "*/*/terraform/module.tf"],
        run: shaka_terraform_check,
    },
    Check {
        // Drift between a project's `ci:` block and the committed
        // wrapper workflow under `.github/workflows/`. Triggers on
        // changes to the generator, any project.cue, or any workflow
        // file.
        name: "shaka ci generate-workflows --check",
        paths: &["tools/shaka/**", "*/*/project.cue", ".github/workflows/**"],
        run: shaka_ci_generate_workflows_check,
    },
    Check {
        // Catches workflow files that bypass generation entirely
        // (sibling concern to generate-workflows --check, which only
        // sees files the generator already accounts for).
        name: "shaka ci audit-workflows",
        paths: &["tools/shaka/**", "*/*/project.cue", ".github/workflows/**"],
        run: shaka_ci_audit_workflows,
    },
    Check {
        name: "shaka project audit",
        // Any project-internal change can flip an audit outcome — README
        // presence, .gitignore presence, source files for the rust-has-tests
        // scan. `*/*/**` covers all of them; the audit itself lives under
        // `tools/shaka/**`, which `*/*/**` already matches.
        paths: &["*/*/**"],
        run: shaka_project_audit,
    },
    Check {
        // Whole-repo scan: typos is fast enough that scoping by
        // changed paths isn't worth the complexity, and a fresh
        // entry in typos.toml that suppresses an old false positive
        // should still fire on every PR until cleaned up.
        name: "typos",
        paths: &[],
        run: typos_check,
    },
    Check {
        name: "actionlint",
        paths: &[".github/workflows/**"],
        run: actionlint_check,
    },
    Check {
        // Whole-repo scan like `typos`: vale is fast enough that
        // scoping by changed paths isn't worth the complexity, and a
        // change to `.vale.ini` or vendored styles needs to re-lint
        // every markdown file.
        name: "vale",
        paths: &[],
        run: vale_check,
    },
];

pub fn run(keep_going: bool, since: Option<String>) {
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

    let (repo_to_run, repo_skipped): (Vec<&Check>, Vec<&Check>) =
        CHECKS.iter().partition(|c| match &changed {
            None => true,
            Some(paths) => c.paths.is_empty() || paths.iter().any(|p| matches_any(p, c.paths)),
        });

    let projects: Vec<PathBuf> = crate::project::schema_check::discover(Path::new("."));
    let (project_to_run, project_skipped): (Vec<PathBuf>, Vec<PathBuf>) =
        projects.into_iter().partition(|p| match &changed {
            None => true,
            Some(paths) => paths.iter().any(|cp| under_project(cp, p)),
        });

    let total = repo_to_run.len() + project_to_run.len();
    let total_skipped = repo_skipped.len() + project_skipped.len();

    let mut passed = 0usize;
    let mut failures: Vec<(String, CheckResult)> = Vec::new();
    let mut bail = false;

    if changed.is_some() {
        if total_skipped == 0 {
            println!(
                "{BOLD}preflight:{RESET} running {} repo + {} project checks",
                repo_to_run.len(),
                project_to_run.len()
            );
        } else {
            println!(
                "{BOLD}preflight:{RESET} running {} repo + {} project checks ({YELLOW}skipped:{RESET} {} repo, {} projects)",
                repo_to_run.len(),
                project_to_run.len(),
                repo_skipped.len(),
                project_skipped.len()
            );
        }
    } else {
        println!(
            "{BOLD}preflight:{RESET} running {} repo + {} project checks",
            repo_to_run.len(),
            project_to_run.len()
        );
    }

    let mut idx = 0usize;
    for check in &repo_to_run {
        idx += 1;
        let label = format!("[repo {}/{}] {}", idx, repo_to_run.len(), check.name);
        print!("  {label} ... ");
        std::io::stdout().flush().ok();

        let result = (check.run)();
        match &result {
            CheckResult::Pass => {
                println!("{GREEN}{BOLD}ok{RESET}");
                passed += 1;
            }
            CheckResult::Fail { detail, .. } => {
                println!("{RED}{BOLD}FAIL{RESET}");
                if !detail.is_empty() {
                    println!("    {DIM}{detail}{RESET}");
                }
                failures.push((check.name.to_string(), result));
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
            let label = format!(
                "[proj {}/{}] {} (just validate)",
                i + 1,
                project_to_run.len(),
                display
            );
            print!("  {label} ... ");
            std::io::stdout().flush().ok();

            let result = just_validate(project);
            match &result {
                CheckResult::Pass => {
                    println!("{GREEN}{BOLD}ok{RESET}");
                    passed += 1;
                }
                CheckResult::Fail { detail, .. } => {
                    println!("{RED}{BOLD}FAIL{RESET}");
                    if !detail.is_empty() {
                        println!("    {DIM}{detail}{RESET}");
                    }
                    failures.push((display, result));
                    if !keep_going {
                        break;
                    }
                }
            }
        }
    }

    println!();
    if failures.is_empty() {
        println!(
            "{GREEN}{BOLD}preflight passed{RESET} ({passed}/{total}{})",
            if total_skipped > 0 {
                format!(", {total_skipped} skipped")
            } else {
                String::new()
            }
        );
        return;
    }

    eprintln!(
        "{RED}{BOLD}preflight failed{RESET} ({passed} passed, {} failed of {total})",
        failures.len()
    );
    std::process::exit(1);
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

fn shaka_token_cloudflare_schema_check() -> CheckResult {
    spawn_self(&["token", "cloudflare", "schema-check"])
}

fn shaka_project_generate_justfiles_check() -> CheckResult {
    spawn_self(&["project", "generate-justfiles", "--check"])
}

fn shaka_deploy_generate_tf_check() -> CheckResult {
    spawn_self(&["deploy", "generate-tf", "--check"])
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
}
