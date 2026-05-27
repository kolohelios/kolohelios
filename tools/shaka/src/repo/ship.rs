use crate::commit::{self, CommitCommand};
use crate::jj;
use crate::preflight;
use crate::repo::send::{self, resolve_bookmark};
use crate::term::{BOLD, DIM, RED, RESET, YELLOW};

const SHIP_REVSET: &str = "main@origin..@";

pub fn run(bookmark_arg: Option<String>, skip_preflight: bool, dry_run: bool) {
    let description = match jj::current_description() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    let trimmed = description.trim();
    if trimmed.is_empty() {
        eprintln!(
            "{RED}{BOLD}error:{RESET} current change has no description — run `jj describe` first"
        );
        std::process::exit(1);
    }

    let bookmark = match bookmark_arg {
        Some(b) => b,
        None => match resolve_bookmark(trimmed) {
            Ok(b) => b,
            Err(msg) => {
                eprintln!("{RED}{BOLD}error:{RESET} {msg}");
                std::process::exit(1);
            }
        },
    };

    if dry_run {
        println!("would run: jj git fetch");
        println!("would run: jj rebase -b @ -d main@origin");
        println!("would run: shaka commit lint -r {SHIP_REVSET}");
        println!("would run: jj diff -r {SHIP_REVSET}");
        if skip_preflight {
            println!("would skip: shaka preflight (--skip-preflight)");
        } else {
            println!("would run: shaka preflight");
        }
        send::run(Some(bookmark), false, true);
        return;
    }

    println!("{BOLD}step 1/6: fetching from origin{RESET}");
    if let Err(e) = jj::fetch() {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    println!("{BOLD}step 2/6: rebasing onto main@origin{RESET}");
    if let Err(e) = jj::rebase_branch_onto("@", "main@origin") {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    let ahead = match jj::ahead_count("main@origin") {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };
    if ahead == 0 {
        println!(
            "{YELLOW}{BOLD}nothing to ship{RESET} (no non-empty commits ahead of main@origin)"
        );
        return;
    }

    println!("{BOLD}step 3/6: linting commits in {SHIP_REVSET}{RESET}");
    commit::run(CommitCommand::Lint {
        revset: SHIP_REVSET.to_string(),
        allow_no_issue_link: false,
    });

    println!("{BOLD}step 4/6: self-review diff{RESET} {DIM}({SHIP_REVSET}){RESET}");
    if let Err(e) = jj::run_streaming(&["diff", "-r", SHIP_REVSET]) {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    if skip_preflight {
        println!("{BOLD}step 5/6: preflight{RESET} {DIM}(skipped via --skip-preflight){RESET}");
    } else {
        println!("{BOLD}step 5/6: preflight{RESET}");
        preflight::run(false, None);
    }

    println!("{BOLD}step 6/6: handing off to repo send{RESET}");
    send::run(Some(bookmark), false, false);
}
