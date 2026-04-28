mod generate_justfiles;
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
}

pub fn run(cmd: ProjectCommand) {
    match cmd {
        ProjectCommand::SchemaCheck => schema_check::run(),
        ProjectCommand::GenerateJustfiles { check } => generate_justfiles::run(check),
    }
}
