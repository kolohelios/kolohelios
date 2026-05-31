use std::path::PathBuf;
use std::process::ExitCode;

use aof::{render, todoist};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

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
    /// Fetch and print the current Todoist project list (sanity check
    /// for reconciliation; reconciliation itself lands in #610).
    Sync {
        /// Emit ndjson instead of the default human-readable table
        #[arg(long)]
        json: bool,
    },
    /// Render an SVG diagram inline in the current terminal
    Render {
        /// Path to the SVG file to render
        #[arg(long)]
        from: PathBuf,
    },
    #[command(hide = true)]
    Completions { shell: Shell },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate => {
            unimplemented("validate");
            ExitCode::FAILURE
        }
        Command::Sync { json } => match run_sync(json) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("aof sync: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Render { from } => match run_render(&from) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("aof render: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
            ExitCode::SUCCESS
        }
    }
}

fn run_render(from: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let svg = std::fs::read(from)?;
    render::render_svg(&svg)?;
    Ok(())
}

fn run_sync(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let projects = todoist::list_projects()?;
    let out = if json {
        todoist::render_ndjson(&projects)
    } else {
        todoist::render_table(&projects)
    };
    println!("{out}");
    Ok(())
}

fn unimplemented(name: &str) {
    eprintln!("aof {name}: not yet implemented");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
