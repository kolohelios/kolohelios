use crate::gh;
use crate::jj;
use crate::repo::send::split_message;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

pub fn run(bookmark_arg: Option<String>, dry_run: bool) {
    let bookmark = match bookmark_arg {
        Some(b) => b,
        None => match resolve_bookmark() {
            Ok(b) => b,
            Err(msg) => {
                eprintln!("{RED}{BOLD}error:{RESET} {msg}");
                std::process::exit(1);
            }
        },
    };

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
        Ok(Some(url)) => println!("{GREEN}{BOLD}pushed{RESET} (PR: {url})"),
        Ok(None) => match gh::pr_create(title, body, &bookmark) {
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

fn resolve_bookmark() -> Result<String, String> {
    let bookmarks = jj::current_bookmarks().map_err(|e| e.to_string())?;
    if bookmarks.len() == 1 {
        return Ok(bookmarks.into_iter().next().unwrap());
    }
    if bookmarks.is_empty() {
        let desc = jj::current_description().map_err(|e| e.to_string())?;
        return jj::derive_bookmark(desc.trim()).ok_or_else(|| {
            "no bookmark on current change and could not derive one — pass --bookmark".to_string()
        });
    }
    Err(format!(
        "multiple bookmarks on current change ({}); pass --bookmark to disambiguate",
        bookmarks.join(", ")
    ))
}
