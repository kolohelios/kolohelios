use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

const SCHEMA: &str = include_str!("../../../schema/cloudflare-token.cue");

enum FileResult {
    Pass,
    Failed(String),
    Error(String),
}

pub fn run(registry_dir: &Path) {
    let schema_path = match write_schema() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} could not write schema: {e}");
            std::process::exit(1);
        }
    };

    let files = match discover(registry_dir) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "{RED}{BOLD}error:{RESET} could not read registry {}: {e}",
                registry_dir.display()
            );
            std::process::exit(1);
        }
    };

    if files.is_empty() {
        println!(
            "{YELLOW}no cloudflare-token registry files in {}{RESET}",
            registry_dir.display()
        );
        return;
    }

    println!(
        "{BOLD}token cloudflare schema-check:{RESET} {} files in {}",
        files.len(),
        registry_dir.display()
    );

    let mut failures = 0;
    for file in &files {
        let display = file.display();
        match validate_file(&schema_path, file) {
            FileResult::Pass => println!("  {GREEN}{BOLD}ok{RESET}    {display}"),
            FileResult::Failed(msg) => {
                println!("  {RED}{BOLD}FAIL{RESET}  {display}");
                for line in msg.lines() {
                    println!("    {DIM}{line}{RESET}");
                }
                failures += 1;
            }
            FileResult::Error(msg) => {
                println!("  {RED}{BOLD}ERROR{RESET} {display} ({DIM}{msg}{RESET})");
                failures += 1;
            }
        }
    }

    println!();
    if failures > 0 {
        eprintln!(
            "{RED}{BOLD}token cloudflare schema-check failed{RESET} ({failures} of {} files)",
            files.len()
        );
        std::process::exit(1);
    }
    println!("{GREEN}{BOLD}token cloudflare schema-check passed{RESET}");
}

fn discover(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("cue"))
        .collect();
    files.sort();
    Ok(files)
}

fn validate_file(schema_path: &Path, file: &Path) -> FileResult {
    match Command::new("cue")
        .arg("vet")
        .arg("-c")
        .arg(schema_path)
        .arg(file)
        .output()
    {
        Ok(out) if out.status.success() => FileResult::Pass,
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let combined = if stderr.is_empty() { stdout } else { stderr };
            FileResult::Failed(combined)
        }
        Err(e) => FileResult::Error(format!("failed to spawn cue: {e}")),
    }
}

fn write_schema() -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "shaka-cloudflare-token-schema-check-{}.cue",
        std::process::id()
    ));
    let mut f = std::fs::File::create(&path)?;
    f.write_all(SCHEMA.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn discover_returns_empty_for_missing_dir() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("nonexistent");
        assert_eq!(discover(&missing).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn discover_returns_only_cue_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.cue"), "").unwrap();
        fs::write(tmp.path().join("b.cue"), "").unwrap();
        fs::write(tmp.path().join("readme.md"), "").unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();

        let files = discover(tmp.path()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|p| p.extension().unwrap() == "cue"));
    }

    #[test]
    fn discover_returns_sorted_paths() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("zeta.cue"), "").unwrap();
        fs::write(tmp.path().join("alpha.cue"), "").unwrap();
        fs::write(tmp.path().join("middle.cue"), "").unwrap();

        let files = discover(tmp.path()).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["alpha.cue", "middle.cue", "zeta.cue"]);
    }
}
