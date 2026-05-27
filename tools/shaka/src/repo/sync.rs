use crate::gh;
use crate::jj;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};
use crate::workspace::staleness::{self, Staleness};

pub fn run(dry_run: bool, no_cleanup_hint: bool) {
    if dry_run {
        println!("would run: jj git fetch");
        println!("would run: jj rebase -b @ -d main@origin");
        return;
    }

    println!("{BOLD}fetching from origin{RESET}");
    if let Err(e) = jj::fetch() {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    println!("{BOLD}rebasing onto main@origin{RESET}");
    if let Err(e) = jj::rebase_branch_onto("@", "main@origin") {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    println!("{GREEN}{BOLD}synced{RESET}");

    if !no_cleanup_hint {
        print_cleanup_hint();
    }
}

/// Walk every workspace, classify each via the shared staleness module,
/// and print a one-line hint per stale workspace. Errors are swallowed
/// — the hint is a side-info; failing to query GitHub shouldn't break
/// a sync that already succeeded.
fn print_cleanup_hint() {
    let repo_root = match jj::repo_root() {
        Ok(p) => p,
        Err(_) => return,
    };
    let repo = match gh::detect_repo() {
        Ok(r) => r,
        Err(_) => return,
    };

    let classified = staleness::classify_all(&repo_root, &repo);
    let stale: Vec<&(String, Staleness)> = classified
        .iter()
        .filter(|(_, s)| !matches!(s, Staleness::Active))
        .collect();

    if stale.is_empty() {
        return;
    }

    let plural = if stale.len() == 1 { "" } else { "s" };
    println!(
        "{YELLOW}hint: {} workspace{plural} look stale; run shaka workspace cleanup{RESET}",
        stale.len()
    );
    for (name, s) in &stale {
        match s {
            Staleness::Merged { pr_url } => {
                println!("  {BOLD}{name}{RESET} {DIM}(merged: {pr_url}){RESET}")
            }
            Staleness::Orphan => println!("  {BOLD}{name}{RESET} {DIM}(orphan){RESET}"),
            Staleness::Active => unreachable!("filtered above"),
        }
    }
}
