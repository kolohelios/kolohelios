use clap::{Parser, Subcommand};

mod ci;
mod commit;
mod generate;
mod gh;
mod jj;
mod preflight;
mod repo;
mod validate;

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
    /// Generate justfiles from each project.cue (root + per-project)
    Generate {
        /// Compare generated content to disk and fail on any drift instead of writing
        #[arg(long)]
        check: bool,
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
    /// Repository management
    Repo {
        #[command(subcommand)]
        command: repo::RepoCommand,
    },
    /// Validate every project's project.cue against the shared schema
    Validate,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build => {
            println!("build");
        }
        Commands::Ci { command } => ci::run(command),
        Commands::Commit { command } => commit::run(command),
        Commands::Generate { check } => generate::run(check),
        Commands::Preflight { keep_going, since } => preflight::run(keep_going, since),
        Commands::Repo { command } => repo::run(command),
        Commands::Validate => validate::run(),
    }
}
