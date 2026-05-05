use crate::gh;
use crate::jj;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

pub fn run(bookmark_arg: Option<String>, no_pr: bool, dry_run: bool) {
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
        if !no_pr {
            println!("would run: gh pr create --title {title:?} --head {bookmark}");
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

    if no_pr {
        println!("{GREEN}{BOLD}pushed{RESET}");
        return;
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
        Ok(Some(pr)) => println!("{GREEN}{BOLD}pushed{RESET} (PR exists: {})", pr.url),
        Ok(None) => match gh::pr_create(&repo, title, body, &bookmark) {
            Ok(url) => println!("{GREEN}{BOLD}sent{RESET} ({url})"),
            Err(e) => {
                eprintln!("{RED}{BOLD}pr create failed:{RESET} {e}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("{YELLOW}{BOLD}warn:{RESET} could not check existing PRs: {e}");
            std::process::exit(1);
        }
    }
}

/// Pick the bookmark for the current change.
///
/// Prefers a bookmark already on `@` (set by `/start` or the user). If `@`
/// has no bookmark, derives one from the description. If `@` has multiple
/// bookmarks, warns and falls back to deriving — pass `--bookmark` to pick
/// one explicitly.
pub fn resolve_bookmark(description: &str) -> Result<String, String> {
    let bookmarks = jj::current_bookmarks().map_err(|e| e.to_string())?;
    match bookmarks.len() {
        1 => Ok(bookmarks.into_iter().next().unwrap()),
        0 => jj::derive_bookmark(description).ok_or_else(|| {
            "could not derive bookmark from description; pass --bookmark".to_string()
        }),
        _ => {
            eprintln!(
                "{YELLOW}{BOLD}warn:{RESET} multiple bookmarks on @ ({}); deriving from description — pass --bookmark to override",
                bookmarks.join(", ")
            );
            jj::derive_bookmark(description).ok_or_else(|| {
                "could not derive bookmark from description; pass --bookmark".to_string()
            })
        }
    }
}

/// Split a commit message into a PR title (first line) and body (the rest,
/// with leading blank lines stripped).
pub fn split_message(message: &str) -> (&str, &str) {
    match message.split_once('\n') {
        Some((title, rest)) => (title.trim(), rest.trim_start_matches('\n').trim_end()),
        None => (message.trim(), ""),
    }
}

#[cfg(test)]
mod tests {
    use super::split_message;

    #[test]
    fn split_title_only() {
        assert_eq!(split_message("feat: thing"), ("feat: thing", ""));
    }

    #[test]
    fn split_title_and_body() {
        let msg = "feat: thing\n\nlonger explanation\nspans lines\n";
        assert_eq!(
            split_message(msg),
            ("feat: thing", "longer explanation\nspans lines"),
        );
    }
}
