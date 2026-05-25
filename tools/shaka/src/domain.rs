mod inventory;
mod schema_check;

use std::path::PathBuf;

use clap::Subcommand;

const DEFAULT_REGISTRY_DIR: &str = "infra/cloudflare-dns/domains";

#[derive(Subcommand)]
pub enum DomainCommand {
    /// Diff a Hover snapshot against the per-domain registry
    #[command(after_help = inventory::REFRESH_SNIPPET)]
    Inventory {
        /// Path to a sanitized Hover snapshot JSON (see --help for the refresh procedure)
        #[arg(long, value_name = "FILE")]
        input: PathBuf,
        /// Directory of per-domain CUE registry files (one #Domain instance per file)
        #[arg(long, value_name = "DIR", default_value = DEFAULT_REGISTRY_DIR)]
        registry_dir: PathBuf,
    },
    /// Validate every per-domain CUE file in the registry against the #Domain schema
    #[command(name = "schema-check")]
    SchemaCheck {
        /// Directory of per-domain CUE registry files (one #Domain instance per file)
        #[arg(long, value_name = "DIR", default_value = DEFAULT_REGISTRY_DIR)]
        registry_dir: PathBuf,
    },
}

pub fn run(cmd: DomainCommand) {
    match cmd {
        DomainCommand::Inventory {
            input,
            registry_dir,
        } => inventory::run(&input, &registry_dir),
        DomainCommand::SchemaCheck { registry_dir } => schema_check::run(&registry_dir),
    }
}
