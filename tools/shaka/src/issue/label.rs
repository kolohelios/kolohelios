//! `shaka issue label sync` — reconciles GitHub's live label set
//! against the canonical declaration in `.shaka/labels.cue`.

use std::collections::HashMap;
use std::path::Path;

use crate::gh::{self, RemoteLabel};
use crate::issue::labels::{self, Label};
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub to_create: Vec<Label>,
    pub to_edit: Vec<EditDiff>,
    pub to_delete: Vec<RemoteLabel>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct EditDiff {
    pub name: String,
    pub remote_color: String,
    pub canonical_color: String,
    pub remote_description: String,
    pub canonical_description: String,
}

impl Plan {
    pub fn is_clean(&self) -> bool {
        self.to_create.is_empty() && self.to_edit.is_empty() && self.to_delete.is_empty()
    }
}

/// Compute the diff between the canonical label set and what GitHub
/// currently reports. Pure function — no side effects — so it stays
/// easy to unit-test without shelling out to `gh`.
pub fn compute_plan(canonical: &[Label], remote: &[RemoteLabel]) -> Plan {
    let canonical_by_name: HashMap<&str, &Label> =
        canonical.iter().map(|l| (l.name.as_str(), l)).collect();
    let remote_by_name: HashMap<&str, &RemoteLabel> =
        remote.iter().map(|l| (l.name.as_str(), l)).collect();

    let mut to_create: Vec<Label> = canonical
        .iter()
        .filter(|l| !remote_by_name.contains_key(l.name.as_str()))
        .cloned()
        .collect();
    to_create.sort_by(|a, b| a.name.cmp(&b.name));

    let mut to_edit: Vec<EditDiff> = canonical
        .iter()
        .filter_map(|c| {
            let r = remote_by_name.get(c.name.as_str())?;
            if r.color == c.color && r.description == c.description {
                None
            } else {
                Some(EditDiff {
                    name: c.name.clone(),
                    remote_color: r.color.clone(),
                    canonical_color: c.color.clone(),
                    remote_description: r.description.clone(),
                    canonical_description: c.description.clone(),
                })
            }
        })
        .collect();
    to_edit.sort_by(|a, b| a.name.cmp(&b.name));

    let mut to_delete: Vec<RemoteLabel> = remote
        .iter()
        .filter(|r| !canonical_by_name.contains_key(r.name.as_str()))
        .cloned()
        .collect();
    to_delete.sort_by(|a, b| a.name.cmp(&b.name));

    Plan {
        to_create,
        to_edit,
        to_delete,
    }
}

pub fn run(repo_arg: Option<String>, apply: bool, delete: bool) {
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

    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let canonical = match labels::load(&cwd) {
        Ok(set) => set.labels,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    let remote = match gh::list_labels(&repo) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    let plan = compute_plan(&canonical, &remote);
    print!("{}", render_plan(&repo, &plan, apply, delete));

    if !apply {
        if plan.is_clean() {
            std::process::exit(0);
        } else {
            std::process::exit(1);
        }
    }

    let mut errors = 0;
    for label in &plan.to_create {
        match gh::label_create(&repo, &label.name, &label.color, &label.description) {
            Ok(()) => println!("  {GREEN}+{RESET} created {}", label.name),
            Err(e) => {
                eprintln!("  {RED}!{RESET} create {}: {e}", label.name);
                errors += 1;
            }
        }
    }
    for diff in &plan.to_edit {
        match gh::label_edit(
            &repo,
            &diff.name,
            &diff.canonical_color,
            &diff.canonical_description,
        ) {
            Ok(()) => println!("  {GREEN}~{RESET} edited {}", diff.name),
            Err(e) => {
                eprintln!("  {RED}!{RESET} edit {}: {e}", diff.name);
                errors += 1;
            }
        }
    }
    if delete {
        for label in &plan.to_delete {
            match gh::label_delete(&repo, &label.name) {
                Ok(()) => println!("  {YELLOW}-{RESET} deleted {}", label.name),
                Err(e) => {
                    eprintln!("  {RED}!{RESET} delete {}: {e}", label.name);
                    errors += 1;
                }
            }
        }
    }

    if errors > 0 {
        eprintln!("\n{RED}{BOLD}{errors} operation(s) failed{RESET}");
        std::process::exit(1);
    }
}

fn render_plan(repo: &str, plan: &Plan, apply: bool, delete: bool) -> String {
    let mut out = String::new();
    let mode = match (apply, delete) {
        (true, true) => "apply (with deletes)",
        (true, false) => "apply (skip deletes)",
        (false, _) => "dry-run",
    };
    out.push_str(&format!("{BOLD}shaka issue label sync{RESET}\n"));
    out.push_str(&format!("{DIM}├── repo: {repo}  mode: {mode}{RESET}\n"));

    if plan.is_clean() {
        out.push_str(&format!(
            "{GREEN}└── clean — canonical and remote agree{RESET}\n"
        ));
        return out;
    }

    out.push_str(&format!(
        "├── {GREEN}create{RESET}: {}\n",
        plan.to_create.len()
    ));
    for l in &plan.to_create {
        out.push_str(&format!("│     {GREEN}+{RESET} {} #{}\n", l.name, l.color));
    }
    out.push_str(&format!(
        "├── {YELLOW}edit{RESET}: {}\n",
        plan.to_edit.len()
    ));
    for d in &plan.to_edit {
        if d.remote_color != d.canonical_color {
            out.push_str(&format!(
                "│     {YELLOW}~{RESET} {} color #{} → #{}\n",
                d.name, d.remote_color, d.canonical_color
            ));
        }
        if d.remote_description != d.canonical_description {
            out.push_str(&format!(
                "│     {YELLOW}~{RESET} {} description {:?} → {:?}\n",
                d.name, d.remote_description, d.canonical_description
            ));
        }
    }
    let delete_label = if delete || !apply {
        "delete"
    } else {
        "delete (skipped without --delete)"
    };
    out.push_str(&format!(
        "└── {RED}{}{RESET}: {}\n",
        delete_label,
        plan.to_delete.len()
    ));
    for l in &plan.to_delete {
        out.push_str(&format!("      {RED}-{RESET} {}\n", l.name));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(name: &str, color: &str, description: &str, scope: bool) -> Label {
        Label {
            name: name.to_string(),
            color: color.to_string(),
            description: description.to_string(),
            scope,
        }
    }

    fn remote(name: &str, color: &str, description: &str) -> RemoteLabel {
        RemoteLabel {
            name: name.to_string(),
            color: color.to_string(),
            description: description.to_string(),
        }
    }

    #[test]
    fn clean_when_canonical_and_remote_agree() {
        let canonical = vec![label("shaka", "5319e7", "shaka CLI", true)];
        let remote = vec![remote("shaka", "5319e7", "shaka CLI")];
        let plan = compute_plan(&canonical, &remote);
        assert!(plan.is_clean());
    }

    #[test]
    fn detects_missing_label_for_create() {
        let canonical = vec![label("new", "abcdef", "new", true)];
        let remote: Vec<RemoteLabel> = vec![];
        let plan = compute_plan(&canonical, &remote);
        assert_eq!(plan.to_create.len(), 1);
        assert_eq!(plan.to_create[0].name, "new");
        assert!(plan.to_edit.is_empty());
        assert!(plan.to_delete.is_empty());
    }

    #[test]
    fn detects_color_drift_for_edit() {
        let canonical = vec![label("shaka", "5319e7", "shaka CLI", true)];
        let remote = vec![remote("shaka", "000000", "shaka CLI")];
        let plan = compute_plan(&canonical, &remote);
        assert!(plan.to_create.is_empty());
        assert_eq!(plan.to_edit.len(), 1);
        assert_eq!(plan.to_edit[0].remote_color, "000000");
        assert_eq!(plan.to_edit[0].canonical_color, "5319e7");
        assert!(plan.to_delete.is_empty());
    }

    #[test]
    fn detects_description_drift_for_edit() {
        let canonical = vec![label("shaka", "5319e7", "shaka CLI", true)];
        let remote = vec![remote("shaka", "5319e7", "old description")];
        let plan = compute_plan(&canonical, &remote);
        assert_eq!(plan.to_edit.len(), 1);
        assert_eq!(plan.to_edit[0].remote_description, "old description");
        assert_eq!(plan.to_edit[0].canonical_description, "shaka CLI");
    }

    #[test]
    fn detects_extra_remote_label_for_delete() {
        let canonical: Vec<Label> = vec![];
        let remote = vec![remote("stale", "ffffff", "")];
        let plan = compute_plan(&canonical, &remote);
        assert!(plan.to_create.is_empty());
        assert!(plan.to_edit.is_empty());
        assert_eq!(plan.to_delete.len(), 1);
        assert_eq!(plan.to_delete[0].name, "stale");
    }

    #[test]
    fn sorts_diff_buckets_by_name() {
        let canonical = vec![
            label("zeta", "ffffff", "", false),
            label("alpha", "ffffff", "", false),
        ];
        let remote: Vec<RemoteLabel> = vec![];
        let plan = compute_plan(&canonical, &remote);
        assert_eq!(plan.to_create[0].name, "alpha");
        assert_eq!(plan.to_create[1].name, "zeta");
    }
}
