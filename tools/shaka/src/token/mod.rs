mod cloudflare;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum TokenCommand {
    /// Cloudflare API token tooling (schema-check, etc.)
    Cloudflare {
        #[command(subcommand)]
        command: cloudflare::CloudflareCommand,
    },
}

pub fn run(cmd: TokenCommand) {
    match cmd {
        TokenCommand::Cloudflare { command } => cloudflare::run(command),
    }
}
