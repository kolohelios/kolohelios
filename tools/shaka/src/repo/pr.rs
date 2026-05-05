use crate::gh;
use crate::jj;
use crate::repo::send::{resolve_bookmark, split_message};

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

pub fn run(bookmark_arg: Option<String>, dry_run: bool) {
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
        println!("would run: jj bookmark set {bookmark} -r @");
        println!("would run: jj git push --allow-new --bookmark {bookmark}");
        println!("would ensure PR exists for head {bookmark}");
        return;
    }

    println!("{BOLD}setting bookmark {bookmark}{RESET}");
    if let Err(e) = jj::set_bookmark(&bookmark) {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    println!("{BOLD}pushing {bookmark}{RESET}");
    if let Err(e) = jj::push_bookmark(&bookmark) {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    let repo = match gh::detect_repo() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    match gh::pr_for_head(&repo, &bookmark) {
        Ok(Some(pr)) => println!("{GREEN}{BOLD}pushed{RESET} (PR: {})", pr.url),
        Ok(None) => match gh::pr_create(&repo, title, body, &bookmark) {
            Ok(url) => println!("{GREEN}{BOLD}created{RESET} ({url})"),
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
