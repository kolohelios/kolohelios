use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

/// Environment variable a nix-packaged `shaka` uses to point at its
/// bundled CUE module closure (the schema, its `cue.mod`, and the
/// imported domain registry). Set by `wrapProgram` in the generated
/// flake (see `generate_flakes`); unset for the in-repo dev wrapper,
/// which builds in-tree.
const MODULE_DIR_ENV: &str = "SHAKA_CUE_MODULE_DIR";

/// Path of the project schema *relative to the CUE module root* — the
/// layout preserved when the closure is shipped under
/// `$out/share/shaka/cue/` by the packaged binary.
const PROJECT_SCHEMA_REL: &str = "tools/shaka/schema/project-schema.cue";

use super::SLOTS;

/// Absolute path to the project schema file.
///
/// A nix-packaged `shaka` finds it under `$SHAKA_CUE_MODULE_DIR`; the
/// in-repo dev wrapper falls back to the in-tree copy next to the
/// sources (`CARGO_MANIFEST_DIR`). Both are verified to exist so a
/// stale env var or a binary built outside the repo fails with a clear
/// message instead of a downstream `cue` "file not found".
pub fn project_schema_path() -> io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os(MODULE_DIR_ENV) {
        let path = PathBuf::from(dir).join(PROJECT_SCHEMA_REL);
        if path.is_file() {
            return Ok(path);
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{MODULE_DIR_ENV} is set but {} does not exist",
                path.display()
            ),
        ));
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/project-schema.cue");
    if path.is_file() {
        return Ok(path);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "project schema not found at {}; set {MODULE_DIR_ENV} for a packaged binary",
            path.display()
        ),
    ))
}

/// The directory `cue` must run in so the schema's
/// `import "kolohelios.com/infra/cloudflare-dns/domains:domain"` (and
/// the rest of the closure) resolves. CUE locates imports via the
/// `cue.mod/module.cue` found by walking up from its cwd.
///
/// Walk up from the project file to its enclosing module root first, so
/// a `project.cue` inside any checkout is validated against *that*
/// checkout's registry — the live monorepo for the dev wrapper or a
/// nix-built binary run in-repo (#647), or a temp fixture that plants a
/// `cue.mod`. Only when the project sits outside any CUE module (a
/// nix-packaged `shaka` run in a non-module cwd) fall back to the
/// bundled closure at `$SHAKA_CUE_MODULE_DIR`.
fn cue_module_root(project_file: &Path) -> io::Result<PathBuf> {
    let abs = std::fs::canonicalize(project_file)?;
    let mut cursor = abs.parent();
    while let Some(dir) = cursor {
        if dir.join("cue.mod/module.cue").is_file() {
            return Ok(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    if let Some(dir) = std::env::var_os(MODULE_DIR_ENV) {
        let dir = PathBuf::from(dir);
        if dir.join("cue.mod/module.cue").is_file() {
            return Ok(dir);
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{MODULE_DIR_ENV} is set but {} has no cue.mod",
                dir.display()
            ),
        ));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "no cue.mod/module.cue found above {}; set {MODULE_DIR_ENV} for a packaged binary",
            project_file.display()
        ),
    ))
}

/// Run `cue <args> <project-schema> <project-file>` with cwd anchored at
/// the CUE module root so the schema's import closure resolves no matter
/// where the project file lives — including a nix-packaged `shaka` run in
/// a repo with no `cue.mod` of its own. The project file is passed by
/// absolute path because cue's cwd is the module root, not the caller's.
///
/// `args` carries the verb and its flags, e.g. `["vet", "-c"]` or
/// `["export", "--out", "json"]`.
pub fn cue_project(args: &[&str], project_file: &Path) -> io::Result<Output> {
    let schema = project_schema_path()?;
    let module_root = cue_module_root(project_file)?;
    let project_abs = std::fs::canonicalize(project_file)?;
    Command::new("cue")
        .args(args)
        .arg(&schema)
        .arg(&project_abs)
        .current_dir(&module_root)
        .output()
}

enum ProjectResult {
    Pass,
    MissingFile,
    Failed(String),
    Error(String),
}

pub fn run() {
    if let Err(e) = project_schema_path() {
        eprintln!("{RED}{BOLD}error:{RESET} could not resolve project schema: {e}");
        std::process::exit(1);
    }

    let root = Path::new(".");
    let strays = find_stray_project_cues(root);
    if !strays.is_empty() {
        eprintln!(
            "{RED}{BOLD}stray project.cue files:{RESET} {} found (must live at exactly <slot>/<name>/project.cue)",
            strays.len()
        );
        for stray in &strays {
            eprintln!("  {RED}{BOLD}FAIL{RESET}  {}", stray.display());
        }
        eprintln!();
        eprintln!(
            "{RED}{BOLD}schema-check failed{RESET} ({} stray project.cue file(s))",
            strays.len()
        );
        std::process::exit(1);
    }

    let projects = discover(root);
    if projects.is_empty() {
        println!("{YELLOW}no projects found in slots {SLOTS:?}{RESET}");
        return;
    }

    println!("{BOLD}schema-check:{RESET} {} projects", projects.len());

    let mut failures = 0;
    for project in &projects {
        let display = project.display();
        match validate_project(project) {
            ProjectResult::Pass => println!("  {GREEN}{BOLD}ok{RESET}    {display}"),
            ProjectResult::MissingFile => {
                println!("  {RED}{BOLD}FAIL{RESET}  {display} ({DIM}missing project.cue{RESET})");
                failures += 1;
            }
            ProjectResult::Failed(msg) => {
                println!("  {RED}{BOLD}FAIL{RESET}  {display}");
                for line in msg.lines() {
                    println!("    {DIM}{line}{RESET}");
                }
                failures += 1;
            }
            ProjectResult::Error(msg) => {
                println!("  {RED}{BOLD}ERROR{RESET} {display} ({DIM}{msg}{RESET})");
                failures += 1;
            }
        }
    }

    println!();
    if failures > 0 {
        eprintln!(
            "{RED}{BOLD}schema-check failed{RESET} ({failures} of {} projects)",
            projects.len()
        );
        std::process::exit(1);
    }
    println!("{GREEN}{BOLD}schema-check passed{RESET}");
}

pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut projects = Vec::new();
    for slot in SLOTS {
        let slot_dir = root.join(slot);
        let entries = match std::fs::read_dir(&slot_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                projects.push(path);
            }
        }
    }
    projects.sort();
    projects
}

/// Walk every slot recursively and return paths to `project.cue` files that
/// are *not* at the canonical `<slot>/<name>/project.cue` depth. A
/// project.cue at any other depth is silently invisible to `discover()` —
/// neither shallower (`<slot>/project.cue`) nor deeper
/// (`<slot>/<name>/<sub>/project.cue`, `<slot>/<name>/<sub>/<sub>/project.cue`)
/// is permitted. Skips noise dirs (`target`, `.git`, `.jj`, `node_modules`,
/// `.direnv`, `result*`) so build artifacts in a checked-out repo don't
/// produce false positives.
pub fn find_stray_project_cues(root: &Path) -> Vec<PathBuf> {
    let mut strays = Vec::new();
    for slot in SLOTS {
        let slot_dir = root.join(slot);
        if !slot_dir.is_dir() {
            continue;
        }
        // depth=0 here means "directly under slot_dir"; canonical project.cue
        // lives at depth 1 (slot/name/project.cue). Anything else is stray.
        walk_for_stray_cues(&slot_dir, 0, &mut strays);
    }
    strays.sort();
    strays
}

fn walk_for_stray_cues(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if path.is_dir() {
            if is_noise_dir(&name_str) {
                continue;
            }
            walk_for_stray_cues(&path, depth + 1, out);
        } else if name_str == "project.cue" && depth != 1 {
            out.push(path);
        }
    }
}

fn is_noise_dir(name: &str) -> bool {
    matches!(name, "target" | ".git" | ".jj" | "node_modules" | ".direnv")
        || name.starts_with("result")
}

fn validate_project(project_dir: &Path) -> ProjectResult {
    let project_file = project_dir.join("project.cue");
    if !project_file.exists() {
        return ProjectResult::MissingFile;
    }

    match cue_project(&["vet", "-c"], &project_file) {
        Ok(out) if out.status.success() => ProjectResult::Pass,
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let combined = if stderr.is_empty() { stdout } else { stderr };
            ProjectResult::Failed(combined)
        }
        Err(e) => ProjectResult::Error(format!("failed to spawn cue: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn mkdir(root: &Path, rel: &str) {
        fs::create_dir_all(root.join(rel)).unwrap();
    }

    fn touch(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::File::create(path).unwrap();
    }

    #[test]
    fn discover_finds_direct_children_of_each_slot() {
        let tmp = TempDir::new().unwrap();
        mkdir(tmp.path(), "tools/shaka");
        mkdir(tmp.path(), "infra/devbox");
        mkdir(tmp.path(), "apps/foo");

        let projects = discover(tmp.path());

        assert_eq!(projects.len(), 3);
        assert!(projects.iter().any(|p| p.ends_with("apps/foo")));
        assert!(projects.iter().any(|p| p.ends_with("infra/devbox")));
        assert!(projects.iter().any(|p| p.ends_with("tools/shaka")));
    }

    #[test]
    fn discover_returns_empty_for_empty_root() {
        let tmp = TempDir::new().unwrap();
        assert!(discover(tmp.path()).is_empty());
    }

    #[test]
    fn discover_skips_missing_slot_dirs() {
        let tmp = TempDir::new().unwrap();
        mkdir(tmp.path(), "tools/shaka");

        let projects = discover(tmp.path());

        assert_eq!(projects.len(), 1);
        assert!(projects[0].ends_with("tools/shaka"));
    }

    #[test]
    fn discover_ignores_non_slot_directories() {
        let tmp = TempDir::new().unwrap();
        // `docs/` and `.git/` are not slots and never will be; this
        // test guards the slot filter, not the slot list itself.
        // (Pre-#628 this test used `projects/legacy` as the non-slot
        // example — `projects/` was missing from the discovery list
        // at the time, but only because of the bug we're fixing.)
        mkdir(tmp.path(), ".git/refs");
        mkdir(tmp.path(), "docs/intro");
        mkdir(tmp.path(), "tools/shaka");

        let projects = discover(tmp.path());

        assert_eq!(projects.len(), 1);
        assert!(projects[0].ends_with("tools/shaka"));
    }

    #[test]
    fn discover_ignores_files_inside_slots() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "apps/.gitkeep");
        touch(tmp.path(), "tools/README.md");
        mkdir(tmp.path(), "tools/shaka");

        let projects = discover(tmp.path());

        assert_eq!(projects.len(), 1);
        assert!(projects[0].ends_with("tools/shaka"));
    }

    #[test]
    fn discover_does_not_recurse_into_projects() {
        let tmp = TempDir::new().unwrap();
        mkdir(tmp.path(), "tools/shaka/src/subdir");
        mkdir(tmp.path(), "tools/shaka/schema");

        let projects = discover(tmp.path());

        assert_eq!(projects.len(), 1);
        assert!(projects[0].ends_with("tools/shaka"));
    }

    #[test]
    fn discover_returns_sorted_paths() {
        let tmp = TempDir::new().unwrap();
        mkdir(tmp.path(), "tools/zeta");
        mkdir(tmp.path(), "apps/alpha");
        mkdir(tmp.path(), "infra/middle");

        let projects = discover(tmp.path());

        let names: Vec<String> = projects.iter().map(|p| p.display().to_string()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn validate_project_reports_missing_project_file() {
        let tmp = TempDir::new().unwrap();
        mkdir(tmp.path(), "apps/foo");
        let result = validate_project(&tmp.path().join("apps/foo"));
        assert!(matches!(result, ProjectResult::MissingFile));
    }

    #[test]
    fn stray_finder_accepts_canonical_layout() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "apps/foo/project.cue");
        touch(tmp.path(), "tools/shaka/project.cue");
        touch(tmp.path(), "infra/devbox/project.cue");

        let strays = find_stray_project_cues(tmp.path());

        assert!(strays.is_empty(), "got strays: {strays:?}");
    }

    #[test]
    fn stray_finder_flags_shallow_project_cue() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "apps/project.cue");

        let strays = find_stray_project_cues(tmp.path());

        assert_eq!(strays.len(), 1);
        assert!(strays[0].ends_with("apps/project.cue"));
    }

    #[test]
    fn stray_finder_flags_deep_project_cue() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "apps/foo/bar/project.cue");

        let strays = find_stray_project_cues(tmp.path());

        assert_eq!(strays.len(), 1);
        assert!(strays[0].ends_with("apps/foo/bar/project.cue"));
    }

    #[test]
    fn stray_finder_flags_nested_alongside_valid() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "apps/foo/project.cue");
        touch(tmp.path(), "apps/foo/sub/project.cue");

        let strays = find_stray_project_cues(tmp.path());

        assert_eq!(strays.len(), 1);
        assert!(strays[0].ends_with("apps/foo/sub/project.cue"));
    }

    #[test]
    fn stray_finder_skips_noise_dirs() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "tools/shaka/project.cue");
        touch(tmp.path(), "tools/shaka/target/debug/build/foo/project.cue");
        touch(tmp.path(), "tools/shaka/node_modules/pkg/project.cue");
        touch(tmp.path(), "tools/shaka/.direnv/flake-profile/project.cue");
        touch(tmp.path(), "tools/shaka/result-bin/project.cue");

        let strays = find_stray_project_cues(tmp.path());

        assert!(strays.is_empty(), "got strays: {strays:?}");
    }

    #[test]
    fn stray_finder_returns_sorted_paths() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "tools/zeta/sub/project.cue");
        touch(tmp.path(), "apps/alpha/sub/project.cue");
        touch(tmp.path(), "infra/project.cue");

        let strays = find_stray_project_cues(tmp.path());

        let names: Vec<String> = strays.iter().map(|p| p.display().to_string()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn stray_finder_ignores_non_slot_directories() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "docs/project.cue");
        touch(tmp.path(), "scripts/project.cue");

        let strays = find_stray_project_cues(tmp.path());

        assert!(strays.is_empty(), "got strays: {strays:?}");
    }
}
