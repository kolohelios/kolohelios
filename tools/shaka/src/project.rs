mod audit;
mod coverage_thresholds;
mod generate_flakes;
mod generate_justfiles;
mod new;
pub mod schema_check;

use clap::Subcommand;

/// Top-level directories that may contain projects. Every discovery
/// pass (`schema_check::discover`, `generate_justfiles`,
/// `generate_flakes` via `schema_check`) walks these. Keep in sync
/// with the slot table in `CLAUDE.md`. The `project new` scaffold
/// allowlist is deliberately narrower — it lives in `new.rs`.
pub const SLOTS: &[&str] = &[
    "apps", "infra", "nix", "packages", "projects", "services", "tools",
];

#[derive(Subcommand)]
pub enum ProjectCommand {
    /// Validate every project.cue against the shared CUE schema
    SchemaCheck,
    /// Generate justfiles from each project.cue (root + per-project)
    GenerateJustfiles {
        /// Compare generated content to disk and fail on any drift instead of writing
        #[arg(long)]
        check: bool,
    },
    /// Generate per-project flake.nix from each project.cue's kind block
    /// (rust-cli, rust-lib, rust-worker, wasm-app, document, and opt-in
    /// infra/nix-lib; kinds without a flake template keep their
    /// hand-authored flake)
    GenerateFlakes {
        /// Compare generated content to disk and fail on any drift instead of writing
        #[arg(long)]
        check: bool,
    },
    /// Export the cwd project's `project.cue` as JSON (used by the
    /// generated `coverage` recipe to read its gate without a
    /// kolohelios source-tree path)
    CoverageThresholds,
    /// Audit every discovered project for structural conformance
    Audit,
    /// Scaffold a new project (currently rust-only)
    New {
        /// Project name (must match `^[a-z][a-z0-9-]*$`)
        #[arg(long)]
        name: String,
        /// Slot to scaffold under (apps, packages, projects, tools)
        #[arg(long)]
        slot: String,
    },
}

pub fn run(cmd: ProjectCommand) {
    match cmd {
        ProjectCommand::SchemaCheck => schema_check::run(),
        ProjectCommand::GenerateJustfiles { check } => generate_justfiles::run(check),
        ProjectCommand::GenerateFlakes { check } => generate_flakes::run(check),
        ProjectCommand::CoverageThresholds => coverage_thresholds::run(),
        ProjectCommand::Audit => audit::run(),
        ProjectCommand::New { name, slot } => new::run(name, slot),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_include_projects() {
        // `project new --slot projects` succeeds, so discovery must
        // see the slot too. Pre-#628 the lists drifted apart and a
        // project scaffolded there was invisible to schema-check /
        // generate-justfiles / generate-flakes / audit.
        assert!(SLOTS.contains(&"projects"));
    }

    #[test]
    fn slots_cover_every_directory_in_claude_md_slot_table() {
        // Keep this in sync with the slot table in `CLAUDE.md`. If a
        // new slot is added there, the discovery list has to learn
        // about it or projects under it become orphans.
        for slot in [
            "apps", "infra", "nix", "packages", "projects", "services", "tools",
        ] {
            assert!(
                SLOTS.contains(&slot),
                "discovery slot list is missing {slot:?}"
            );
        }
    }
}
