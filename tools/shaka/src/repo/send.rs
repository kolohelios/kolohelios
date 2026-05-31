use crate::gh;
use crate::jj;
use crate::preflight;
use crate::repo::describe;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

pub fn run(bookmark_arg: Option<String>, no_pr: bool, no_auto_merge: bool, dry_run: bool) {
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
        println!("would run: jj git fetch");
        println!("would run: jj rebase -b @ -d main@origin");
        println!(
            "would run: shaka preflight --since main@origin {DIM}(only if rebase moved @){RESET}"
        );
        println!("would run: jj bookmark set {bookmark} -r @");
        println!("would run: jj git push --allow-new --bookmark {bookmark}");
        if !no_pr {
            println!("would run: gh pr create --title {title:?} --head {bookmark}");
            if !no_auto_merge {
                println!("would run: gh pr merge --auto --rebase <pr-url>");
            }
        }
        return;
    }

    println!("{BOLD}fetching from origin{RESET}");
    if let Err(e) = jj::fetch() {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    let before = match jj::commit_id_of("@") {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    println!("{BOLD}rebasing onto main@origin{RESET}");
    if let Err(e) = jj::rebase_branch_onto("@", "main@origin") {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }

    let after = match jj::commit_id_of("@") {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    if before != after {
        println!(
            "{BOLD}main@origin advanced — re-running preflight{RESET} {DIM}(rebase pulled in new ancestors){RESET}"
        );
        preflight::run(false, Some("main@origin".to_string()));
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

    let pr_url = match gh::pr_for_head(&repo, &bookmark) {
        Ok(Some(pr)) => {
            println!("{GREEN}{BOLD}pushed{RESET} (PR exists: {})", pr.url);
            pr.url
        }
        Ok(None) => match gh::pr_create(&repo, title, body, &bookmark) {
            Ok(url) => {
                println!("{GREEN}{BOLD}sent{RESET} ({url})");
                url
            }
            Err(e) => {
                eprintln!("{RED}{BOLD}pr create failed:{RESET} {e}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("{YELLOW}{BOLD}warn:{RESET} could not check existing PRs: {e}");
            std::process::exit(1);
        }
    };

    if !no_auto_merge {
        queue_auto_merge(&pr_url);
    }
}

/// Queue GitHub auto-merge (rebase strategy) on the PR. Soft-fails:
/// the PR is already open, so a missing repo policy (e.g.
/// `allow_auto_merge: false`) should warn rather than error out.
fn queue_auto_merge(pr_url: &str) {
    match gh::pr_merge_auto_rebase(pr_url) {
        Ok(()) => println!("{DIM}auto-merge queued{RESET}"),
        Err(e) => {
            eprintln!("{YELLOW}{BOLD}warn:{RESET} could not queue auto-merge for {pr_url}: {e}")
        }
    }
}

/// Pick the bookmark for the current change.
///
/// Prefers a bookmark already on `@` (set by `/start` or the user). If `@`
/// has no bookmark, derives one from the description. If `@` has multiple
/// bookmarks, errors — the user has expressed intent by creating bookmarks,
/// so silently picking one (or deriving a third) would be wrong; pass
/// `--bookmark` to disambiguate.
pub fn resolve_bookmark(description: &str) -> Result<String, String> {
    let bookmarks = jj::current_bookmarks().map_err(|e| e.to_string())?;
    match bookmarks.len() {
        1 => Ok(bookmarks.into_iter().next().unwrap()),
        0 => jj::derive_bookmark(description).ok_or_else(|| {
            "could not derive bookmark from description; pass --bookmark".to_string()
        }),
        _ => Err(format!(
            "multiple bookmarks on @ ({}); pass --bookmark to pick one",
            bookmarks.join(", ")
        )),
    }
}
