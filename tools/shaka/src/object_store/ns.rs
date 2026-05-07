use std::path::Path;

use crate::object_store::registry;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

pub fn list() {
    let entries = match registry::collect(Path::new(".")) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("{RED}{BOLD}error:{RESET} {}", err);
            std::process::exit(1);
        }
    };

    if entries.is_empty() {
        println!("{YELLOW}no namespaces declared in any project.cue{RESET}");
        return;
    }

    println!("{BOLD}namespaces{RESET} ({} total)", entries.len());
    for e in &entries {
        println!(
            "  {BOLD}{}{RESET}  {DIM}{}{RESET}",
            e.namespace.prefix(),
            e.project.display()
        );
        println!("    {DIM}{}{RESET}", e.namespace.purpose);
    }
}

pub fn audit(_bucket: &str) {
    let entries = match registry::collect(Path::new(".")) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("{RED}{BOLD}error:{RESET} {}", err);
            std::process::exit(1);
        }
    };

    let errors = registry::validate_uniqueness(&entries);
    if errors.is_empty() {
        println!(
            "{GREEN}{BOLD}registry ok{RESET} ({} namespace{} declared)",
            entries.len(),
            if entries.len() == 1 { "" } else { "s" }
        );
        // Live-bucket audit lands in a follow-up commit; for now, registry-
        // only checks are enough to gate against duplicate or colliding
        // declarations across projects.
        return;
    }

    eprintln!("{RED}{BOLD}registry errors:{RESET}");
    for e in &errors {
        eprintln!("  {RED}-{RESET} {e}");
    }
    std::process::exit(1);
}
