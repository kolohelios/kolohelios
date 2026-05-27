//! `shaka repo rebase-wip` — bulk-rebase local WIP bookmarks onto
//! `main@origin`. After a sibling PR merges, every other open branch
//! needs the same operation; doing them by hand is mechanical and
//! error-prone (see #147).

use std::process::Command;

use crate::jj;
use crate::term::{BOLD, DIM, GREEN, RED, RESET};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Rebased without conflict; ready to push.
    Clean { from: String, to: String },
    /// Rebase landed conflicts in the working copy. Left for manual
    /// resolution; not pushed even with `--push`.
    Conflict { at: String },
    /// Bookmark already pointed at a commit reachable from `main@origin`
    /// (no rebase needed). Jj reports "Skipped rebase…".
    UpToDate { at: String },
}

#[derive(Debug)]
pub(crate) struct BookmarkOutcome {
    pub name: String,
    pub outcome: Outcome,
}

pub fn run(push: bool) {
    if let Err(e) = jj::fetch() {
        eprintln!("{RED}{BOLD}error:{RESET} jj git fetch: {e}");
        std::process::exit(1);
    }

    let candidates = match collect_candidates() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    if candidates.is_empty() {
        println!("{GREEN}{BOLD}rebase-wip{RESET} no local bookmarks ahead of main@origin");
        return;
    }

    let mut outcomes: Vec<BookmarkOutcome> = Vec::new();
    for (name, pre_commit) in &candidates {
        let outcome = rebase_one(name, pre_commit);
        outcomes.push(BookmarkOutcome {
            name: name.clone(),
            outcome,
        });
    }

    print_report(&outcomes);

    if push {
        push_clean(&outcomes);
    }

    let conflicts = outcomes
        .iter()
        .filter(|o| matches!(o.outcome, Outcome::Conflict { .. }))
        .count();
    if conflicts > 0 {
        std::process::exit(1);
    }
}

/// Local bookmarks on commits in `main@origin..` (i.e., ahead of main).
/// `main` itself sits at the base of that revset and is auto-excluded.
/// Returns `(bookmark_name, commit_id)` pairs.
fn collect_candidates() -> Result<Vec<(String, String)>, String> {
    let out = Command::new("jj")
        .args([
            "log",
            "-r",
            "main@origin..",
            "--no-graph",
            "-T",
            r#"bookmarks.map(|b| b.name() ++ "\t" ++ commit_id).join("\n") ++ "\n""#,
        ])
        .output()
        .map_err(|e| format!("failed to run jj: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(parse_candidates(&String::from_utf8_lossy(&out.stdout)))
}

/// Pure: parse the `<name>\t<commit_id>` lines emitted by the candidate
/// template. Empty / whitespace-only lines (commits with no bookmark)
/// are skipped. Dedupes — a bookmark on multiple commits in the revset
/// would otherwise repeat (shouldn't happen in practice, defensive).
pub(crate) fn parse_candidates(stdout: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let (Some(name), Some(commit_id)) = (parts.next(), parts.next()) else {
            continue;
        };
        let entry = (name.to_string(), commit_id.to_string());
        if !out.iter().any(|p| p == &entry) {
            out.push(entry);
        }
    }
    out
}

fn rebase_one(name: &str, pre_commit: &str) -> Outcome {
    let rebase_out = Command::new("jj")
        .args(["rebase", "-b", name, "-d", "main@origin"])
        .output();
    let rebase_out = match rebase_out {
        Ok(o) => o,
        Err(e) => {
            return Outcome::Conflict {
                at: format!("failed to spawn jj rebase: {e}"),
            };
        }
    };
    if !rebase_out.status.success() {
        let stderr = String::from_utf8_lossy(&rebase_out.stderr)
            .trim()
            .to_string();
        return Outcome::Conflict { at: stderr };
    }

    // Distinguish clean rebase from up-to-date no-op via commit-id
    // comparison, then check for conflicts (jj keeps conflicts as a
    // commit attribute rather than failing the rebase).
    let post_commit = match commit_id_of_bookmark(name) {
        Ok(id) => id,
        Err(e) => {
            return Outcome::Conflict {
                at: format!("could not resolve post-rebase commit: {e}"),
            };
        }
    };

    let has_conflict = bookmark_has_conflict(name).unwrap_or(false);
    classify(pre_commit, &post_commit, has_conflict)
}

/// Pure: turn pre/post commit ids and a conflict flag into an `Outcome`.
pub(crate) fn classify(pre: &str, post: &str, has_conflict: bool) -> Outcome {
    if has_conflict {
        return Outcome::Conflict {
            at: short(post).to_string(),
        };
    }
    if pre == post {
        return Outcome::UpToDate {
            at: short(post).to_string(),
        };
    }
    Outcome::Clean {
        from: short(pre).to_string(),
        to: short(post).to_string(),
    }
}

fn short(commit_id: &str) -> &str {
    let n = commit_id.len().min(12);
    &commit_id[..n]
}

fn commit_id_of_bookmark(name: &str) -> Result<String, String> {
    let out = Command::new("jj")
        .args(["log", "-r", name, "--no-graph", "-T", "commit_id ++ \"\n\""])
        .output()
        .map_err(|e| format!("jj log: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn bookmark_has_conflict(name: &str) -> Result<bool, String> {
    let out = Command::new("jj")
        .args([
            "log",
            "-r",
            name,
            "--no-graph",
            "-T",
            r#"self.conflict() ++ "\n""#,
        ])
        .output()
        .map_err(|e| format!("jj log: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let body = String::from_utf8_lossy(&out.stdout);
    Ok(body.trim().eq_ignore_ascii_case("true"))
}

fn print_report(outcomes: &[BookmarkOutcome]) {
    println!("{BOLD}shaka repo rebase-wip{RESET}");
    let total = outcomes.len();
    let last = total.saturating_sub(1);
    for (i, o) in outcomes.iter().enumerate() {
        let connector = if i == last { "└──" } else { "├──" };
        match &o.outcome {
            Outcome::Clean { from, to } => {
                println!(
                    "{connector} {GREEN}clean{RESET}   {} {DIM}{from} → {to}{RESET}",
                    o.name
                );
            }
            Outcome::UpToDate { at } => {
                println!(
                    "{connector} {DIM}up-to-date{RESET} {} {DIM}{at}{RESET}",
                    o.name
                );
            }
            Outcome::Conflict { at } => {
                println!(
                    "{connector} {RED}conflict{RESET} {} {DIM}{at}{RESET}",
                    o.name
                );
            }
        }
    }
    let clean = outcomes
        .iter()
        .filter(|o| matches!(o.outcome, Outcome::Clean { .. }))
        .count();
    let conflicts = outcomes
        .iter()
        .filter(|o| matches!(o.outcome, Outcome::Conflict { .. }))
        .count();
    let up_to_date = outcomes
        .iter()
        .filter(|o| matches!(o.outcome, Outcome::UpToDate { .. }))
        .count();
    println!(
        "\n{BOLD}summary{RESET}: {GREEN}{clean} clean{RESET}, \
         {RED}{conflicts} conflict{RESET}, \
         {DIM}{up_to_date} up-to-date{RESET}"
    );
}

fn push_clean(outcomes: &[BookmarkOutcome]) {
    let clean_names: Vec<&str> = outcomes
        .iter()
        .filter_map(|o| match &o.outcome {
            Outcome::Clean { .. } => Some(o.name.as_str()),
            _ => None,
        })
        .collect();
    if clean_names.is_empty() {
        return;
    }
    println!("\n{BOLD}pushing {} bookmark(s){RESET}", clean_names.len());
    for name in clean_names {
        match jj::push_bookmark(name) {
            Ok(()) => println!("  {GREEN}pushed{RESET} {name}"),
            Err(e) => {
                eprintln!("  {RED}push failed:{RESET} {name}: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_no_change_is_up_to_date() {
        let o = classify(
            "0123456789abcdef0000000000000000",
            "0123456789abcdef0000000000000000",
            false,
        );
        assert!(matches!(o, Outcome::UpToDate { .. }));
    }

    #[test]
    fn classify_distinct_commits_is_clean() {
        let o = classify(
            "0123456789abcdef0000000000000000",
            "fedcba98765432100000000000000000",
            false,
        );
        match o {
            Outcome::Clean { from, to } => {
                assert_eq!(from, "0123456789ab");
                assert_eq!(to, "fedcba987654");
            }
            other => panic!("expected Clean, got {other:?}"),
        }
    }

    #[test]
    fn classify_conflict_wins_over_distinct_commits() {
        let o = classify(
            "0123456789abcdef0000000000000000",
            "fedcba98765432100000000000000000",
            true,
        );
        assert!(matches!(o, Outcome::Conflict { .. }));
    }

    #[test]
    fn parse_candidates_extracts_tab_separated_pairs() {
        let stdout = "feat/foo\tabc123\nfeat/bar\tdef456\n";
        let pairs = parse_candidates(stdout);
        assert_eq!(
            pairs,
            vec![
                ("feat/foo".to_string(), "abc123".to_string()),
                ("feat/bar".to_string(), "def456".to_string()),
            ]
        );
    }

    #[test]
    fn parse_candidates_skips_empty_lines() {
        // A commit in main@origin.. with no bookmarks emits a blank line
        // from the template — skip it.
        let stdout = "\nfeat/foo\tabc123\n\n\nfeat/bar\tdef456\n";
        let pairs = parse_candidates(stdout);
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn parse_candidates_dedupes() {
        // Same bookmark reported twice — defensive, shouldn't happen but
        // we don't want to try rebasing the same bookmark twice.
        let stdout = "feat/foo\tabc123\nfeat/foo\tabc123\n";
        let pairs = parse_candidates(stdout);
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn parse_candidates_ignores_malformed_lines() {
        // Lines without a tab are skipped (could happen if jj's output
        // format changes; better to skip than crash).
        let stdout = "no-tab-here\nfeat/foo\tabc123\n";
        let pairs = parse_candidates(stdout);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "feat/foo");
    }
}
