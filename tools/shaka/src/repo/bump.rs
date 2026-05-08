use crate::gh;
use crate::jj;
use crate::term::{BOLD, GREEN, RED, RESET, YELLOW};

/// Refresh an in-flight PR: fetch, rebase the stack onto `main@origin`,
/// push, and report the PR url.
///
/// Resolves the bookmark to refresh in this order:
///
/// 1. `--bookmark <name>` (explicit override).
/// 2. The unique bookmark in `main@origin..@` (inclusive of `@`). Errors
///    if zero or more than one bookmark is found and no `--bookmark` was
///    supplied — multi-bookmark stacks (stack-of-PRs) are out of scope.
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

    if dry_run {
        println!("would run: jj git fetch");
        println!("would run: jj rebase -b @ -d main@origin");
        println!("would run: jj git push --bookmark {bookmark}");
        println!("would print PR url for head {bookmark}");
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

    println!("{BOLD}pushing {bookmark}{RESET}");
    if let Err(e) = jj::push_bookmark(&bookmark) {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    let repo = match gh::detect_repo() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{YELLOW}{BOLD}warn:{RESET} could not detect repo: {e}");
            println!("{GREEN}{BOLD}bumped{RESET} (skipping PR lookup)");
            return;
        }
    };

    match gh::pr_for_head(&repo, &bookmark) {
        Ok(Some(pr)) => println!("{GREEN}{BOLD}bumped{RESET} ({})", pr.url),
        Ok(None) => println!("{GREEN}{BOLD}bumped{RESET} (no PR found for {bookmark})"),
        Err(e) => {
            eprintln!("{YELLOW}{BOLD}warn:{RESET} could not look up PR: {e}");
            println!("{GREEN}{BOLD}bumped{RESET}");
        }
    }
}

/// Find the unique bookmark in the in-flight stack (`main@origin..@`).
///
/// Returns an error message when zero or more than one bookmark is found.
/// Multi-bookmark stacks (stack-of-PRs) are out of scope; the caller can
/// pass `--bookmark` to disambiguate.
fn resolve_bookmark() -> Result<String, String> {
    let bookmarks = jj::bookmarks_on("main@origin..@").map_err(|e| e.to_string())?;
    match bookmarks.len() {
        1 => Ok(bookmarks.into_iter().next().unwrap()),
        0 => Err(
            "no bookmark found in main@origin..@; pass --bookmark or set one with `jj bookmark`"
                .to_string(),
        ),
        _ => Err(format!(
            "multiple bookmarks in main@origin..@ ({}); pass --bookmark to pick one",
            bookmarks.join(", ")
        )),
    }
}
