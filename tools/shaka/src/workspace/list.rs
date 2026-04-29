use super::{die, workspace_path, BOLD, DIM, RESET};
use crate::jj;

pub fn run() {
    let repo_root = match jj::repo_root() {
        Ok(p) => p,
        Err(e) => die(&e.to_string()),
    };
    let workspaces = match jj::workspaces() {
        Ok(w) => w,
        Err(e) => die(&e.to_string()),
    };

    for ws in workspaces {
        let path = if ws.name == "default" {
            repo_root.clone()
        } else {
            workspace_path(&repo_root, &ws.name)
        };
        let desc = if ws.description_first_line.is_empty() {
            format!("{DIM}(no description){RESET}")
        } else {
            ws.description_first_line.clone()
        };
        println!("{BOLD}{}{RESET} {DIM}{}{RESET}", ws.name, path.display());
        println!("  {DIM}{}{RESET}  {desc}", ws.change_id);
    }
}
