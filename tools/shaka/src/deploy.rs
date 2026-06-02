//! `shaka deploy` — Cloudflare Worker deploy plumbing:
//!
//! - `generate-tf` turns `deploy:` blocks in `project.cue` into
//!   committed Terraform under
//!   `infra/cloudflare-deploy/terraform/generated/`. Today only the
//!   `cloudflare-worker` target is implemented; future targets (`fly`,
//!   `hetzner`, ...) extend the schema disjunction and the emitter in
//!   `generate_tf.rs`.
//! - `sweep-previews` reaps orphaned per-PR preview Workers (see
//!   `sweep_previews.rs`).

pub mod generate_tf;
pub mod sweep_previews;

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
    /// Delete orphaned Cloudflare preview Workers — those named
    /// `<previewScriptPrefix>-pr-<N>` whose PR has closed or merged.
    /// Backstops the per-PR `<name>-cleanup.yml` job. Reads
    /// `CLOUDFLARE_API_TOKEN`/`CLOUDFLARE_ACCOUNT_ID` from the env, so
    /// wrap it in `op run --env-file=.env -- ...`.
    SweepPreviews {
        /// Report what would be deleted without deleting anything.
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(cmd: DeployCommand) {
    match cmd {
        DeployCommand::GenerateTf { check } => generate_tf::run(check),
        DeployCommand::SweepPreviews { dry_run } => sweep_previews::run(dry_run),
    }
}
