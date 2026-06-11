use std::path::{Path, PathBuf};
use std::process::Command;

use crate::gh::{self, PrState};
use crate::project::schema_check;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

#[derive(Debug, PartialEq, Eq)]
enum BumpResult {
    Updated,
    Unchanged,
    Skipped,
    Failed(String),
}

pub fn run(input: String, pr_branch: Option<String>, repo: Option<String>, auto_merge: bool) {
    match repo {
        Some(slug) => {
            let Some(branch) = pr_branch else {
                eprintln!(
                    "{RED}{BOLD}--repo requires --pr-branch{RESET} \
                     (remote bumps always open or update a PR)"
                );
                std::process::exit(2);
            };
            if let Err(e) = run_remote(&input, &branch, &slug, auto_merge) {
                eprintln!("{RED}{BOLD}bump-locks failed:{RESET} {e}");
                std::process::exit(1);
            }
        }
        None => run_monorepo(input, pr_branch, auto_merge),
    }
}

fn run_monorepo(input: String, pr_branch: Option<String>, auto_merge: bool) {
    let projects = schema_check::discover(Path::new("."));
    if projects.is_empty() {
        println!("{YELLOW}no projects found{RESET}");
        return;
    }

    println!("{BOLD}bump-locks:{RESET} input `{input}`");

    let mut updated_paths: Vec<PathBuf> = Vec::new();
    let mut unchanged = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for project in &projects {
        let display = project.display();
        match bump_one(project, &input) {
            BumpResult::Updated => {
                println!("  {GREEN}{BOLD}updated{RESET}   {display}");
                updated_paths.push(project.clone());
                if let Err(e) = sync_worker_wasm_bindgen(project) {
                    println!("  {RED}{BOLD}FAIL{RESET}      {display} ({DIM}{e}{RESET})");
                    failed += 1;
                }
            }
            BumpResult::Unchanged => {
                println!("  {DIM}unchanged{RESET} {display}");
                unchanged += 1;
            }
            BumpResult::Skipped => {
                skipped += 1;
            }
            BumpResult::Failed(msg) => {
                println!("  {RED}{BOLD}FAIL{RESET}      {display} ({DIM}{msg}{RESET})");
                failed += 1;
            }
        }
    }

    println!();
    println!(
        "{} projects scanned, {} updated, {unchanged} unchanged, {skipped} skipped",
        projects.len(),
        updated_paths.len(),
    );

    if failed > 0 {
        eprintln!("{RED}{BOLD}bump-locks failed{RESET} ({failed} failure(s))");
        std::process::exit(1);
    }

    if let Some(branch) = pr_branch {
        if let Err(e) = publish_pr(&input, &branch, &updated_paths, auto_merge) {
            eprintln!("{RED}{BOLD}publish failed:{RESET} {e}");
            std::process::exit(1);
        }
    }
}

fn run_remote(input: &str, branch: &str, slug: &str, auto_merge: bool) -> Result<(), String> {
    println!("{BOLD}bump-locks:{RESET} input `{input}` in remote `{slug}`");

    let temp = tempfile::tempdir().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let work = temp.path().join("repo");

    let work_str = work
        .to_str()
        .ok_or_else(|| "temp dir path is not valid UTF-8".to_string())?;
    let output = Command::new("gh")
        .args(["repo", "clone", slug, work_str, "--", "--depth", "1"])
        .output()
        .map_err(|e| format!("failed to spawn gh: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh repo clone {slug}: {}", stderr.trim()));
    }

    match bump_one(&work, input) {
        BumpResult::Updated => {
            println!("  {GREEN}{BOLD}updated{RESET}   {slug}");
        }
        BumpResult::Unchanged => {
            println!("  {DIM}unchanged{RESET} {slug}");
            println!("{DIM}no changes to publish{RESET}");
            return Ok(());
        }
        BumpResult::Skipped => {
            return Err(format!(
                "remote {slug} does not declare `{input}` as a flake input \
                 (checked root flake.nix)"
            ));
        }
        BumpResult::Failed(msg) => {
            return Err(format!("nix flake update failed: {msg}"));
        }
    }

    git_in(&work, &["checkout", "-B", branch])?;
    git_in(&work, &["add", "flake.lock"])?;
    let title = format!("chore(deps): bump {input} flake input");
    git_in(&work, &["commit", "-m", &title])?;
    // Re-anchor on live `main`: previous bump's PR may have merged since the clone (#563).
    git_in(&work, &["fetch", "--depth", "1", "origin", "main"])?;
    git_in(&work, &["rebase", "origin/main"])?;
    git_in(&work, &["push", "--force", "origin", branch])?;

    if let Some(pr) = gh::pr_for_head(slug, branch).map_err(|e| e.to_string())? {
        if pr.state == PrState::Open {
            println!("{GREEN}{BOLD}updated existing PR:{RESET} {}", pr.url);
            if auto_merge {
                enable_auto_merge(&pr.url)?;
            }
            return Ok(());
        }
    }

    let body = format_remote_pr_body(input, auto_merge);
    let url = gh::pr_create(slug, &title, &body, branch).map_err(|e| e.to_string())?;
    println!("{GREEN}{BOLD}created PR:{RESET} {url}");
    if auto_merge {
        enable_auto_merge(&url)?;
    }
    Ok(())
}

fn enable_auto_merge(pr_url: &str) -> Result<(), String> {
    gh::pr_merge_auto_rebase(pr_url).map_err(|e| e.to_string())?;
    println!("  {DIM}auto-merge queued{RESET}");
    Ok(())
}

fn git_in(work: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(work)
        .output()
        .map_err(|e| format!("failed to spawn git {}: {e}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {}: {}", args.join(" "), stderr.trim()));
    }
    Ok(())
}

fn format_remote_pr_body(input: &str, auto_merge: bool) -> String {
    format!(
        "Automated bump of `{input}` flake input.\n\n{}\n",
        merge_footer(auto_merge)
    )
}

fn merge_footer(auto_merge: bool) -> &'static str {
    if auto_merge {
        "Auto-merge queued — GitHub will rebase-merge once required checks pass."
    } else {
        "No auto-merge. Review the lockfile diff and merge when CI is green."
    }
}

fn bump_one(project: &Path, input: &str) -> BumpResult {
    let flake = project.join("flake.nix");
    let Ok(contents) = std::fs::read_to_string(&flake) else {
        return BumpResult::Skipped;
    };
    if !references_flake_input(&contents, input) {
        return BumpResult::Skipped;
    }

    let lock = project.join("flake.lock");
    let before = std::fs::read(&lock).ok();

    let output = match Command::new("nix")
        .args(["flake", "update", input])
        .current_dir(project)
        .output()
    {
        Ok(o) => o,
        Err(e) => return BumpResult::Failed(format!("failed to spawn nix: {e}")),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return BumpResult::Failed(format!(
            "nix flake update failed: {}",
            stderr.lines().last().unwrap_or("(no output)")
        ));
    }

    let after = std::fs::read(&lock).ok();
    if before == after {
        BumpResult::Unchanged
    } else {
        BumpResult::Updated
    }
}

/// The wasm-bindgen crates a `rust-worker` pins in lockstep — their
/// versions move together, so bumping one without the rest leaves an
/// unsatisfiable lock. Every worker depends on the `worker` crate, which
/// pulls all four transitively.
const WASM_BINDGEN_FAMILY: [&str; 4] =
    ["wasm-bindgen", "js-sys", "web-sys", "wasm-bindgen-futures"];

/// Keep a `rust-worker`'s `wasm-bindgen` family at the latest release so
/// it stays at/above `worker-build`'s minimum (#864). `worker-build` is
/// nixpkgs-pinned, so its minimum only moves when the toolchain lock
/// does — pairing this `cargo update` with the `flake.lock` bump keeps the
/// two in lockstep inside a single PR. No-op for non-worker projects; the
/// caller's `git add -A` folds the resulting `Cargo.lock` change into the
/// bump commit.
fn sync_worker_wasm_bindgen(project: &Path) -> Result<(), String> {
    if schema_check::project_kind(project).as_deref() != Some("rust-worker") {
        return Ok(());
    }

    let mut args = vec!["update"];
    for pkg in WASM_BINDGEN_FAMILY {
        args.push("-p");
        args.push(pkg);
    }
    let output = Command::new("cargo")
        .args(&args)
        .current_dir(project)
        .output()
        .map_err(|e| format!("failed to spawn cargo: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "cargo update wasm-bindgen failed: {}",
            stderr.lines().last().unwrap_or("(no output)")
        ));
    }
    Ok(())
}

fn publish_pr(
    input: &str,
    branch: &str,
    updated: &[PathBuf],
    auto_merge: bool,
) -> Result<(), String> {
    if updated.is_empty() {
        println!("{DIM}no changes to publish{RESET}");
        return Ok(());
    }

    git(&["checkout", "-B", branch])?;
    git(&["add", "-A"])?;
    let title = format!("chore(deps): bump {input} flake input");
    git(&["commit", "-m", &title])?;
    git(&["push", "--force-with-lease", "origin", branch])?;

    let repo = gh::detect_repo_or_env().map_err(|e| e.to_string())?;
    if let Some(pr) = gh::pr_for_head(&repo, branch).map_err(|e| e.to_string())? {
        if pr.state == PrState::Open {
            println!("{GREEN}{BOLD}updated existing PR:{RESET} {}", pr.url);
            if auto_merge {
                enable_auto_merge(&pr.url)?;
            }
            return Ok(());
        }
    }

    let body = format_pr_body(input, updated, auto_merge);
    let url = gh::pr_create(&repo, &title, &body, branch).map_err(|e| e.to_string())?;
    println!("{GREEN}{BOLD}created PR:{RESET} {url}");
    if auto_merge {
        enable_auto_merge(&url)?;
    }
    Ok(())
}

fn git(args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("failed to spawn git {}: {e}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {}: {}", args.join(" "), stderr.trim()));
    }
    Ok(())
}

fn format_pr_body(input: &str, updated: &[PathBuf], auto_merge: bool) -> String {
    let mut body =
        format!("Automated daily bump of `{input}` across all FlakeHub-pinned consumers.\n\n");
    body.push_str("Updated `flake.lock` in:\n");
    for project in updated {
        body.push_str(&format!("- `{}`\n", project.display()));
    }
    body.push('\n');
    body.push_str(merge_footer(auto_merge));
    body.push('\n');
    body
}

// Returns true iff `flake.nix` contents declare `input_name` as a flake
// input, in either form: `<input>.url = "..."` (inside an `inputs = { ... }`
// block) or `inputs.<input>.url = "..."` (top-level). Block-form
// (`<input> = { url = ...; }`) is not supported — the bumper fails closed.
// Comments are ignored. The audit rule `kolohelios-nix-via-flakehub`
// guarantees the URL form is canonical for any input we automate.
pub fn references_flake_input(flake_contents: &str, input_name: &str) -> bool {
    let in_block = format!("{input_name}.url");
    let top_level = format!("inputs.{input_name}.url");
    for line in flake_contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let after = trimmed
            .strip_prefix(top_level.as_str())
            .or_else(|| trimmed.strip_prefix(in_block.as_str()));
        if let Some(rest) = after {
            if rest.trim_start().starts_with('=') {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_in_block_form() {
        let nix = "{\n  inputs = {\n    foo.url = \"https://example.com\";\n  };\n}\n";
        assert!(references_flake_input(nix, "foo"));
    }

    #[test]
    fn references_top_level_form() {
        let nix = "{\n  inputs.foo.url = \"https://example.com\";\n}\n";
        assert!(references_flake_input(nix, "foo"));
    }

    #[test]
    fn ignores_comment_only_mention() {
        let nix = "{\n  # foo.url = \"https://stale.example.com\";\n  inputs.bar.url = \"x\";\n}\n";
        assert!(!references_flake_input(nix, "foo"));
    }

    #[test]
    fn ignores_unrelated_attribute_with_substring_match() {
        // `nixpkgs.follows = "kolohelios-nix/nixpkgs"` mentions kolohelios-nix
        // in a value, not as an input declaration.
        let nix = "{\n  inputs.nixpkgs.follows = \"kolohelios-nix/nixpkgs\";\n}\n";
        assert!(!references_flake_input(nix, "kolohelios-nix"));
    }

    #[test]
    fn returns_false_when_input_absent() {
        let nix = "{\n  inputs.bar.url = \"x\";\n}\n";
        assert!(!references_flake_input(nix, "foo"));
    }

    #[test]
    fn rejects_attribute_that_only_shares_a_prefix() {
        // `foo-bar.url` is not the `foo` input even though it starts with `foo`.
        let nix = "{\n  inputs.foo-bar.url = \"x\";\n}\n";
        assert!(!references_flake_input(nix, "foo"));
    }

    #[test]
    fn remote_pr_body_is_terse_and_names_input() {
        let body = format_remote_pr_body("blogctl", false);
        assert!(body.contains("`blogctl`"));
        assert!(body.contains("No auto-merge"));
        // The remote case has only one flake by construction; the body
        // should not list project paths the way the monorepo version does.
        assert!(!body.contains("Updated `flake.lock` in:"));
    }

    #[test]
    fn remote_pr_body_advertises_auto_merge_when_set() {
        let body = format_remote_pr_body("blogctl", true);
        assert!(body.contains("`blogctl`"));
        assert!(body.contains("Auto-merge queued"));
        assert!(!body.contains("No auto-merge"));
    }

    #[test]
    fn pr_body_lists_each_updated_project() {
        let body = format_pr_body(
            "kolohelios-nix",
            &[
                PathBuf::from("./apps/blogctl"),
                PathBuf::from("./infra/devbox"),
            ],
            false,
        );
        assert!(body.contains("`kolohelios-nix`"));
        assert!(body.contains("./apps/blogctl"));
        assert!(body.contains("./infra/devbox"));
        assert!(body.contains("No auto-merge"));
    }

    #[test]
    fn pr_body_advertises_auto_merge_when_set() {
        let body = format_pr_body("kolohelios-nix", &[PathBuf::from("./apps/blogctl")], true);
        assert!(body.contains("Auto-merge queued"));
        assert!(!body.contains("No auto-merge"));
    }
}
