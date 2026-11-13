mod renewal;
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
    /// Advance expiry declarations for opted-in tokens nearing expiration
    Renew {
        #[arg(long, default_value = DEFAULT_REGISTRY_DIR)]
        registry_dir: PathBuf,
        #[arg(long, default_value = "infra/cloudflare-tokens/terraform/main.tf")]
        terraform_file: PathBuf,
        /// RFC3339 reference time (for reproducible dry-runs)
        #[arg(long, requires = "dry_run")]
        now: Option<String>,
        #[arg(long)]
        dry_run: bool,
        /// Publish one renewal PR using the jj/repo-send workflow
        #[arg(long)]
        pr_bookmark: Option<String>,
        #[arg(long, requires = "pr_bookmark")]
        auto_merge: bool,
    },
    /// Verify renewed credentials from 1Password against the live Cloudflare API
    Verify {
        #[arg(long, default_value = DEFAULT_REGISTRY_DIR)]
        registry_dir: PathBuf,
    },
}

pub fn run(cmd: CloudflareCommand) {
    match cmd {
        CloudflareCommand::SchemaCheck { registry_dir } => schema_check::run(&registry_dir),
        CloudflareCommand::Renew {
            registry_dir,
            terraform_file,
            now,
            dry_run,
            pr_bookmark,
            auto_merge,
        } => {
            if let Err(error) = renewal::renew(renewal::RenewOptions {
                registry_dir,
                terraform_file,
                now,
                dry_run,
                pr_bookmark,
                auto_merge,
            }) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        CloudflareCommand::Verify { registry_dir } => {
            if let Err(error) = renewal::verify(&registry_dir) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
}
