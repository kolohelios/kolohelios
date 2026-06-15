//! `shaka update` — bring *this* host's nix-managed system up to date.
//!
//! Two steps, in order:
//!
//! 1. Bump the `kolohelios-nix` lock the system flake (`infra/home`)
//!    consumes, reusing the same `nix flake update` path as
//!    `shaka repo bump-locks`, so the rebuild picks up the latest shared
//!    lib. (The daily bots keep `main`'s lock fresh; this catches a
//!    machine that hasn't pulled, and is a no-op when already current.)
//! 2. Run the platform-appropriate system rebuild from `infra/home`,
//!    resolving the host and flake path automatically so the command
//!    works on any devbox with no hardcoded hostname or `cwd`.
//!
//! Only macOS (nix-darwin) is wired up today. The NixOS path is a clean
//! seam — `detect_plan` returns a clear "not yet supported" error on
//! other platforms rather than guessing — tracked in #647.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::repo::bump_locks::{bump_one, BumpResult};
use crate::term::{BOLD, DIM, GREEN, RED, RESET};

/// The system flake, relative to the repo root. `infra/home` holds the
/// `darwinConfigurations` (and, eventually, `nixosConfigurations`).
const SYSTEM_FLAKE_REL: &str = "infra/home";

/// The shared-lib input whose lock feeds the system rebuild.
const SHARED_LOCK_INPUT: &str = "kolohelios-nix";

pub fn run(dry_run: bool) {
    if let Err(e) = run_inner(dry_run) {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }
}

fn run_inner(dry_run: bool) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("could not read cwd: {e}"))?;
    let flake_dir = find_system_flake(&cwd).ok_or_else(|| {
        format!(
            "could not find {SYSTEM_FLAKE_REL}/flake.nix in any ancestor of {}",
            cwd.display()
        )
    })?;

    let plan = detect_plan(std::env::consts::OS, &flake_dir)?;
    let (program, args) = plan.command();
    let rebuild = format!("{program} {}", args.join(" "));

    if dry_run {
        println!("{BOLD}shaka update{RESET} {DIM}(dry-run){RESET}");
        println!("{DIM}├──{RESET} system flake: {}", flake_dir.display());
        println!("{DIM}├──{RESET} lock bump:    nix flake update {SHARED_LOCK_INPUT}");
        println!("{DIM}└──{RESET} rebuild:      {rebuild}");
        return Ok(());
    }

    // 1. Bump the shared lock the system flake consumes.
    println!(
        "{BOLD}bumping {SHARED_LOCK_INPUT} lock{RESET} in {}",
        flake_dir.display()
    );
    match bump_one(&flake_dir, SHARED_LOCK_INPUT) {
        BumpResult::Updated => {
            println!(
                "  {GREEN}{BOLD}updated{RESET} {SYSTEM_FLAKE_REL}/flake.lock \
                 {DIM}(commit it via the normal PR flow, or jj restore to discard){RESET}"
            );
        }
        BumpResult::Unchanged => {
            println!("  {DIM}already current{RESET}");
        }
        BumpResult::Skipped => {
            return Err(format!(
                "{SYSTEM_FLAKE_REL}/flake.nix does not declare `{SHARED_LOCK_INPUT}` as an input"
            ));
        }
        BumpResult::Failed(msg) => return Err(msg),
    }

    // 2. Rebuild the system.
    println!("{BOLD}rebuilding system{RESET}: {rebuild}");
    let status = Command::new(&program)
        .args(&args)
        .status()
        .map_err(|e| format!("failed to spawn {program}: {e}"))?;
    if !status.success() {
        return Err(format!("`{rebuild}` failed"));
    }
    println!("{GREEN}{BOLD}system updated{RESET}");
    Ok(())
}

/// Walk upward from `start` for a directory containing
/// `infra/home/flake.nix`, so `shaka update` works from any `cwd` inside
/// the repo (mirrors how the `shaka` wrapper finds the repo root).
fn find_system_flake(start: &Path) -> Option<PathBuf> {
    let mut current = start.canonicalize().ok()?;
    loop {
        let candidate = current.join(SYSTEM_FLAKE_REL);
        if candidate.join("flake.nix").is_file() {
            return Some(candidate);
        }
        current = current.parent()?.to_path_buf();
    }
}

/// The system rebuild to run, resolved per platform.
#[derive(Debug, PartialEq, Eq)]
enum SystemPlan {
    /// macOS via nix-darwin: `darwin-rebuild switch --flake <dir>#<host>`.
    Darwin { flake_dir: PathBuf, host: String },
}

impl SystemPlan {
    /// The `(program, args)` to spawn, with `sudo` since the system
    /// rebuild needs root.
    fn command(&self) -> (String, Vec<String>) {
        match self {
            SystemPlan::Darwin { flake_dir, host } => {
                let flake_ref = format!("{}#{host}", flake_dir.display());
                (
                    "sudo".to_string(),
                    vec![
                        "darwin-rebuild".to_string(),
                        "switch".to_string(),
                        "--flake".to_string(),
                        flake_ref,
                    ],
                )
            }
        }
    }
}

/// Resolve the rebuild plan for `os` (`std::env::consts::OS`). macOS
/// resolves the host from `LocalHostName`; every other platform is an
/// explicit "not yet supported" error rather than a wrong guess.
fn detect_plan(os: &str, flake_dir: &Path) -> Result<SystemPlan, String> {
    match os {
        "macos" => Ok(SystemPlan::Darwin {
            flake_dir: flake_dir.to_path_buf(),
            host: local_host_name()?,
        }),
        other => Err(format!(
            "`shaka update` does not yet support `{other}`; only macOS (nix-darwin) \
             is wired up. The NixOS rebuild path is tracked in #647."
        )),
    }
}

/// The macOS `LocalHostName` — the `darwinConfigurations` attribute the
/// flake keys on (e.g. `Jons-MacBook-Pro`).
fn local_host_name() -> Result<String, String> {
    let output = Command::new("scutil")
        .args(["--get", "LocalHostName"])
        .output()
        .map_err(|e| format!("failed to spawn scutil: {e}"))?;
    if !output.status.success() {
        return Err("scutil --get LocalHostName failed".to_string());
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        return Err("scutil --get LocalHostName returned empty".to_string());
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn find_system_flake_walks_up_to_repo_root() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join(SYSTEM_FLAKE_REL);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("flake.nix"), "{}").unwrap();
        let nested = tmp.path().join("tools/shaka/src");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_system_flake(&nested).unwrap();
        assert!(found.ends_with(SYSTEM_FLAKE_REL));
    }

    #[test]
    fn find_system_flake_none_outside_repo() {
        let tmp = TempDir::new().unwrap();
        assert!(find_system_flake(tmp.path()).is_none());
    }

    #[test]
    fn find_system_flake_requires_flake_nix_not_just_dir() {
        // A bare `infra/home/` dir without flake.nix must not match.
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(SYSTEM_FLAKE_REL)).unwrap();
        assert!(find_system_flake(tmp.path()).is_none());
    }

    #[test]
    fn darwin_command_is_sudo_darwin_rebuild_with_flake_ref() {
        let plan = SystemPlan::Darwin {
            flake_dir: PathBuf::from("/repo/infra/home"),
            host: "Jons-MacBook-Pro".to_string(),
        };
        let (program, args) = plan.command();
        assert_eq!(program, "sudo");
        assert_eq!(
            args,
            vec![
                "darwin-rebuild",
                "switch",
                "--flake",
                "/repo/infra/home#Jons-MacBook-Pro",
            ]
        );
    }

    #[test]
    fn detect_plan_rejects_unsupported_os_with_hint() {
        let err = detect_plan("linux", Path::new("/repo/infra/home")).unwrap_err();
        assert!(err.contains("linux"), "should name the OS: {err}");
        assert!(
            err.contains("#647"),
            "should point at the tracking issue: {err}"
        );
    }
}
