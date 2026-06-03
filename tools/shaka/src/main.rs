use std::io;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

mod ci;
mod claude;
mod commit;
mod deploy;
mod domain;
mod gh;
mod issue;
mod jj;
mod object_store;
mod output;
mod preflight;
mod project;
mod repo;
mod term;
mod terraform;
mod token;
mod whitespace;
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
    /// Hooks invoked by the Claude Code harness via `.claude/settings.json`
    Claude {
        #[command(subcommand)]
        command: claude::ClaudeCommand,
    },
    /// Commit message tooling (lint, etc.)
    Commit {
        #[command(subcommand)]
        command: commit::CommitCommand,
    },
    #[command(hide = true)]
    Completions { shell: Shell },
    /// Generate per-project deploy Terraform from `deploy:` blocks in
    /// each project.cue (Worker custom domains, future targets)
    Deploy {
        #[command(subcommand)]
        command: deploy::DeployCommand,
    },
    /// Domain inventory and drift tooling
    Domain {
        #[command(subcommand)]
        command: domain::DomainCommand,
    },
    /// GitHub issue tooling (audit scope-label policy, etc.)
    Issue {
        #[command(subcommand)]
        command: issue::IssueCommand,
    },
    /// Manage the shared kolohelios Linode Object Storage bucket and its
    /// namespace registry (Terraform remote state, future caches/assets)
    #[command(name = "object-store")]
    ObjectStore {
        #[command(subcommand)]
        command: object_store::ObjectStoreCommand,
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
    /// Generate Terraform HCL from typed CUE definitions
    Terraform {
        #[command(subcommand)]
        command: terraform::TerraformCommand,
    },
    /// API token tooling, scoped per provider (cloudflare, ...)
    Token {
        #[command(subcommand)]
        command: token::TokenCommand,
    },
    /// Whitespace and line-ending hygiene (check, fix)
    Whitespace {
        #[command(subcommand)]
        command: whitespace::WhitespaceCommand,
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
        Commands::Claude { command } => claude::run(command),
        Commands::Commit { command } => commit::run(command),
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut io::stdout());
        }
        Commands::Deploy { command } => deploy::run(command),
        Commands::Domain { command } => domain::run(command),
        Commands::Issue { command } => issue::run(command),
        Commands::ObjectStore { command } => object_store::run(command),
        Commands::Preflight { keep_going, since } => preflight::run(keep_going, since),
        Commands::Project { command } => project::run(command),
        Commands::Repo { command } => repo::run(command),
        Commands::Terraform { command } => terraform::run(command),
        Commands::Token { command } => token::run(command),
        Commands::Whitespace { command } => whitespace::run(command),
        Commands::Workspace { command } => workspace::run(command),
    }
}
