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

/// Local bookmarks pointing at `@`.
pub fn current_bookmarks() -> Result<Vec<String>, JjError> {
    let out = run(&[
        "log",
        "-r",
        "@",
        "-T",
        r#"bookmarks.map(|b| b.name()).join("\n")"#,
        "--no-graph",
    ])?;
    Ok(out
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
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
}
