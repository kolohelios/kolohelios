use std::path::Path;

use serde_json::{json, Map, Value};

use crate::gh;
use crate::repo::policy::{
    self, BranchProtectionPolicy, MergePolicy, RepoPolicy, RulesetsPolicy, SecurityPolicy,
};
use crate::term::{BOLD, GREEN, RED, RESET, YELLOW};

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
    // Policy source decision: when --repo is explicit, fetch the
    // target's own .shaka/repo.cue via the GitHub contents API.
    // Using cwd's policy across repos compared the wrong policy
    // against the wrong live state (#465). When --repo is omitted,
    // load cwd's policy as before.
    let (repo, policy) = match repo_arg {
        Some(target) => {
            let cue = match gh::fetch_raw_file(&target, ".shaka/repo.cue") {
                Ok(Some(c)) => c,
                Ok(None) => {
                    eprintln!("{RED}{BOLD}error:{RESET} no .shaka/repo.cue found in {target}");
                    eprintln!(
                        "Hint: declare the repo's desired settings in \
                         .shaka/repo.cue at its root, or omit --repo to \
                         audit the current repo against its own policy"
                    );
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("{RED}{BOLD}error:{RESET} fetching policy from {target}: {e}");
                    std::process::exit(1);
                }
            };
            let p = match policy::load_from_string(&cue) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!(
                        "{RED}{BOLD}error:{RESET} parsing .shaka/repo.cue from {target}: {e}"
                    );
                    std::process::exit(1);
                }
            };
            (target, p)
        }
        None => {
            let repo = match gh::detect_repo() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("{RED}{BOLD}error:{RESET} {e}");
                    eprintln!("Hint: pass --repo owner/repo explicitly");
                    std::process::exit(1);
                }
            };
            let p = match policy::load(Path::new(".")) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{RED}{BOLD}error:{RESET} {e}");
                    eprintln!(
                        "Hint: create `.shaka/repo.cue` at the repo root \
                        describing your desired GitHub settings"
                    );
                    std::process::exit(1);
                }
            };
            (repo, p)
        }
    };

    println!("{BOLD}Auditing {repo}{RESET}\n");

    let mut has_failure = false;

    let (checks, _repo_data) = check_general(&repo, &policy);
    print_group("Repository Settings", &checks);
    has_failure |= checks.iter().any(|c| c.status == Status::Fail);

    if fix {
        fix_general(&repo, &checks, &policy);
    }

    if let Some(bp) = &policy.branch_protection {
        let checks = check_branch_protection(&repo, bp);
        print_group("Branch Protection (main)", &checks);
        has_failure |= checks.iter().any(|c| c.status == Status::Fail);

        if fix {
            fix_branch_protection(&repo, &checks, bp);
        }
    }

    if let Some(rs) = &policy.rulesets {
        let bp_ref = policy.branch_protection.as_ref();
        let expected_checks = bp_ref.map(|bp| bp.required_checks.as_slice());
        let expected_strict = bp_ref.map(|bp| bp.strict_status_checks);
        let checks = check_rulesets(&repo, rs, expected_checks, expected_strict);
        print_group("Rulesets", &checks);
        has_failure |= checks.iter().any(|c| c.status == Status::Fail);

        if fix {
            fix_rulesets(&repo, &checks, expected_strict);
        }
    }

    if let Some(sec) = &policy.security {
        let checks = check_security(&repo, sec);
        print_group("Security", &checks);
        has_failure |= checks.iter().any(|c| c.status == Status::Fail);

        if fix {
            fix_security(&repo, &checks);
        }
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

fn check_general(repo: &str, policy: &RepoPolicy) -> (Vec<Check>, Option<Value>) {
    let data = match gh::api_get(&format!("/repos/{repo}")) {
        Ok(v) => v,
        Err(e) => {
            return (
                vec![Check::error("Repository", format!("API error: {e}"))],
                None,
            );
        }
    };

    let mut checks = Vec::with_capacity(8);
    let observed_branch = data["default_branch"].as_str().unwrap_or("unknown");
    checks.push(bool_check(
        "Default branch",
        observed_branch == policy.default_branch,
        &policy.default_branch,
        observed_branch,
    ));

    if let Some(want_issues) = policy.issues {
        let observed = data["has_issues"].as_bool() == Some(true);
        let (pass_msg, fail_msg) = enabled_messages(want_issues);
        checks.push(bool_check(
            "Issues enabled",
            observed == want_issues,
            pass_msg,
            fail_msg,
        ));
    }

    let observed_bool = |key: &str| data[key].as_bool() == Some(true);
    checks.extend([
        merge_check(
            "Rebase merge",
            observed_bool("allow_rebase_merge"),
            policy.merge.rebase,
        ),
        merge_check(
            "Merge commits",
            observed_bool("allow_merge_commit"),
            policy.merge.merge,
        ),
        merge_check(
            "Squash merge",
            observed_bool("allow_squash_merge"),
            policy.merge.squash,
        ),
        merge_check(
            "Delete branch on merge",
            observed_bool("delete_branch_on_merge"),
            policy.merge.delete_branch_on_merge,
        ),
        merge_check(
            "Auto-merge",
            observed_bool("allow_auto_merge"),
            policy.merge.auto_merge,
        ),
    ]);

    (checks, Some(data))
}

fn merge_check(name: &'static str, observed: bool, want: bool) -> Check {
    let (pass_msg, fail_msg) = enabled_messages(want);
    bool_check(name, observed == want, pass_msg, fail_msg)
}

fn enabled_messages(want: bool) -> (&'static str, &'static str) {
    if want {
        ("enabled", "disabled (should be enabled)")
    } else {
        ("disabled", "enabled (should be disabled)")
    }
}

fn fix_general(repo: &str, checks: &[Check], policy: &RepoPolicy) {
    let needs_fix = checks.iter().any(|c| {
        c.status == Status::Fail
            && matches!(
                c.name,
                "Rebase merge"
                    | "Merge commits"
                    | "Squash merge"
                    | "Delete branch on merge"
                    | "Auto-merge"
                    | "Issues enabled"
            )
    });

    if !needs_fix {
        return;
    }

    println!("  {YELLOW}Fixing repository settings...{RESET}");
    let mut body = Map::new();
    if let Some(want_issues) = policy.issues {
        body.insert("has_issues".into(), json!(want_issues));
    }
    let MergePolicy {
        rebase,
        merge,
        squash,
        delete_branch_on_merge,
        auto_merge,
    } = policy.merge;
    body.insert("allow_rebase_merge".into(), json!(rebase));
    body.insert("allow_merge_commit".into(), json!(merge));
    body.insert("allow_squash_merge".into(), json!(squash));
    body.insert(
        "delete_branch_on_merge".into(),
        json!(delete_branch_on_merge),
    );
    body.insert("allow_auto_merge".into(), json!(auto_merge));

    match gh::api_patch(&format!("/repos/{repo}"), &Value::Object(body)) {
        Ok(_) => println!("  {GREEN}Fixed{RESET}"),
        Err(e) => eprintln!("  {RED}Fix failed: {e}{RESET}"),
    }
}

// ── Branch protection ──────────────────────────────────────────

fn check_branch_protection(repo: &str, bp: &BranchProtectionPolicy) -> Vec<Check> {
    let data = match gh::api_get(&format!("/repos/{repo}/branches/main/protection")) {
        Ok(v) => v,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("404") || msg.contains("not protected") || msg.contains("Not Found") {
                return vec![Check::fail("Branch protection", "not enabled on main")];
            }
            return vec![Check::error("Branch protection", format!("API error: {e}"))];
        }
    };

    let observed_strict = data["required_status_checks"]["strict"].as_bool() == Some(true);
    let observed_force = data["allow_force_pushes"]["enabled"].as_bool() == Some(true);
    let observed_delete = data["allow_deletions"]["enabled"].as_bool() == Some(true);

    let observed_checks: Vec<String> = data["required_status_checks"]["contexts"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let want_set: std::collections::BTreeSet<&str> =
        bp.required_checks.iter().map(String::as_str).collect();
    let observed_set: std::collections::BTreeSet<&str> =
        observed_checks.iter().map(String::as_str).collect();
    let checks_match = want_set == observed_set;

    vec![
        Check::pass("Branch protection", "enabled"),
        bool_check(
            "Required status checks",
            checks_match,
            &checks_summary(&bp.required_checks),
            &format!(
                "expected {:?}, got {:?}",
                bp.required_checks, observed_checks
            ),
        ),
        bool_check(
            "Strict status checks",
            observed_strict == bp.strict_status_checks,
            if bp.strict_status_checks {
                "on (forces rebase before merge)"
            } else {
                "off (stale-but-mergeable allowed)"
            },
            if bp.strict_status_checks {
                "off (should be on)"
            } else {
                "on (should be off — forces rebase before merge)"
            },
        ),
        bool_check(
            "Force push",
            observed_force == bp.allow_force_push,
            if bp.allow_force_push {
                "allowed"
            } else {
                "blocked"
            },
            if bp.allow_force_push {
                "blocked (should be allowed)"
            } else {
                "allowed (should be blocked)"
            },
        ),
        bool_check(
            "Branch deletion",
            observed_delete == bp.allow_deletion,
            if bp.allow_deletion {
                "allowed"
            } else {
                "blocked"
            },
            if bp.allow_deletion {
                "blocked (should be allowed)"
            } else {
                "allowed (should be blocked)"
            },
        ),
    ]
}

fn checks_summary(want: &[String]) -> String {
    if want.is_empty() {
        "no required checks (configured)".into()
    } else {
        format!("{:?}", want)
    }
}

fn fix_branch_protection(repo: &str, checks: &[Check], bp: &BranchProtectionPolicy) {
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
            "strict": bp.strict_status_checks,
            "contexts": bp.required_checks,
        },
        "enforce_admins": true,
        "required_pull_request_reviews": null,
        "restrictions": null,
        "allow_force_pushes": bp.allow_force_push,
        "allow_deletions": bp.allow_deletion,
    });

    match gh::api_put(&format!("/repos/{repo}/branches/main/protection"), &body) {
        Ok(_) => println!("  {GREEN}Fixed{RESET}"),
        Err(e) => eprintln!("  {RED}Fix failed: {e}{RESET}"),
    }
}

// ── Rulesets ───────────────────────────────────────────────────

fn check_rulesets(
    repo: &str,
    policy: &RulesetsPolicy,
    expected_checks: Option<&[String]>,
    expected_strict: Option<bool>,
) -> Vec<Check> {
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

    let mut has_deletion = false;
    let mut has_non_ff = false;
    let mut has_status_checks = false;
    let mut observed_check_contexts: Vec<String> = Vec::new();
    let mut observed_strict: Option<bool> = None;

    for rs in &active {
        let id = rs["id"].as_u64().unwrap_or(0);
        if let Ok(detail) = gh::api_get(&format!("/repos/{repo}/rulesets/{id}")) {
            if let Some(rules) = detail["rules"].as_array() {
                for rule in rules {
                    match rule["type"].as_str() {
                        Some("deletion") => has_deletion = true,
                        Some("non_fast_forward") => has_non_ff = true,
                        Some("required_status_checks") => {
                            has_status_checks = true;
                            let params = &rule["parameters"];
                            if let Some(contexts) = params["required_status_checks"].as_array() {
                                for entry in contexts {
                                    if let Some(ctx) = entry["context"].as_str() {
                                        observed_check_contexts.push(ctx.to_string());
                                    }
                                }
                            }
                            observed_strict =
                                params["strict_required_status_checks_policy"].as_bool();
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if policy.require_deletion {
        checks.push(bool_check(
            "Deletion rule",
            has_deletion,
            "present",
            "missing",
        ));
    }
    if policy.require_non_fast_forward {
        checks.push(bool_check(
            "Non-fast-forward rule",
            has_non_ff,
            "present",
            "missing",
        ));
    }
    if policy.require_status_checks {
        checks.push(bool_check(
            "Required status checks rule",
            has_status_checks,
            "present",
            "missing",
        ));
        // Cross-check the ruleset's required-check contexts against the
        // expected list (sourced from `branchProtection.requiredChecks`).
        // Without an expected list the audit cannot tell whether the
        // ruleset contexts match policy intent, so it surfaces the policy
        // gap rather than silently passing.
        match expected_checks {
            Some(want) if !want.is_empty() => {
                let want_set: std::collections::BTreeSet<&str> =
                    want.iter().map(String::as_str).collect();
                let observed_set: std::collections::BTreeSet<&str> =
                    observed_check_contexts.iter().map(String::as_str).collect();
                checks.push(bool_check(
                    "Ruleset status check contexts",
                    want_set == observed_set,
                    &checks_summary(want),
                    &format!("expected {:?}, got {:?}", want, observed_check_contexts),
                ));
            }
            _ => {
                checks.push(Check::fail(
                    "Ruleset status check contexts",
                    "policy.rulesets.requireStatusChecks is true but policy.branchProtection.requiredChecks is empty or absent",
                ));
            }
        }

        if let (Some(observed), Some(want)) = (observed_strict, expected_strict) {
            checks.push(bool_check(
                "Ruleset strict status checks",
                observed == want,
                if want {
                    "on (forces rebase before merge)"
                } else {
                    "off (stale-but-mergeable allowed)"
                },
                if want {
                    "off (should be on)"
                } else {
                    "on (should be off — forces rebase before merge)"
                },
            ));
        }
    }

    checks
}

fn fix_rulesets(repo: &str, checks: &[Check], expected_strict: Option<bool>) {
    let strict_needs_fix = checks
        .iter()
        .any(|c| c.name == "Ruleset strict status checks" && c.status == Status::Fail);

    if !strict_needs_fix {
        return;
    }

    let Some(want_strict) = expected_strict else {
        return;
    };

    let rulesets = match gh::api_get(&format!("/repos/{repo}/rulesets")) {
        Ok(Value::Array(arr)) => arr,
        _ => return,
    };

    for rs in &rulesets {
        let Some(id) = rs["id"].as_u64() else {
            continue;
        };
        if rs["enforcement"].as_str() != Some("active") {
            continue;
        }
        let Ok(detail) = gh::api_get(&format!("/repos/{repo}/rulesets/{id}")) else {
            continue;
        };
        let Some(rules) = detail["rules"].as_array() else {
            continue;
        };
        let has_status_check = rules
            .iter()
            .any(|r| r["type"].as_str() == Some("required_status_checks"));
        if !has_status_check {
            continue;
        }

        let mut updated_rules: Vec<Value> = rules.clone();
        for rule in &mut updated_rules {
            if rule["type"].as_str() == Some("required_status_checks") {
                rule["parameters"]["strict_required_status_checks_policy"] =
                    Value::Bool(want_strict);
            }
        }

        let body = json!({
            "name": detail["name"],
            "target": detail["target"],
            "enforcement": detail["enforcement"],
            "conditions": detail["conditions"],
            "rules": updated_rules,
            "bypass_actors": detail["bypass_actors"],
        });

        println!("  {YELLOW}Fixing ruleset strict status checks...{RESET}");
        match gh::api_put(&format!("/repos/{repo}/rulesets/{id}"), &body) {
            Ok(_) => println!("  {GREEN}Fixed{RESET}"),
            Err(e) => eprintln!("  {RED}Fix failed: {e}{RESET}"),
        }
    }
}

// ── Security ───────────────────────────────────────────────────

fn check_security(repo: &str, policy: &SecurityPolicy) -> Vec<Check> {
    let mut checks = Vec::new();

    let want = policy.dependabot_alerts;
    match gh::api_get_status(&format!("/repos/{repo}/vulnerability-alerts")) {
        Ok(204) => {
            checks.push(if want {
                Check::pass("Dependabot alerts", "enabled")
            } else {
                Check::fail("Dependabot alerts", "enabled (should be disabled)")
            });
        }
        Ok(404) => {
            checks.push(if !want {
                Check::pass("Dependabot alerts", "disabled")
            } else {
                Check::fail("Dependabot alerts", "disabled (should be enabled)")
            });
        }
        Ok(code) => checks.push(Check::warn(
            "Dependabot alerts",
            format!("unexpected status {code}"),
        )),
        Err(e) => checks.push(Check::error("Dependabot alerts", format!("{e}"))),
    }

    checks
}

fn fix_security(repo: &str, checks: &[Check]) {
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
