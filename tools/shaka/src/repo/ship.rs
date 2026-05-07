use crate::commit::{self, CommitCommand};
use crate::gh;
use crate::jj;
use crate::preflight;
use crate::repo::send::{resolve_bookmark, split_message};
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

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

    let (title, body) = split_message(trimmed);

    if dry_run {
        println!("would run: jj git fetch");
        println!("would run: jj rebase -d main@origin");
        println!("would run: shaka commit lint -r {SHIP_REVSET}");
        println!("would run: jj diff -r {SHIP_REVSET}");
        if skip_preflight {
            println!("would skip: shaka preflight (--skip-preflight)");
        } else {
            println!("would run: shaka preflight");
        }
        println!("would run: jj bookmark set {bookmark} -r @");
        println!("would run: jj git push --allow-new --bookmark {bookmark}");
        println!("would ensure PR exists for head {bookmark}");
        return;
    }

    println!("{BOLD}step 1/6: fetching from origin{RESET}");
    if let Err(e) = jj::fetch() {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    println!("{BOLD}step 2/6: rebasing onto main@origin{RESET}");
    if let Err(e) = jj::rebase_onto("main@origin") {
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

    println!("{BOLD}step 6/6: push and ensure PR{RESET}");
    if let Err(e) = jj::set_bookmark(&bookmark) {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }
    if let Err(e) = jj::push_bookmark(&bookmark) {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    let repo = match gh::detect_repo() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{YELLOW}{BOLD}warn:{RESET} could not detect repo: {e}");
            println!("{GREEN}{BOLD}pushed{RESET} (skipping PR)");
            return;
        }
    };

    match gh::pr_for_head(&repo, &bookmark) {
        Ok(Some(pr)) => println!("{GREEN}{BOLD}shipped{RESET} (PR: {})", pr.url),
        Ok(None) => match gh::pr_create(&repo, title, body, &bookmark) {
            Ok(url) => println!("{GREEN}{BOLD}shipped{RESET} ({url})"),
            Err(e) => {
                eprintln!("{RED}{BOLD}pr create failed:{RESET} {e}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    }
}
