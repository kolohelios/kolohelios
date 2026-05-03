mod inventory;

use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum DomainCommand {
    /// Diff a Hover snapshot against the per-domain registry
    #[command(after_help = inventory::REFRESH_SNIPPET)]
    Inventory {
        /// Path to a sanitized Hover snapshot JSON (see --help for the refresh procedure)
        #[arg(long, value_name = "FILE")]
        input: PathBuf,
        /// Directory of per-domain CUE registry files (one #Domain instance per file)
        #[arg(
            long,
            value_name = "DIR",
            default_value = "infra/cloudflare-dns/domains"
        )]
        registry_dir: PathBuf,
    },
}

pub fn run(cmd: DomainCommand) {
    match cmd {
        DomainCommand::Inventory {
            input,
            registry_dir,
        } => inventory::run(&input, &registry_dir),
    }
}
