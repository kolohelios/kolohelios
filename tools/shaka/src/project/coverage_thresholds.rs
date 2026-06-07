//! `shaka project coverage-thresholds` — emit the cwd project's
//! `project.cue` as JSON so the generated `coverage` recipe can read its
//! gate without referencing a kolohelios source-tree path.
//!
//! The generated justfile used to run
//! `cue export ../../tools/shaka/schema/project-schema.cue project.cue`,
//! which only resolves inside a kolohelios checkout. Routing through
//! `shaka` lets the schema come from `$SHAKA_CUE_MODULE_DIR` (a packaged
//! binary) or the in-tree copy (the dev wrapper) via the same resolution
//! the rest of `shaka` uses, so the recipe works in any external repo
//! that has `shaka` on PATH.
//!
//! Output is the full project JSON (identical to what `cue export`
//! produced), so consumers keep reading `.coverage.line.fail` and
//! `.coverage // "absent"` exactly as before.

use std::path::Path;

use crate::term::{BOLD, RED, RESET};

use super::schema_check::cue_project;

/// Export the `project.cue` in the current working directory to JSON and
/// print it to stdout. Exits non-zero with a diagnostic on any failure
/// so the calling recipe fails loudly rather than feeding empty input to
/// `jq`.
pub fn run() {
    let project_file = Path::new("project.cue");
    if !project_file.is_file() {
        eprintln!(
            "{RED}{BOLD}error:{RESET} no project.cue in {}",
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".into())
        );
        std::process::exit(1);
    }

    let output = match cue_project(&["export"], project_file) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} failed to run cue export: {e}");
            std::process::exit(1);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{RED}{BOLD}error:{RESET} cue export failed:\n{stderr}");
        std::process::exit(1);
    }

    use std::io::Write;
    let mut stdout = std::io::stdout();
    if stdout.write_all(&output.stdout).is_err() {
        std::process::exit(1);
    }
}
