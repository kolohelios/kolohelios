mod schema_check;

use std::path::PathBuf;

use clap::Subcommand;

const DEFAULT_REGISTRY_DIR: &str = "infra/cloudflare-tokens/tokens";

#[derive(Subcommand)]
pub enum CloudflareCommand {
    /// Validate every per-token CUE file in the registry against the #Token schema
    #[command(name = "schema-check")]
    SchemaCheck {
        /// Directory of per-token CUE registry files (one #Token instance per file)
        #[arg(long, value_name = "DIR", default_value = DEFAULT_REGISTRY_DIR)]
        registry_dir: PathBuf,
    },
}

pub fn run(cmd: CloudflareCommand) {
    match cmd {
        CloudflareCommand::SchemaCheck { registry_dir } => schema_check::run(&registry_dir),
    }
}
