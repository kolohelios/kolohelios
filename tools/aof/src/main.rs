// Non-test code must not `.unwrap()`; `not(test)` exempts unit tests,
// and integration tests compile as separate crates (no attribute).
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use aof::{diagram, render, todoist};
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
    /// Render the areas-of-focus tree as a diagram inline in the terminal
    Render {
        /// Directory holding the areas CUE package (schema + data).
        #[arg(long, default_value = "data")]
        data: PathBuf,
        /// Render this pre-rendered SVG directly, bypassing the
        /// cue/d2 pipeline. A debug escape hatch.
        #[arg(long, conflicts_with = "data")]
        from: Option<PathBuf>,
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
        Command::Render { data, from } => match run_render(&data, from.as_deref()) {
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

fn run_render(data: &Path, from: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let svg = match from {
        // Escape hatch: render a pre-built SVG without touching cue/d2.
        Some(path) => std::fs::read(path)?,
        // Default: load the areas tree, emit D2, render to SVG.
        None => {
            let tree = diagram::Tree::load(data)?;
            let source = diagram::tree_to_d2(&tree);
            diagram::d2_to_svg(&source)?
        }
    };
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
