//! Persisted workspace → issue mapping.
//!
//! When `shaka workspace new --issue N` runs, we record the issue number under
//! `<repo_root>/.shaka/workspaces/<name>.json`. `cleanup` reads this back to
//! find the merged PR that closed the issue, even after `repo sync` has
//! deleted the local bookmark (which makes the prior bookmark-based detection
//! miss). The directory is gitignored.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct IssueLink {
    pub issue: u64,
}

/// Directory that holds per-workspace link files.
fn dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".shaka").join("workspaces")
}

/// Path to the link file for a given workspace name.
pub fn path(repo_root: &Path, name: &str) -> PathBuf {
    dir(repo_root).join(format!("{name}.json"))
}

/// Persist `{ "issue": N }` for the given workspace. Creates parent dirs.
pub fn write(repo_root: &Path, name: &str, link: &IssueLink) -> io::Result<()> {
    let dir = dir(repo_root);
    fs::create_dir_all(&dir)?;
    let body = serde_json::to_string_pretty(link).map_err(io::Error::other)?;
    fs::write(path(repo_root, name), body)
}

/// Read the link for a workspace. Returns `Ok(None)` if the file is absent —
/// pre-existing workspaces from before this fix won't have one.
pub fn read(repo_root: &Path, name: &str) -> io::Result<Option<IssueLink>> {
    let p = path(repo_root, name);
    match fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Remove the link file if present. Missing file is not an error.
pub fn remove(repo_root: &Path, name: &str) -> io::Result<()> {
    match fs::remove_file(path(repo_root, name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn round_trip() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "i42", &IssueLink { issue: 42 }).unwrap();
        let got = read(dir.path(), "i42").unwrap();
        assert_eq!(got, Some(IssueLink { issue: 42 }));
    }

    #[test]
    fn read_missing_returns_none() {
        let dir = TempDir::new().unwrap();
        assert_eq!(read(dir.path(), "nope").unwrap(), None);
    }

    #[test]
    fn remove_missing_is_ok() {
        let dir = TempDir::new().unwrap();
        remove(dir.path(), "nope").unwrap();
    }

    #[test]
    fn remove_clears_file() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "i1", &IssueLink { issue: 1 }).unwrap();
        remove(dir.path(), "i1").unwrap();
        assert_eq!(read(dir.path(), "i1").unwrap(), None);
    }
}
