use crate::jj;
use crate::term::{BOLD, GREEN, RED, RESET};

pub fn run(dry_run: bool) {
    if dry_run {
        println!("would run: jj git fetch");
        println!("would run: jj rebase -d main@origin");
        return;
    }

    println!("{BOLD}fetching from origin{RESET}");
    if let Err(e) = jj::fetch() {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    println!("{BOLD}rebasing onto main@origin{RESET}");
    if let Err(e) = jj::rebase_onto("main@origin") {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    println!("{GREEN}{BOLD}synced{RESET}");
}
