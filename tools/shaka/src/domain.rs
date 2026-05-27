mod check;
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
    /// Check live state of owned domains for drift (WHOIS expiry + lock, NS pair)
    Check {
        /// Restrict the check to a single domain by name
        #[arg(long, value_name = "DOMAIN")]
        domain: Option<String>,
        /// Override `expiry_warn_days` for every domain in this run.
        /// Without it, each `#Domain` uses its declared value (or the
        /// default 30).
        #[arg(long, value_name = "DAYS")]
        expiry_warn_days: Option<i64>,
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
        DomainCommand::Check {
            domain,
            expiry_warn_days,
            registry_dir,
        } => check::run(&registry_dir, domain.as_deref(), expiry_warn_days),
    }
}
