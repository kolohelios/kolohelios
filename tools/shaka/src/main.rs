use clap::{Parser, Subcommand};

mod ci;
mod commit;
mod gh;
mod jj;
mod preflight;
mod project;
mod repo;
mod workspace;

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
    /// CI orchestration helpers (gate, etc.)
    Ci {
        #[command(subcommand)]
        command: ci::CiCommand,
    },
    /// Commit message tooling (lint, etc.)
    Commit {
        #[command(subcommand)]
        command: commit::CommitCommand,
    },
    /// Run every validation check CI runs (nix flake check, tofu validate, tofu plan)
    Preflight {
        /// Continue running checks after a failure and report all at the end
        #[arg(long)]
        keep_going: bool,
        /// Skip checks whose path scope does not intersect changes since this git ref
        #[arg(long, value_name = "REF")]
        since: Option<String>,
    },
    /// Project tooling (schema validation, justfile generation, etc.)
    Project {
        #[command(subcommand)]
        command: project::ProjectCommand,
    },
    /// Repository management
    Repo {
        #[command(subcommand)]
        command: repo::RepoCommand,
    },
    /// jj workspace management (sibling working copies)
    Workspace {
        #[command(subcommand)]
        command: workspace::WorkspaceCommand,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build => {
            println!("build");
        }
        Commands::Ci { command } => ci::run(command),
        Commands::Commit { command } => commit::run(command),
        Commands::Preflight { keep_going, since } => preflight::run(keep_going, since),
        Commands::Project { command } => project::run(command),
        Commands::Repo { command } => repo::run(command),
        Commands::Workspace { command } => workspace::run(command),
    }
}
