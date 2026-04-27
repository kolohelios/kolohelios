use clap::{Parser, Subcommand};

mod generate;
mod gh;
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
        Commands::Generate { check } => generate::run(check),
        Commands::Preflight { keep_going } => preflight::run(keep_going),
        Commands::Repo { command } => repo::run(command),
        Commands::Validate => validate::run(),
    }
}
