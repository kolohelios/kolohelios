//! `shaka deploy` — turn `deploy:` blocks in `project.cue` into
//! committed Terraform under `infra/cloudflare-deploy/terraform/generated/`.
//!
//! Today only the `cloudflare-worker` target is implemented; future
//! targets (`fly`, `hetzner`, ...) extend the schema disjunction and
//! the emitter in `generate_tf.rs`.

pub mod generate_tf;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum DeployCommand {
    /// Generate Terraform for each project's `deploy:` block. Output
    /// lands under `infra/cloudflare-deploy/terraform/generated/`:
    /// `_zones.tf` aggregates one `data "cloudflare_zone"` per unique
    /// zone, and `<project>.tf` carries each app's attachment.
    GenerateTf {
        /// Fail on drift instead of writing. Used by `shaka preflight`
        /// to keep committed TF in lockstep with project.cue.
        #[arg(long)]
        check: bool,
    },
}

pub fn run(cmd: DeployCommand) {
    match cmd {
        DeployCommand::GenerateTf { check } => generate_tf::run(check),
    }
}
