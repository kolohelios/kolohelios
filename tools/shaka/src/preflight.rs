use std::io::Write;
use std::process::{Command, Output};

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

enum CheckResult {
    Pass,
    Fail { detail: String, output: Option<Output> },
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
        name: "nix flake check",
        paths: &[
            "flake.nix",
            "flake.lock",
            "tools/shaka/**",
            "infra/devbox/nixos/**",
        ],
        run: nix_flake_check,
    },
    Check {
        name: "shaka validate",
        paths: &["tools/shaka/**", "*/*/project.cue"],
        run: shaka_validate,
    },
    Check {
        name: "shaka generate --check",
        paths: &[
            "tools/shaka/**",
            "*/*/project.cue",
            "*/*/justfile",
        ],
        run: shaka_generate_check,
    },
    Check {
        name: "tofu validate (infra/devbox/terraform)",
        paths: &["infra/devbox/terraform/**"],
        run: tofu_validate,
    },
    Check {
        name: "tofu plan (infra/devbox/terraform)",
        paths: &[
            "infra/devbox/terraform/**",
            "infra/devbox/nixos/**",
            "flake.nix",
            "flake.lock",
        ],
        run: tofu_plan,
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

    let (to_run, skipped): (Vec<&Check>, Vec<&Check>) = CHECKS.iter().partition(|c| match &changed {
        None => true,
        Some(paths) => c.paths.is_empty() || paths.iter().any(|p| matches_any(p, c.paths)),
    });

    let total_to_run = to_run.len();
    if let Some(_) = changed {
        if skipped.is_empty() {
            println!("{BOLD}preflight:{RESET} running {total_to_run} checks (no path filter applied)");
        } else {
            let names: Vec<&str> = skipped.iter().map(|c| c.name).collect();
            println!(
                "{BOLD}preflight:{RESET} running {total_to_run} of {} checks ({YELLOW}skipped:{RESET} {})",
                CHECKS.len(),
                names.join(", ")
            );
        }
    } else {
        println!("{BOLD}preflight:{RESET} running {total_to_run} checks");
    }

    let mut passed = 0usize;
    let mut failures: Vec<(&'static str, CheckResult)> = Vec::new();

    for (i, check) in to_run.iter().enumerate() {
        let label = format!("[{}/{}] {}", i + 1, total_to_run, check.name);
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
                failures.push((check.name, result));
                if !keep_going {
                    break;
                }
            }
        }
    }

    println!();
    if failures.is_empty() {
        println!(
            "{GREEN}{BOLD}preflight passed{RESET} ({passed}/{total_to_run}{})",
            if !skipped.is_empty() {
                format!(", {} skipped", skipped.len())
            } else {
                String::new()
            }
        );
        return;
    }

    for (name, result) in &failures {
        if let CheckResult::Fail { output: Some(out), .. } = result {
            println!("{BOLD}── {name} ──{RESET}");
            std::io::stdout().write_all(&out.stdout).ok();
            std::io::stderr().write_all(&out.stderr).ok();
            println!();
        }
    }

    eprintln!(
        "{RED}{BOLD}preflight failed{RESET} ({passed} passed, {} failed of {total_to_run})",
        failures.len()
    );
    std::process::exit(1);
}

fn changed_paths(since: &str) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(["diff", "--name-only", since])
        .output()
        .map_err(|e| format!("failed to spawn git: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("git diff failed: {stderr}"));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(String::from)
        .collect())
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

fn nix_flake_check() -> CheckResult {
    run_command(Command::new("nix").args(["flake", "check", "--all-systems"]))
}

fn shaka_validate() -> CheckResult {
    spawn_self(&["validate"])
}

fn shaka_generate_check() -> CheckResult {
    spawn_self(&["generate", "--check"])
}

fn spawn_self(args: &[&str]) -> CheckResult {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            return CheckResult::Fail {
                detail: format!("could not locate shaka binary: {e}"),
                output: None,
            };
        }
    };
    let mut cmd = Command::new(exe);
    cmd.args(args);
    run_command(&mut cmd)
}

fn tofu_validate() -> CheckResult {
    let init = Command::new("tofu")
        .args(["init", "-backend=false", "-input=false"])
        .current_dir("infra/devbox/terraform")
        .output();
    match init {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            return CheckResult::Fail {
                detail: "tofu init failed".into(),
                output: Some(out),
            };
        }
        Err(e) => {
            return CheckResult::Fail {
                detail: format!("failed to spawn tofu: {e}"),
                output: None,
            };
        }
    }

    run_command(
        Command::new("tofu")
            .arg("validate")
            .current_dir("infra/devbox/terraform"),
    )
}

fn tofu_plan() -> CheckResult {
    let required = ["TF_VAR_linode_token", "TF_VAR_root_pass", "TF_VAR_image_id"];
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|var| std::env::var(var).is_err())
        .collect();
    if !missing.is_empty() {
        return CheckResult::Fail {
            detail: format!("missing required env vars: {}", missing.join(", ")),
            output: None,
        };
    }

    let init = Command::new("tofu")
        .args(["init", "-input=false"])
        .current_dir("infra/devbox/terraform")
        .output();
    match init {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            return CheckResult::Fail {
                detail: "tofu init failed".into(),
                output: Some(out),
            };
        }
        Err(e) => {
            return CheckResult::Fail {
                detail: format!("failed to spawn tofu: {e}"),
                output: None,
            };
        }
    }

    run_command(
        Command::new("tofu")
            .arg("plan")
            .current_dir("infra/devbox/terraform"),
    )
}

fn run_command(cmd: &mut Command) -> CheckResult {
    match cmd.output() {
        Ok(out) if out.status.success() => CheckResult::Pass,
        Ok(out) => CheckResult::Fail {
            detail: format!("exit code {}", out.status.code().unwrap_or(-1)),
            output: Some(out),
        },
        Err(e) => CheckResult::Fail {
            detail: format!("failed to spawn: {e}"),
            output: None,
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
        assert!(!matches_pattern("tools/other/src/main.rs", "tools/shaka/**"));
        assert!(!matches_pattern("apps/foo/src/main.rs", "tools/shaka/**"));
    }

    #[test]
    fn single_star_matches_one_component() {
        assert!(matches_pattern("tools/shaka/project.cue", "*/*/project.cue"));
        assert!(matches_pattern("apps/foo/project.cue", "*/*/project.cue"));
        assert!(!matches_pattern("tools/shaka/src/project.cue", "*/*/project.cue"));
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
}
