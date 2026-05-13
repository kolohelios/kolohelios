use std::path::Path;
use std::process::Command;

use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

pub fn run(registry_dir: &Path) {
    if !registry_dir.exists() {
        println!(
            "{YELLOW}no domain registry files in {}{RESET}",
            registry_dir.display()
        );
        return;
    }

    println!(
        "{BOLD}domain schema-check:{RESET} {}",
        registry_dir.display()
    );

    match validate_package(registry_dir) {
        Ok(()) => {
            println!("  {GREEN}{BOLD}ok{RESET}");
            println!();
            println!("{GREEN}{BOLD}domain schema-check passed{RESET}");
        }
        Err(msg) => {
            println!("  {RED}{BOLD}FAIL{RESET}");
            for line in msg.lines() {
                println!("    {DIM}{line}{RESET}");
            }
            println!();
            eprintln!("{RED}{BOLD}domain schema-check failed{RESET}");
            std::process::exit(1);
        }
    }
}

/// `cue vet` the registry directory as a package. CUE walks every
/// non-underscore `.cue` file in the directory and unifies them. The
/// in-tree `#Domain` schema is found via the `import schema
/// "kolohelios.com/tools/shaka/schema/domain"` declarations in the
/// registry files, which resolve through `cue.mod/module.cue` at the
/// repo root. Duplicate hostname keys with conflicting values surface
/// here as a unification error, not as a separate Rust-side check.
fn validate_package(registry_dir: &Path) -> Result<(), String> {
    // `current_dir(dir)` + `.` package path: `cue` rejects absolute
    // directory arguments with "cannot use absolute directory as
    // package path". CUE walks up from `current_dir` to find
    // `cue.mod/module.cue`, which lets the registry files' `import
    // schema "kolohelios.com/..."` resolve.
    match Command::new("cue")
        .arg("vet")
        .arg(".")
        .current_dir(registry_dir)
        .output()
    {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let combined = if stderr.is_empty() { stdout } else { stderr };
            Err(combined)
        }
        Err(e) => Err(format!("failed to spawn cue: {e}")),
    }
}
