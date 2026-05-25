mod audit;
mod generate_justfiles;
mod new;
pub mod schema_check;

use clap::Subcommand;

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
        ProjectCommand::Audit => audit::run(),
        ProjectCommand::New { name, slot } => new::run(name, slot),
    }
}
