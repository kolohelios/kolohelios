use std::fmt;
use std::process::Command;

#[derive(Debug)]
pub struct JjError {
    pub message: String,
}

impl fmt::Display for JjError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<std::io::Error> for JjError {
    fn from(e: std::io::Error) -> Self {
        JjError {
            message: format!("failed to run jj: {e}"),
        }
    }
}

/// Run `jj` with the given args, returning stdout on success.
pub fn run(args: &[&str]) -> Result<String, JjError> {
    let output = Command::new("jj").args(args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(JjError {
            message: format!("jj {}: {}", args.join(" "), stderr.trim()),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run `jj` and stream stdout/stderr to the parent's stdio.
pub fn run_streaming(args: &[&str]) -> Result<(), JjError> {
    let status = Command::new("jj").args(args).status()?;
    if !status.success() {
        return Err(JjError {
            message: format!("jj {} exited with status {status}", args.join(" ")),
        });
    }
    Ok(())
}

pub fn fetch() -> Result<(), JjError> {
    run_streaming(&["git", "fetch"])
}

pub fn rebase_onto(dest: &str) -> Result<(), JjError> {
    run_streaming(&["rebase", "-d", dest])
}

/// Description of the current change (`@`).
pub fn current_description() -> Result<String, JjError> {
    run(&["log", "-r", "@", "-T", "description", "--no-graph"])
}

/// Resolve a single revset to its commit id. Errors if the revset matches
/// zero or more than one revision.
pub fn commit_id_of(revset: &str) -> Result<String, JjError> {
    let out = run(&[
        "log",
        "-r",
        revset,
        "-T",
        r#"commit_id ++ "\n""#,
        "--no-graph",
    ])?;
    let mut ids = out.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = ids.next().ok_or_else(|| JjError {
        message: format!("revset {revset:?} matched no revisions"),
    })?;
    if ids.next().is_some() {
        return Err(JjError {
            message: format!("revset {revset:?} matched multiple revisions"),
        });
    }
    Ok(first.to_string())
}

/// Count non-empty commits in `base..@`.
pub fn ahead_count(base: &str) -> Result<usize, JjError> {
    let revset = format!("{base}..@ ~ empty()");
    let out = run(&[
        "log",
        "-r",
        &revset,
        "-T",
        r#"commit_id ++ "\n""#,
        "--no-graph",
    ])?;
    Ok(out.lines().filter(|l| !l.trim().is_empty()).count())
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DirtyCounts {
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
}

impl DirtyCounts {
    pub fn total(&self) -> usize {
        self.added + self.modified + self.deleted
    }
}

/// Count files changed by `@` versus its parent.
pub fn dirty_counts() -> Result<DirtyCounts, JjError> {
    let out = run(&["diff", "--summary", "-r", "@"])?;
    Ok(parse_diff_summary(&out))
}

fn parse_diff_summary(out: &str) -> DirtyCounts {
    let mut counts = DirtyCounts::default();
    for line in out.lines() {
        match line.chars().next() {
            Some('A' | 'C') => counts.added += 1,
            Some('M' | 'R') => counts.modified += 1,
            Some('D') => counts.deleted += 1,
            _ => {}
        }
    }
    counts
}

/// Short change id of the current change (`@`).
pub fn current_change_id() -> Result<String, JjError> {
    let out = run(&[
        "log",
        "-r",
        "@",
        "-T",
        r#"change_id.short() ++ "\n""#,
        "--no-graph",
    ])?;
    Ok(out.trim().to_string())
}

/// Timestamp (RFC3339-ish) of the most recent `git fetch` operation, or
/// `None` if the op log doesn't contain one within the inspected window.
pub fn last_fetch_time() -> Result<Option<String>, JjError> {
    let out = run(&[
        "op",
        "log",
        "--no-graph",
        "-T",
        r#"separate(" | ", time.end().format("%+"), description.first_line()) ++ "\n""#,
        "--limit",
        "200",
    ])?;
    for line in out.lines() {
        if let Some((ts, desc)) = line.split_once(" | ") {
            if desc.starts_with("fetch from git remote") {
                return Ok(Some(ts.trim().to_string()));
            }
        }
    }
    Ok(None)
}

/// Local bookmarks pointing at any commit in the given revset, in revset
/// order (jj's default: youngest first).
pub fn bookmarks_on(revset: &str) -> Result<Vec<String>, JjError> {
    let out = run(&[
        "log",
        "-r",
        revset,
        "-T",
        r#"bookmarks.map(|b| b.name()).join("\n") ++ "\n""#,
        "--no-graph",
    ])?;
    let mut names = Vec::new();
    for line in out.lines() {
        let name = line.trim();
        if !name.is_empty() && !names.iter().any(|n: &String| n == name) {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// Local bookmarks pointing at `@`.
pub fn current_bookmarks() -> Result<Vec<String>, JjError> {
    bookmarks_on("@")
}

/// Move (or create) a bookmark to point at `@`.
pub fn set_bookmark(name: &str) -> Result<(), JjError> {
    run_streaming(&["bookmark", "set", name, "-r", "@"])
}

pub fn push_bookmark(name: &str) -> Result<(), JjError> {
    run_streaming(&["git", "push", "--allow-new", "--bookmark", name])
}

/// Best-effort slugify of a string into a bookmark-safe segment.
pub fn slugify(s: &str, max_len: usize) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in s.to_lowercase().chars() {
        if out.len() >= max_len {
            break;
        }
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Derive a bookmark name from a Conventional-Commits-style description.
///
/// `feat(shaka): add commit lint` → `feat/shaka-add-commit-lint`
/// `fix: typo in readme` → `fix/typo-in-readme`
/// `arbitrary text` → `change/arbitrary-text`
pub fn derive_bookmark(description: &str) -> Option<String> {
    const MAX_LEN: usize = 60;
    let first_line = description.lines().next()?.trim();
    if first_line.is_empty() {
        return None;
    }

    let (prefix, subject) = match first_line.find(": ") {
        Some(colon) => {
            let header = &first_line[..colon];
            let subject = first_line[colon + 2..].trim();
            let (typ, scope) = match (header.find('('), header.rfind(')')) {
                (Some(open), Some(close)) if close > open => {
                    (&header[..open], Some(&header[open + 1..close]))
                }
                _ => (header, None),
            };
            let typ = typ.trim();
            let prefix = match scope {
                Some(s) if !s.trim().is_empty() => format!("{}/{}", typ, s.trim()),
                _ => format!("{typ}/"),
            };
            (prefix, subject)
        }
        None => ("change/".to_string(), first_line),
    };

    let remaining = MAX_LEN.saturating_sub(prefix.len());
    let slug = slugify(subject, remaining.max(8));
    if slug.is_empty() {
        return None;
    }

    let bookmark = if prefix.ends_with('/') {
        format!("{prefix}{slug}")
    } else {
        format!("{prefix}-{slug}")
    };
    Some(bookmark)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Add Commit Lint", 50), "add-commit-lint");
    }

    #[test]
    fn slugify_collapses_punct() {
        assert_eq!(slugify("foo!! bar...baz", 50), "foo-bar-baz");
    }

    #[test]
    fn slugify_truncates_and_trims_dash() {
        assert_eq!(slugify("hello world there", 8), "hello-wo");
        assert_eq!(slugify("hello world there", 6), "hello");
    }

    #[test]
    fn derive_with_scope() {
        assert_eq!(
            derive_bookmark("feat(shaka): add commit lint").as_deref(),
            Some("feat/shaka-add-commit-lint"),
        );
    }

    #[test]
    fn derive_without_scope() {
        assert_eq!(
            derive_bookmark("fix: typo in readme").as_deref(),
            Some("fix/typo-in-readme"),
        );
    }

    #[test]
    fn derive_non_conventional() {
        assert_eq!(
            derive_bookmark("arbitrary text here").as_deref(),
            Some("change/arbitrary-text-here"),
        );
    }

    #[test]
    fn derive_empty_returns_none() {
        assert!(derive_bookmark("").is_none());
        assert!(derive_bookmark("   \n").is_none());
    }

    #[test]
    fn parse_diff_summary_empty() {
        assert_eq!(parse_diff_summary(""), DirtyCounts::default());
    }

    #[test]
    fn parse_diff_summary_mixed() {
        let out = "A new.rs\nM changed.rs\nM other.rs\nD gone.rs\nR moved.rs\nC copied.rs\n";
        let counts = parse_diff_summary(out);
        assert_eq!(counts.added, 2); // A + C
        assert_eq!(counts.modified, 3); // M + M + R
        assert_eq!(counts.deleted, 1);
        assert_eq!(counts.total(), 6);
    }

    #[test]
    fn parse_diff_summary_skips_blank_and_unknown() {
        let out = "\nA file\n? weird\n";
        let counts = parse_diff_summary(out);
        assert_eq!(counts.added, 1);
        assert_eq!(counts.modified, 0);
        assert_eq!(counts.deleted, 0);
    }
}
