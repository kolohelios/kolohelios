use std::fs;

use super::issue_link;
use super::{die, workspace_path, BOLD, DIM, GREEN, RESET};
use crate::jj;

pub fn run(name: &str, force: bool) {
    if name == "default" {
        die("cannot forget the default workspace");
    }

    let repo_root = match jj::repo_root() {
        Ok(p) => p,
        Err(e) => die(&e.to_string()),
    };

    let workspaces = match jj::workspaces() {
        Ok(w) => w,
        Err(e) => die(&e.to_string()),
    };
    if !workspaces.iter().any(|w| w.name == name) {
        die(&format!("no such workspace: {name}"));
    }

    if !force {
        let dirty = match jj::dirty_counts_for(name) {
            Ok(d) => d,
            Err(e) => die(&e.to_string()),
        };
        if dirty.total() > 0 {
            die(&format!(
                "workspace {name} has {} uncommitted file change(s); pass --force to override",
                dirty.total()
            ));
        }
        let ahead = match jj::ahead_count_for(name, "main@origin") {
            Ok(n) => n,
            Err(e) => die(&format!(
                "could not check commits ahead of main@origin (pass --force to skip the safety check): {e}"
            )),
        };
        if ahead > 0 {
            die(&format!(
                "workspace {name} has {ahead} commit(s) ahead of main@origin; pass --force to override"
            ));
        }
    }

    if let Err(e) = jj::workspace_forget(name) {
        die(&e.to_string());
    }

    let path = workspace_path(&repo_root, name);
    if path.exists() {
        if let Err(e) = fs::remove_dir_all(&path) {
            die(&format!(
                "removed jj workspace but failed to remove directory {}: {e}",
                path.display()
            ));
        }
    }

    if let Err(e) = issue_link::remove(&repo_root, name) {
        eprintln!("{DIM}warn:{RESET} failed to remove issue link for {name}: {e}");
    }

    println!("{GREEN}{BOLD}forgot{RESET} workspace {BOLD}{name}{RESET}");
}
