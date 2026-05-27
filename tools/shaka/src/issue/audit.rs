use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde_json::Value;

use crate::gh;
use crate::issue::labels;
use crate::term::{BOLD, GREEN, RED, RESET};

pub fn run(repo_arg: Option<String>) {
    let repo = match repo_arg {
        Some(r) => r,
        None => match gh::detect_repo() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} {e}");
                eprintln!("Hint: pass --repo owner/repo explicitly");
                std::process::exit(1);
            }
        },
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let label_set = match labels::load(&cwd) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };
    let scope_labels: Vec<&str> = label_set.scope_names();

    println!("{BOLD}Auditing {repo}{RESET}");
    println!("Scope labels: {}\n", scope_labels.join(", "));

    let issues = match list_open_issues(&repo) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    let mut scope_violators: Vec<(u64, String)> = Vec::new();
    let mut missing_native_link: Vec<MissingLink> = Vec::new();
    let mut nonexistent_refs: Vec<NonexistentRef> = Vec::new();

    // Cache `gh::issue_exists` lookups so a parent referenced by N
    // children only costs one round trip.
    let mut existence_cache: BTreeMap<u64, bool> = BTreeMap::new();

    for issue in &issues {
        let number = issue["number"].as_u64().unwrap_or(0);
        let title = issue["title"].as_str().unwrap_or("").to_string();
        let body = issue["body"].as_str().unwrap_or("");

        let labels: Vec<&str> = issue["labels"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|l| l["name"].as_str()).collect())
            .unwrap_or_default();
        let has_scope = labels.iter().any(|l| scope_labels.contains(l));
        if !has_scope {
            scope_violators.push((number, title.clone()));
        }

        let refs = extract_parent_refs(body);
        if refs.is_empty() {
            continue;
        }

        // Only call /issues/{N} when there's a freeform ref to verify.
        let native_parent = match gh::issue_parent(&repo, number) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{RED}{BOLD}warn:{RESET} could not load #{number} parent: {e}");
                None
            }
        };

        for r in &refs {
            let exists = *existence_cache.entry(r.parent).or_insert_with(|| {
                gh::issue_exists(&repo, r.parent).unwrap_or_else(|e| {
                    eprintln!("{RED}{BOLD}warn:{RESET} could not check #{}: {e}", r.parent);
                    // Treat lookup failure as "assume exists" so we don't
                    // false-positive a noisy network blip into a violation.
                    true
                })
            });
            if !exists {
                nonexistent_refs.push(NonexistentRef {
                    child: number,
                    child_title: title.clone(),
                    parent: r.parent,
                    phrase: r.phrase.clone(),
                });
            }
        }

        let referenced_parents: BTreeSet<u64> = refs.iter().map(|r| r.parent).collect();
        if !referenced_parents.contains(&native_parent.unwrap_or(0)) {
            let phrase = refs.first().map(|r| r.phrase.clone()).unwrap_or_default();
            missing_native_link.push(MissingLink {
                child: number,
                child_title: title.clone(),
                referenced_parents: referenced_parents.into_iter().collect(),
                phrase,
            });
        }
    }

    let mut violations = 0;

    if !scope_violators.is_empty() {
        println!("{BOLD}Open issues missing a scope label:{RESET}");
        for (n, title) in &scope_violators {
            println!("  {RED}#{n}{RESET}  {title}");
        }
        println!();
        violations += scope_violators.len();
    }

    if !missing_native_link.is_empty() {
        println!("{BOLD}Issues with freeform parent reference but no native link:{RESET}");
        for v in &missing_native_link {
            let parents: Vec<String> = v
                .referenced_parents
                .iter()
                .map(|p| format!("#{p}"))
                .collect();
            println!(
                "  {RED}#{}{RESET}  {} {RED}→{RESET} {:?} (link to {})",
                v.child,
                v.child_title,
                v.phrase,
                parents.join(", ")
            );
        }
        println!();
        violations += missing_native_link.len();
    }

    if !nonexistent_refs.is_empty() {
        println!("{BOLD}Issues referencing nonexistent issues:{RESET}");
        for v in &nonexistent_refs {
            println!(
                "  {RED}#{}{RESET}  {} {RED}→{RESET} {:?} (#{} not found)",
                v.child, v.child_title, v.phrase, v.parent
            );
        }
        println!();
        violations += nonexistent_refs.len();
    }

    if violations == 0 {
        println!(
            "{GREEN}{BOLD}All {} open issue(s) pass audit{RESET}",
            issues.len()
        );
        return;
    }

    println!(
        "{RED}{BOLD}{} violation(s) across {} open issue(s){RESET}",
        violations,
        issues.len()
    );
    std::process::exit(1);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParentRef {
    pub(crate) parent: u64,
    /// The matched phrase (e.g. `"Sub-issue of #15"`), surfaced in
    /// audit output so the user can grep the body for it.
    pub(crate) phrase: String,
}

#[derive(Debug)]
struct MissingLink {
    child: u64,
    child_title: String,
    referenced_parents: Vec<u64>,
    phrase: String,
}

#[derive(Debug)]
struct NonexistentRef {
    child: u64,
    child_title: String,
    parent: u64,
    phrase: String,
}

/// Scan an issue body for freeform parent-pointer phrases. Returns one
/// `ParentRef` per match (so two phrases pointing to different parents
/// both surface).
///
/// Recognized phrases:
/// - `Sub-issue of #N`
/// - `Tracked in #N`
///
/// Both are case-sensitive and match the canonical form used in this
/// repo's existing issue bodies. PR-side close keywords (`Closes`,
/// `Fixes`, `Resolves`) and sibling relationships (`Blocked on`,
/// `Depends on`) are intentionally excluded — see issue #541 for
/// rationale.
pub(crate) fn extract_parent_refs(body: &str) -> Vec<ParentRef> {
    const PHRASES: &[&str] = &["Sub-issue of", "Tracked in"];
    let mut out: Vec<ParentRef> = Vec::new();
    let mut seen: BTreeSet<(u64, String)> = BTreeSet::new();

    for phrase in PHRASES {
        let needle = format!("{phrase} #");
        let mut search_from = 0usize;
        while let Some(idx) = body[search_from..].find(&needle) {
            let abs = search_from + idx + needle.len();
            // Read digits until non-digit.
            let digits: String = body[abs..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !digits.is_empty() {
                if let Ok(n) = digits.parse::<u64>() {
                    let phrase_str = format!("{phrase} #{digits}");
                    if seen.insert((n, phrase_str.clone())) {
                        out.push(ParentRef {
                            parent: n,
                            phrase: phrase_str,
                        });
                    }
                }
                search_from = abs + digits.len();
            } else {
                search_from = abs;
            }
        }
    }
    out
}

// `gh issue list` rather than the REST API directly, since the REST API
// mixes PRs into the issues endpoint while the gh CLI filters them out.
fn list_open_issues(repo: &str) -> Result<Vec<Value>, gh::GhError> {
    use snafu::ResultExt;

    let output = Command::new("gh")
        .args([
            "issue",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--limit",
            "1000",
            "--json",
            "number,title,labels,body",
        ])
        .output()
        .context(gh::SpawnSnafu)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return gh::GhCommandSnafu {
            command: "gh issue list".to_string(),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&body).context(gh::JsonParseSnafu {
        context: "failed to parse JSON from gh issue list".to_string(),
    })?;

    Ok(parsed.as_array().cloned().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_sub_issue_of_phrase() {
        let refs = extract_parent_refs("Sub-issue of #15. Closes the loophole.");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].parent, 15);
        assert_eq!(refs[0].phrase, "Sub-issue of #15");
    }

    #[test]
    fn extracts_tracked_in_phrase() {
        let refs = extract_parent_refs("Slice tracked in #209: `shaka issue audit`.");
        // "Tracked in #209" appears as " tracked in #209" — case-sensitive
        // pattern requires capital T at start-of-phrase. Confirm: lower-
        // case "tracked in" *does not* match (matches comment style only).
        assert!(refs.is_empty(), "lowercase 'tracked in' should not match");
    }

    #[test]
    fn extracts_capital_tracked_in() {
        let refs = extract_parent_refs("Tracked in #209: see context.");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].parent, 209);
    }

    #[test]
    fn extracts_multiple_phrases() {
        let refs = extract_parent_refs("Sub-issue of #15. Tracked in #209 for slicing.");
        assert_eq!(refs.len(), 2);
        let parents: Vec<u64> = refs.iter().map(|r| r.parent).collect();
        assert!(parents.contains(&15));
        assert!(parents.contains(&209));
    }

    #[test]
    fn dedupes_repeated_phrase_for_same_parent() {
        let refs = extract_parent_refs(
            "Sub-issue of #15. Reiterated: Sub-issue of #15 in next paragraph.",
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].parent, 15);
    }

    #[test]
    fn ignores_pr_close_keywords() {
        let refs = extract_parent_refs("Closes #100. Fixes #200. Resolves #300.");
        assert!(refs.is_empty());
    }

    #[test]
    fn ignores_blocked_on_and_depends_on() {
        let refs = extract_parent_refs("Blocked on #100. Depends on #200.");
        assert!(refs.is_empty());
    }

    #[test]
    fn handles_empty_body() {
        assert!(extract_parent_refs("").is_empty());
    }

    #[test]
    fn handles_phrase_without_number() {
        // "Sub-issue of #" followed by non-digit — no match.
        assert!(extract_parent_refs("Sub-issue of #notanumber").is_empty());
    }
}
