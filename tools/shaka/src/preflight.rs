use std::io::Write;
use std::process::{Command, Output};

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

enum CheckResult {
    Pass,
    Fail { detail: String, output: Option<Output> },
}

struct Check {
    name: &'static str,
    run: fn() -> CheckResult,
}

const CHECKS: &[Check] = &[
    Check { name: "nix flake check", run: nix_flake_check },
    Check { name: "tofu validate (infra/devbox/terraform)", run: tofu_validate },
    Check { name: "tofu plan (infra/devbox/terraform)", run: tofu_plan },
];

pub fn run(keep_going: bool) {
    let total = CHECKS.len();
    let mut passed = 0usize;
    let mut failures: Vec<(&'static str, CheckResult)> = Vec::new();

    println!("{BOLD}preflight:{RESET} running {total} checks");

    for (i, check) in CHECKS.iter().enumerate() {
        let label = format!("[{}/{}] {}", i + 1, total, check.name);
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
        println!("{GREEN}{BOLD}preflight passed{RESET} ({passed}/{total})");
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
        "{RED}{BOLD}preflight failed{RESET} ({passed} passed, {} failed of {total})",
        failures.len()
    );
    std::process::exit(1);
}

fn nix_flake_check() -> CheckResult {
    run_command(Command::new("nix").args(["flake", "check", "--all-systems"]))
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
