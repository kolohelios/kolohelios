use clap::{Parser, Subcommand};

mod gh;
mod repo;

#[derive(Parser)]
#[command(name = "shaka", about = "Build tooling for kolohelios")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build the project
    Build,
    /// Repository management
    Repo {
        #[command(subcommand)]
        command: repo::RepoCommand,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build => {
            println!("build");
        }
        Commands::Repo { command } => repo::run(command),
    }
}
