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

pub fn run(input: String, pr_branch: Option<String>) {
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
        if let Err(e) = publish_pr(&input, &branch, &updated_paths) {
            eprintln!("{RED}{BOLD}publish failed:{RESET} {e}");
            std::process::exit(1);
        }
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

fn publish_pr(input: &str, branch: &str, updated: &[PathBuf]) -> Result<(), String> {
    if updated.is_empty() {
        println!("{DIM}no changes to publish{RESET}");
        return Ok(());
    }

    git(&["checkout", "-B", branch])?;
    git(&["add", "-A"])?;
    let title = format!("chore(deps): bump {input} flake input");
    git(&["commit", "-m", &title])?;
    git(&["push", "--force-with-lease", "origin", branch])?;

    let repo = gh::detect_repo_or_env().map_err(|e| e.message)?;
    if let Some(pr) = gh::pr_for_head(&repo, branch).map_err(|e| e.message)? {
        if pr.state == PrState::Open {
            println!("{GREEN}{BOLD}updated existing PR:{RESET} {}", pr.url);
            return Ok(());
        }
    }

    let body = format_pr_body(input, updated);
    let url = gh::pr_create(&repo, &title, &body, branch).map_err(|e| e.message)?;
    println!("{GREEN}{BOLD}created PR:{RESET} {url}");
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

fn format_pr_body(input: &str, updated: &[PathBuf]) -> String {
    let mut body =
        format!("Automated daily bump of `{input}` across all FlakeHub-pinned consumers.\n\n");
    body.push_str("Updated `flake.lock` in:\n");
    for project in updated {
        body.push_str(&format!("- `{}`\n", project.display()));
    }
    body.push_str("\nNo auto-merge. Review the lockfile diff and merge when CI is green.\n");
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
    fn pr_body_lists_each_updated_project() {
        let body = format_pr_body(
            "kolohelios-nix",
            &[
                PathBuf::from("./apps/blogctl"),
                PathBuf::from("./infra/devbox"),
            ],
        );
        assert!(body.contains("`kolohelios-nix`"));
        assert!(body.contains("./apps/blogctl"));
        assert!(body.contains("./infra/devbox"));
        assert!(body.contains("No auto-merge"));
    }
}
