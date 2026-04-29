use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

pub const SCHEMA: &str = include_str!("../../schema/project.cue");
const SLOTS: &[&str] = &["apps", "infra", "nix", "packages", "services", "tools"];

enum ProjectResult {
    Pass,
    MissingFile,
    Failed(String),
    Error(String),
}

pub fn run() {
    let schema_path = match write_schema() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} could not write schema: {e}");
            std::process::exit(1);
        }
    };

    let projects = discover(Path::new("."));
    if projects.is_empty() {
        println!("{YELLOW}no projects found in slots {:?}{RESET}", SLOTS);
        return;
    }

    println!("{BOLD}schema-check:{RESET} {} projects", projects.len());

    let mut failures = 0;
    for project in &projects {
        let display = project.display();
        match validate_project(&schema_path, project) {
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

fn validate_project(schema_path: &Path, project_dir: &Path) -> ProjectResult {
    let project_file = project_dir.join("project.cue");
    if !project_file.exists() {
        return ProjectResult::MissingFile;
    }

    match Command::new("cue")
        .arg("vet")
        .arg("-c")
        .arg(schema_path)
        .arg(&project_file)
        .output()
    {
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

pub fn write_schema() -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join("shaka-project-schema.cue");
    let mut f = std::fs::File::create(&path)?;
    f.write_all(SCHEMA.as_bytes())?;
    Ok(path)
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
        mkdir(tmp.path(), "projects/legacy");
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
        let result = validate_project(
            Path::new("/nonexistent-schema.cue"),
            &tmp.path().join("apps/foo"),
        );
        assert!(matches!(result, ProjectResult::MissingFile));
    }
}
