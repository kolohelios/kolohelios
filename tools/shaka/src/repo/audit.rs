use serde_json::{json, Value};

use crate::gh;

// ANSI escape codes
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Pass,
    Fail,
    Warn,
    Error,
}

struct Check {
    name: &'static str,
    status: Status,
    detail: String,
}

impl Check {
    fn pass(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: Status::Pass,
            detail: detail.into(),
        }
    }

    fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: Status::Fail,
            detail: detail.into(),
        }
    }

    fn warn(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: Status::Warn,
            detail: detail.into(),
        }
    }

    fn error(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: Status::Error,
            detail: detail.into(),
        }
    }

    fn print(&self) {
        let (label, color) = match self.status {
            Status::Pass => ("PASS", GREEN),
            Status::Fail => ("FAIL", RED),
            Status::Warn => ("WARN", YELLOW),
            Status::Error => (" ERR", RED),
        };
        println!(
            "  {color}{BOLD}[{label}]{RESET} {}: {}",
            self.name, self.detail
        );
    }
}

pub fn run(repo_arg: Option<String>, fix: bool) {
    let repo = match repo_arg {
        Some(r) => r,
        None => match gh::detect_repo() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} {e}");
                eprintln!("Hint: pass --repo owner/repo explicitly");
                std::process::exit(1);
            }
        },
    };

    println!("{BOLD}Auditing {repo}{RESET}\n");

    let mut has_failure = false;

    // General settings
    let (checks, repo_data) = check_general(&repo);
    print_group("Repository Settings", &checks);
    has_failure |= checks.iter().any(|c| c.status == Status::Fail);

    if fix {
        fix_general(&repo, &checks, &repo_data);
    }

    // Branch protection
    let checks = check_branch_protection(&repo);
    print_group("Branch Protection (main)", &checks);
    has_failure |= checks.iter().any(|c| c.status == Status::Fail);

    if fix {
        fix_branch_protection(&repo, &checks);
    }

    // Rulesets
    let checks = check_rulesets(&repo);
    print_group("Rulesets", &checks);
    has_failure |= checks.iter().any(|c| c.status == Status::Fail);

    // Security
    let checks = check_security(&repo);
    print_group("Security", &checks);
    has_failure |= checks.iter().any(|c| c.status == Status::Fail);

    if fix {
        fix_security(&repo, &checks);
    }

    println!();
    if has_failure {
        if !fix {
            println!("{YELLOW}Run with --fix to apply recommended settings{RESET}");
        }
        std::process::exit(1);
    } else {
        println!("{GREEN}{BOLD}All checks passed{RESET}");
    }
}

fn print_group(title: &str, checks: &[Check]) {
    println!("{BOLD}{title}{RESET}");
    for check in checks {
        check.print();
    }
    println!();
}

// ── General settings ───────────────────────────────────────────

fn check_general(repo: &str) -> (Vec<Check>, Option<Value>) {
    let data = match gh::api_get(&format!("/repos/{repo}")) {
        Ok(v) => v,
        Err(e) => {
            return (
                vec![Check::error("Repository", format!("API error: {e}"))],
                None,
            );
        }
    };

    let checks = vec![
        bool_check(
            "Default branch",
            data["default_branch"].as_str() == Some("main"),
            "main",
            data["default_branch"].as_str().unwrap_or("unknown"),
        ),
        bool_check(
            "Issues enabled",
            data["has_issues"].as_bool() == Some(true),
            "enabled",
            "disabled",
        ),
        bool_check(
            "Rebase merge",
            data["allow_rebase_merge"].as_bool() == Some(true),
            "enabled",
            "disabled",
        ),
        bool_check(
            "Merge commits",
            data["allow_merge_commit"].as_bool() == Some(false),
            "disabled",
            "enabled (should be disabled)",
        ),
        bool_check(
            "Squash merge",
            data["allow_squash_merge"].as_bool() == Some(false),
            "disabled",
            "enabled (should be disabled)",
        ),
        bool_check(
            "Delete branch on merge",
            data["delete_branch_on_merge"].as_bool() == Some(true),
            "enabled",
            "disabled",
        ),
    ];

    (checks, Some(data))
}

fn fix_general(repo: &str, checks: &[Check], _repo_data: &Option<Value>) {
    let needs_fix = checks.iter().any(|c| {
        c.status == Status::Fail
            && matches!(
                c.name,
                "Rebase merge"
                    | "Merge commits"
                    | "Squash merge"
                    | "Delete branch on merge"
                    | "Issues enabled"
            )
    });

    if !needs_fix {
        return;
    }

    println!("  {YELLOW}Fixing repository settings...{RESET}");
    let body = json!({
        "has_issues": true,
        "allow_rebase_merge": true,
        "allow_merge_commit": false,
        "allow_squash_merge": false,
        "delete_branch_on_merge": true,
    });

    match gh::api_patch(&format!("/repos/{repo}"), &body) {
        Ok(_) => println!("  {GREEN}Fixed{RESET}"),
        Err(e) => eprintln!("  {RED}Fix failed: {e}{RESET}"),
    }
}

// ── Branch protection ──────────────────────────────────────────

fn check_branch_protection(repo: &str) -> Vec<Check> {
    let data = match gh::api_get(&format!("/repos/{repo}/branches/main/protection")) {
        Ok(v) => v,
        Err(e) => {
            let msg = e.message.to_string();
            if msg.contains("404") || msg.contains("not protected") || msg.contains("Not Found") {
                return vec![Check::fail("Branch protection", "not enabled on main")];
            }
            return vec![Check::error("Branch protection", format!("API error: {e}"))];
        }
    };

    vec![
        Check::pass("Branch protection", "enabled"),
        bool_check(
            "Required status checks",
            !data["required_status_checks"].is_null(),
            "configured",
            "not configured",
        ),
        bool_check(
            "Strict status checks",
            data["required_status_checks"]["strict"].as_bool() != Some(true),
            "off (stale-but-mergeable allowed)",
            "on (forces rebase before merge)",
        ),
        bool_check(
            "Force push",
            data["allow_force_pushes"]["enabled"].as_bool() == Some(false),
            "blocked",
            "allowed (should be blocked)",
        ),
        bool_check(
            "Branch deletion",
            data["allow_deletions"]["enabled"].as_bool() == Some(false),
            "blocked",
            "allowed (should be blocked)",
        ),
    ]
}

fn fix_branch_protection(repo: &str, checks: &[Check]) {
    let protection_missing = checks
        .iter()
        .any(|c| c.name == "Branch protection" && c.status == Status::Fail);

    let needs_fix = protection_missing
        || checks.iter().any(|c| {
            c.status == Status::Fail
                && matches!(
                    c.name,
                    "Required status checks"
                        | "Strict status checks"
                        | "Force push"
                        | "Branch deletion"
                )
        });

    if !needs_fix {
        return;
    }

    println!("  {YELLOW}Fixing branch protection...{RESET}");
    let body = json!({
        "required_status_checks": {
            "strict": false,
            "contexts": ["Gate"]
        },
        "enforce_admins": true,
        "required_pull_request_reviews": null,
        "restrictions": null,
        "allow_force_pushes": false,
        "allow_deletions": false,
    });

    match gh::api_put(&format!("/repos/{repo}/branches/main/protection"), &body) {
        Ok(_) => println!("  {GREEN}Fixed{RESET}"),
        Err(e) => eprintln!("  {RED}Fix failed: {e}{RESET}"),
    }
}

// ── Rulesets ───────────────────────────────────────────────────

fn check_rulesets(repo: &str) -> Vec<Check> {
    let rulesets = match gh::api_get(&format!("/repos/{repo}/rulesets")) {
        Ok(Value::Array(arr)) => arr,
        Ok(_) => return vec![Check::warn("Rulesets", "unexpected response format")],
        Err(e) => return vec![Check::error("Rulesets", format!("API error: {e}"))],
    };

    if rulesets.is_empty() {
        return vec![Check::warn(
            "Rulesets",
            "none configured (relying on branch protection only)",
        )];
    }

    let mut checks = Vec::new();
    let active: Vec<&Value> = rulesets
        .iter()
        .filter(|r| r["enforcement"] == "active")
        .collect();

    checks.push(bool_check(
        "Active rulesets",
        !active.is_empty(),
        &format!("{} found", active.len()),
        "none active",
    ));

    // Check each active ruleset for key rules
    let mut has_deletion = false;
    let mut has_non_ff = false;
    let mut has_status_checks = false;

    for rs in &active {
        let id = rs["id"].as_u64().unwrap_or(0);
        if let Ok(detail) = gh::api_get(&format!("/repos/{repo}/rulesets/{id}")) {
            if let Some(rules) = detail["rules"].as_array() {
                for rule in rules {
                    match rule["type"].as_str() {
                        Some("deletion") => has_deletion = true,
                        Some("non_fast_forward") => has_non_ff = true,
                        Some("required_status_checks") => has_status_checks = true,
                        _ => {}
                    }
                }
            }
        }
    }

    checks.push(bool_check(
        "Deletion rule",
        has_deletion,
        "present",
        "missing",
    ));
    checks.push(bool_check(
        "Non-fast-forward rule",
        has_non_ff,
        "present",
        "missing",
    ));
    checks.push(bool_check(
        "Required status checks rule",
        has_status_checks,
        "present",
        "missing",
    ));

    checks
}

// ── Security ───────────────────────────────────────────────────

fn check_security(repo: &str) -> Vec<Check> {
    let mut checks = Vec::new();

    // Vulnerability alerts (Dependabot) — uses status code
    match gh::api_get_status(&format!("/repos/{repo}/vulnerability-alerts")) {
        Ok(204) => checks.push(Check::pass("Dependabot alerts", "enabled")),
        Ok(404) => checks.push(Check::fail("Dependabot alerts", "disabled")),
        Ok(code) => checks.push(Check::warn(
            "Dependabot alerts",
            format!("unexpected status {code}"),
        )),
        Err(e) => checks.push(Check::error("Dependabot alerts", format!("{e}"))),
    }

    checks
}

fn fix_security(repo: &str, checks: &[Check]) {
    // Enable Dependabot alerts
    if checks
        .iter()
        .any(|c| c.name == "Dependabot alerts" && c.status == Status::Fail)
    {
        println!("  {YELLOW}Enabling Dependabot alerts...{RESET}");
        match gh::api_put(&format!("/repos/{repo}/vulnerability-alerts"), &json!({})) {
            Ok(_) => println!("  {GREEN}Fixed{RESET}"),
            Err(e) => eprintln!("  {RED}Fix failed: {e}{RESET}"),
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────

fn bool_check(name: &'static str, pass: bool, pass_msg: &str, fail_msg: &str) -> Check {
    if pass {
        Check::pass(name, pass_msg)
    } else {
        Check::fail(name, fail_msg)
    }
}
