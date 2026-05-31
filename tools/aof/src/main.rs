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
    /// Render the areas tree as an inline terminal diagram
    Render,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate => unimplemented("validate"),
        Command::Sync => unimplemented("sync"),
        Command::Render => unimplemented("render"),
    }
}

fn unimplemented(name: &str) {
    eprintln!("aof {name}: not yet implemented");
    std::process::exit(1);
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
