use super::{die, workspace_path, BOLD, DIM, GREEN, RESET};
use crate::jj;

pub fn run(name: &str) {
    let repo_root = match jj::repo_root() {
        Ok(p) => p,
        Err(e) => die(&e.to_string()),
    };

    let path = workspace_path(&repo_root, name);
    if path.exists() {
        die(&format!("path already exists: {}", path.display()));
    }

    let workspaces = match jj::workspaces() {
        Ok(w) => w,
        Err(e) => die(&e.to_string()),
    };
    if workspaces.iter().any(|w| w.name == name) {
        die(&format!("workspace already registered: {name}"));
    }

    if let Err(e) = jj::workspace_add(name, &path) {
        die(&e.to_string());
    }

    println!(
        "{GREEN}{BOLD}created{RESET} workspace {BOLD}{name}{RESET} at {DIM}{}{RESET}",
        path.display()
    );
}
