use clap::{Parser, Subcommand};

mod gh;
mod preflight;
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
    /// Run every validation check CI runs (nix flake check, tofu validate, tofu plan)
    Preflight {
        /// Continue running checks after a failure and report all at the end
        #[arg(long)]
        keep_going: bool,
    },
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
        Commands::Preflight { keep_going } => preflight::run(keep_going),
        Commands::Repo { command } => repo::run(command),
    }
}
