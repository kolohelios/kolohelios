use std::path::{Path, PathBuf};
use std::process::Command;

use crate::gh::{self, PrState};
use crate::project::schema_check;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BumpResult {
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

    // A consumer's flake may live at the repo root (single-flake repos) or in a
    // project subdir (e.g. buzzingo's `apps/buzzingo`), so discover every
    // consuming project rather than assuming a root flake.
    let targets = consuming_projects(&work, input);
    if targets.is_empty() {
        return Err(format!(
            "remote {slug} does not declare `{input}` as a flake input \
             (checked the root flake and project subdirs)"
        ));
    }
    let mut updated = false;
    for project in &targets {
        let label = project
            .strip_prefix(&work)
            .ok()
            .filter(|rel| !rel.as_os_str().is_empty())
            .map_or_else(|| ".".to_string(), |rel| rel.display().to_string());
        match bump_one(project, input) {
            BumpResult::Updated => {
                println!("  {GREEN}{BOLD}updated{RESET}   {slug} ({label})");
                updated = true;
            }
            BumpResult::Unchanged => {
                println!("  {DIM}unchanged{RESET} {slug} ({label})");
            }
            // `consuming_projects` already filtered to flakes that declare the
            // input, so a non-consumer can't reach here; ignore for safety.
            BumpResult::Skipped => {}
            BumpResult::Failed(msg) => {
                return Err(format!("nix flake update failed in {label}: {msg}"));
            }
        }
    }
    if !updated {
        println!("{DIM}no changes to publish{RESET}");
        return Ok(());
    }

    git_in(&work, &["checkout", "-B", branch])?;
    // `-A` so a subdir consumer's `<project>/flake.lock` is staged, not just a
    // root-level `flake.lock`.
    git_in(&work, &["add", "-A"])?;
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

pub(crate) fn bump_one(project: &Path, input: &str) -> BumpResult {
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

/// Projects within `root` whose `flake.nix` declares `input` as a flake input —
/// the repo root flake (single-flake repos) plus every discovered project slot,
/// so a consumer whose flake lives in a subdir (e.g. buzzingo's `apps/buzzingo`)
/// is found, not only a root flake. Pure: reads `flake.nix` and string-matches,
/// no `nix` invocation.
fn consuming_projects(root: &Path, input: &str) -> Vec<PathBuf> {
    let mut candidates = vec![root.to_path_buf()];
    candidates.extend(schema_check::discover(root));
    candidates.retain(|project| {
        std::fs::read_to_string(project.join("flake.nix"))
            .map(|contents| references_flake_input(&contents, input))
            .unwrap_or(false)
    });
    candidates
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

// Returns true iff `flake.nix` contents declare `input_name` as a bumpable
// flake input, in any of these forms:
//   - `<input>.url = "..."`           (attr, inside `inputs = { ... }`)
//   - `inputs.<input>.url = "..."`    (attr, top-level)
//   - `<input> = { ... url = ...; }`  (block, inside `inputs = { ... }`)
//   - `inputs.<input> = { ... }`      (block, top-level)
// `generate-flakes` emits block form for any input that carries a `follows`
// (e.g. buzzingo's `shaka` / `rust-overlay`), so both forms must be matched. A
// block counts only if it declares a `url` — a follows-only block isn't
// bumpable. Comments are ignored, and prefix-only / value-mention matches are
// rejected (see tests). Known limitation: a non-input attrset elsewhere in the
// file whose attr name happens to equal `input_name` and that has a `url` child
// would also match; generated flakes don't do this.
pub fn references_flake_input(flake_contents: &str, input_name: &str) -> bool {
    // Match the top-level prefix first so `inputs.<input>...` doesn't fall
    // through to the bare-name branch.
    let prefixes = [format!("inputs.{input_name}"), input_name.to_string()];
    let base = flake_contents.as_ptr() as usize;
    for line in flake_contents.lines() {
        // Byte offset of `line` within `flake_contents` (`lines()` yields
        // subslices), robust to `\n` vs `\r\n`.
        let line_start = line.as_ptr() as usize - base;
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        for prefix in &prefixes {
            let Some(rest) = trimmed.strip_prefix(prefix.as_str()) else {
                continue;
            };
            let rest = rest.trim_start();
            // Attr form: `<input>.url = ...`
            if let Some(after_url) = rest.strip_prefix(".url") {
                if after_url.trim_start().starts_with('=') {
                    return true;
                }
            }
            // Block form: `<input> = { ... url = ...; }`. Verify the block
            // declares a `url` so a follows-only block isn't bumped.
            if let Some(after_eq) = rest.strip_prefix('=') {
                if after_eq.trim_start().starts_with('{') {
                    if let Some(brace) = line.find('{') {
                        if block_declares_url(flake_contents, line_start + brace) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

// Starting at the block-opening `{` at byte `open`, return true iff the block
// declares a `url` attribute before it closes. Lexes the block so nothing in a
// string or comment is mistaken for structure: it skips `"..."` (with `\`
// escapes; `${...}` interpolation braces stay inside the string), `''...''` nix
// multiline strings, and `#` line comments, while tracking brace depth — so a
// single-line block or a `url` on the opener line is handled. A follows-only
// block (no `url`) returns false.
fn block_declares_url(flake: &str, open: usize) -> bool {
    enum State {
        Normal,
        DQuote,
        SQuote,
    }
    let bytes = flake.as_bytes();
    let mut depth = 1usize; // already inside the opener's `{`
    let mut state = State::Normal;
    let mut escaped = false;
    let mut i = open + 1;
    while i < bytes.len() {
        let b = bytes[i];
        match state {
            State::DQuote => {
                if escaped {
                    escaped = false;
                } else if b == b'\\' {
                    escaped = true;
                } else if b == b'"' {
                    state = State::Normal;
                }
                i += 1;
            }
            // Nix `''...''` multiline string: ends at the next `''`; its
            // contents (including braces and `"`) are ignored.
            State::SQuote => {
                if b == b'\'' && bytes.get(i + 1) == Some(&b'\'') {
                    state = State::Normal;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            State::Normal => match b {
                b'#' => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
                b'\'' if bytes.get(i + 1) == Some(&b'\'') => {
                    state = State::SQuote;
                    i += 2;
                }
                b'"' => {
                    state = State::DQuote;
                    i += 1;
                }
                b'{' => {
                    depth += 1;
                    i += 1;
                }
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return false; // block closed without a `url`
                    }
                    i += 1;
                }
                b'u' if depth == 1 && is_url_attr(bytes, i) => return true,
                _ => i += 1,
            },
        }
    }
    false
}

// `bytes[i]` is `b'u'`; return true iff it begins a `url` attribute: preceded
// by an attribute boundary (`{`, `;`, or whitespace), spelled exactly `url`
// (not `urls` / `url_x`), and followed by optional whitespace then `=`.
fn is_url_attr(bytes: &[u8], i: usize) -> bool {
    let boundary = i == 0 || matches!(bytes[i - 1], b'{' | b';' | b' ' | b'\t' | b'\n' | b'\r');
    if !boundary || !bytes[i..].starts_with(b"url") {
        return false;
    }
    let mut j = i + 3;
    // Reject identifiers that merely start with `url` (`urls`, `url_x`, ...).
    if matches!(bytes.get(j), Some(c) if c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-' || *c == b'\'')
    {
        return false;
    }
    while matches!(bytes.get(j), Some(b' ' | b'\t')) {
        j += 1;
    }
    bytes.get(j) == Some(&b'=')
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
    fn references_block_form_in_inputs_block() {
        // generate-flakes emits block form for an input carrying a `follows`
        // (e.g. buzzingo's shaka).
        let nix = "{\n  inputs = {\n    shaka = {\n      url = \"https://flakehub.com/f/kolohelios/shaka/*.tar.gz\";\n      inputs.kolohelios-nix.follows = \"kolohelios-nix\";\n    };\n  };\n}\n";
        assert!(references_flake_input(nix, "shaka"));
    }

    #[test]
    fn references_block_form_top_level() {
        let nix = "{\n  inputs.shaka = {\n    url = \"https://example.com\";\n  };\n}\n";
        assert!(references_flake_input(nix, "shaka"));
    }

    #[test]
    fn rejects_follows_only_block_without_url() {
        // A block that only sets `follows` (no url) is not a bumpable input.
        let nix = "{\n  inputs = {\n    foo = {\n      inputs.nixpkgs.follows = \"nixpkgs\";\n    };\n  };\n}\n";
        assert!(!references_flake_input(nix, "foo"));
    }

    #[test]
    fn references_single_line_block() {
        // Opens and closes on one line — the scan must not bail at the closing
        // brace before seeing the url.
        let nix = "{\n  inputs.shaka = { url = \"https://x\"; };\n}\n";
        assert!(references_flake_input(nix, "shaka"));
    }

    #[test]
    fn references_url_on_opener_line() {
        let nix = "{\n  inputs = {\n    shaka = { url = \"https://x\";\n      inputs.nixpkgs.follows = \"nixpkgs\";\n    };\n  };\n}\n";
        assert!(references_flake_input(nix, "shaka"));
    }

    #[test]
    fn brace_inside_sibling_string_does_not_break_scan() {
        // A `{`/`}` inside a sibling string value must not skew brace depth.
        let nix =
            "{\n  inputs.shaka = {\n    meta = \"a}b{c\";\n    url = \"https://x\";\n  };\n}\n";
        assert!(references_flake_input(nix, "shaka"));
    }

    #[test]
    fn url_with_interpolation_is_recognized() {
        let nix = "{\n  inputs.shaka = {\n    url = \"github:o/r/${rev}\";\n  };\n}\n";
        assert!(references_flake_input(nix, "shaka"));
    }

    #[test]
    fn rejects_urls_lookalike_attr_in_block() {
        // `urls = ...` is not a `url` attribute.
        let nix = "{\n  inputs.foo = {\n    urls = \"x\";\n  };\n}\n";
        assert!(!references_flake_input(nix, "foo"));
    }

    #[test]
    fn rejects_commented_url_in_follows_only_block() {
        // A commented-out `url` must not count — the block is follows-only.
        let nix = "{\n  inputs.foo = {\n    # url = \"x\";\n    inputs.nixpkgs.follows = \"nixpkgs\";\n  };\n}\n";
        assert!(!references_flake_input(nix, "foo"));
    }

    #[test]
    fn brace_in_comment_does_not_break_block_scan() {
        // A `}` in a comment must not prematurely close the block.
        let nix = "{\n  inputs.foo = {\n    # closes here }\n    url = \"https://x\";\n  };\n}\n";
        assert!(references_flake_input(nix, "foo"));
    }

    #[test]
    fn multiline_string_sibling_does_not_break_block_scan() {
        // A `''...''` sibling value with a brace must not skew the scan.
        let nix = "{\n  inputs.foo = {\n    a = ''x{y'';\n    url = \"https://x\";\n  };\n}\n";
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

    #[test]
    fn consuming_projects_finds_subdir_flake_not_just_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        // buzzingo-shaped: the flake lives in a project subdir, no root flake.
        let consumer = root.join("apps/buzzingo");
        std::fs::create_dir_all(&consumer).unwrap();
        std::fs::write(
            consumer.join("flake.nix"),
            "{\n  inputs.shaka.url = \"https://flakehub.com/f/kolohelios/shaka/*.tar.gz\";\n}\n",
        )
        .unwrap();
        // Sibling project that doesn't pin the input.
        let other = root.join("apps/other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(
            other.join("flake.nix"),
            "{\n  inputs.nixpkgs.url = \"x\";\n}\n",
        )
        .unwrap();

        assert_eq!(consuming_projects(root, "shaka"), vec![consumer]);
    }

    #[test]
    fn consuming_projects_includes_a_root_flake() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("flake.nix"),
            "{\n  inputs.blogctl.url = \"https://flakehub.com/f/kolohelios/blogctl/*.tar.gz\";\n}\n",
        )
        .unwrap();

        assert_eq!(
            consuming_projects(root, "blogctl"),
            vec![root.to_path_buf()]
        );
    }

    #[test]
    fn consuming_projects_empty_when_input_absent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let proj = tmp.path().join("apps/thing");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("flake.nix"),
            "{\n  inputs.nixpkgs.url = \"x\";\n}\n",
        )
        .unwrap();

        assert!(consuming_projects(tmp.path(), "shaka").is_empty());
    }
}
