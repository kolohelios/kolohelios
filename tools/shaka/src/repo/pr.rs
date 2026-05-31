use crate::gh;
use crate::jj;
use crate::repo::describe;
use crate::repo::send::resolve_bookmark;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

pub fn run(bookmark_arg: Option<String>, no_auto_merge: bool, dry_run: bool) {
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

    let synthesized = match describe::for_current_branch() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };
    let title = synthesized.title.as_str();
    let body = synthesized.body.as_str();

    if dry_run {
        println!("would run: jj bookmark set {bookmark} -r @");
        println!("would run: jj git push --allow-new --bookmark {bookmark}");
        println!("would ensure PR exists for head {bookmark}");
        if !no_auto_merge {
            println!("would run: gh pr merge --auto --rebase <pr-url>");
        }
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

    let pr_url = match gh::pr_for_head(&repo, &bookmark) {
        Ok(Some(pr)) => {
            println!("{GREEN}{BOLD}pushed{RESET} (PR: {})", pr.url);
            pr.url
        }
        Ok(None) => match gh::pr_create(&repo, title, body, &bookmark) {
            Ok(url) => {
                println!("{GREEN}{BOLD}created{RESET} ({url})");
                url
            }
            Err(e) => {
                eprintln!("{RED}{BOLD}pr create failed:{RESET} {e}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    if !no_auto_merge {
        match gh::pr_merge_auto_rebase(&pr_url) {
            Ok(()) => println!("{DIM}auto-merge queued{RESET}"),
            Err(e) => {
                eprintln!("{YELLOW}{BOLD}warn:{RESET} could not queue auto-merge for {pr_url}: {e}")
            }
        }
    }
}
