use std::path::PathBuf;
use std::process::ExitCode;

use aof::render;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "aof",
    version,
    about = "Areas of focus — model, sync with Todoist, render as a diagram"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate the areas-of-focus tree against its CUE schema
    Validate,
    /// Reconcile areas against Todoist projects and report drift
    Sync,
    /// Render an SVG diagram inline in the current terminal
    Render {
        /// Path to the SVG file to render
        #[arg(long)]
        from: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate => {
            unimplemented("validate");
            ExitCode::FAILURE
        }
        Command::Sync => {
            unimplemented("sync");
            ExitCode::FAILURE
        }
        Command::Render { from } => match run_render(&from) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("aof render: {e}");
                ExitCode::FAILURE
            }
        },
    }
}

fn run_render(from: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let svg = std::fs::read(from)?;
    render::render_svg(&svg)?;
    Ok(())
}

fn unimplemented(name: &str) {
    eprintln!("aof {name}: not yet implemented");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
