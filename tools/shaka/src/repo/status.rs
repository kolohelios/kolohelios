use serde::Serialize;

use crate::gh;
use crate::jj;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

#[derive(Serialize)]
struct Status {
    bookmarks: Vec<String>,
    change_id: String,
    parent: Parent,
    dirty: Dirty,
    ahead: usize,
    pr: Option<Pr>,
    last_fetch: Option<String>,
}

#[derive(Serialize)]
struct Parent {
    commit_id: String,
    is_main_origin: bool,
}

#[derive(Serialize)]
struct Dirty {
    added: usize,
    modified: usize,
    deleted: usize,
    total: usize,
}

#[derive(Serialize)]
struct Pr {
    number: u64,
    url: String,
    bookmark: String,
}

pub fn run(json: bool) {
    let status = match collect() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };
    if json {
        match serde_json::to_string_pretty(&status) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} failed to serialize status: {e}");
                std::process::exit(1);
            }
        }
    } else {
        render_human(&status);
    }
}

fn collect() -> Result<Status, String> {
    let bookmarks = jj::bookmarks_on("main@origin..@").map_err(|e| e.to_string())?;
    let change_id = jj::current_change_id().map_err(|e| e.to_string())?;
    let parent_id = jj::commit_id_of("@-").map_err(|e| e.to_string())?;
    let main_id = jj::commit_id_of("main@origin").map_err(|e| e.to_string())?;
    let parent = Parent {
        is_main_origin: parent_id == main_id,
        commit_id: parent_id,
    };
    let counts = jj::dirty_counts().map_err(|e| e.to_string())?;
    let dirty = Dirty {
        added: counts.added,
        modified: counts.modified,
        deleted: counts.deleted,
        total: counts.total(),
    };
    let ahead = jj::ahead_count("main@origin").map_err(|e| e.to_string())?;

    // PR lookup is best-effort: a missing `gh`, no remote, or a transient API
    // error shouldn't fail the whole command.
    let pr = bookmarks.first().and_then(|b| match gh::detect_repo() {
        Ok(repo) => gh::pr_for_head(&repo, b).ok().flatten().map(|info| Pr {
            number: info.number,
            url: info.url,
            bookmark: b.clone(),
        }),
        Err(_) => None,
    });

    let last_fetch = jj::last_fetch_time().map_err(|e| e.to_string())?;

    Ok(Status {
        bookmarks,
        change_id,
        parent,
        dirty,
        ahead,
        pr,
        last_fetch,
    })
}

fn render_human(s: &Status) {
    let bookmark_label = if s.bookmarks.is_empty() {
        format!("{DIM}none{RESET}")
    } else {
        s.bookmarks.join(", ")
    };
    println!(
        "{BOLD}bookmark:{RESET}     {bookmark_label} {DIM}({}){RESET}",
        s.change_id
    );

    let parent_short = short_id(&s.parent.commit_id);
    let parent_label = if s.parent.is_main_origin {
        format!("{GREEN}main@origin{RESET} {DIM}({parent_short}){RESET}")
    } else {
        format!("{YELLOW}{parent_short}{RESET} {DIM}(not main@origin){RESET}")
    };
    println!("{BOLD}parent:{RESET}       {parent_label}");

    let dirty_label = if s.dirty.total == 0 {
        format!("{DIM}clean{RESET}")
    } else {
        format!(
            "{YELLOW}{}+{RESET} {YELLOW}{}~{RESET} {YELLOW}{}-{RESET}",
            s.dirty.added, s.dirty.modified, s.dirty.deleted
        )
    };
    println!("{BOLD}working copy:{RESET} {dirty_label}");

    let ahead_label = match s.ahead {
        0 => format!("{DIM}0 commits past main@origin{RESET}"),
        1 => "1 commit past main@origin".to_string(),
        n => format!("{n} commits past main@origin"),
    };
    println!("{BOLD}ahead:{RESET}        {ahead_label}");

    match &s.pr {
        Some(p) => println!(
            "{BOLD}pr:{RESET}           #{} {DIM}{}{RESET}",
            p.number, p.url
        ),
        None => println!("{BOLD}pr:{RESET}           {DIM}none{RESET}"),
    }

    match &s.last_fetch {
        Some(t) => println!("{BOLD}last fetch:{RESET}   {t}"),
        None => println!("{BOLD}last fetch:{RESET}   {DIM}unknown{RESET}"),
    }
}

fn short_id(id: &str) -> &str {
    let n = id.len().min(12);
    &id[..n]
}
