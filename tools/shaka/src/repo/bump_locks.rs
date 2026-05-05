use std::path::Path;
use std::process::Command;

use crate::project::schema_check;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[derive(Debug, PartialEq, Eq)]
enum BumpResult {
    Updated,
    Unchanged,
    Skipped,
    Failed(String),
}

pub fn run(input: String) {
    let projects = schema_check::discover(Path::new("."));
    if projects.is_empty() {
        println!("{YELLOW}no projects found{RESET}");
        return;
    }

    println!("{BOLD}bump-locks:{RESET} input `{input}`");

    let mut updated = 0usize;
    let mut unchanged = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for project in &projects {
        let display = project.display();
        match bump_one(project, &input) {
            BumpResult::Updated => {
                println!("  {GREEN}{BOLD}updated{RESET}   {display}");
                updated += 1;
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
        "{} projects scanned, {updated} updated, {unchanged} unchanged, {skipped} skipped",
        projects.len(),
    );

    if failed > 0 {
        eprintln!("{RED}{BOLD}bump-locks failed{RESET} ({failed} failure(s))");
        std::process::exit(1);
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
}
