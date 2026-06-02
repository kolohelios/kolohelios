use std::path::PathBuf;

use clap::Subcommand;

mod audit_workflows;
mod gate;
mod generate;
mod main_workflow;
mod mask_and_run;
mod worker_cleanup_workflow;
mod worker_deploy_workflow;
mod workflow;

#[derive(Subcommand)]
pub enum CiCommand {
    /// Assert every upstream job in a workflow's needs context succeeded or was skipped
    Gate {
        /// JSON object from `${{ toJson(needs) }}` in the workflow file
        #[arg(long)]
        needs: String,
    },
    /// Generate per-project workflow files under `.github/workflows/`
    /// from each project.cue's `ci:` block. `--check` mode mirrors
    /// `shaka project generate-justfiles --check`: compares generated
    /// output against the committed files and exits non-zero on drift.
    #[command(name = "generate-workflows")]
    GenerateWorkflows {
        /// Verify committed workflows match what would be generated; exit non-zero on drift
        #[arg(long)]
        check: bool,
    },
    /// Assert every file under `.github/workflows/` is either generated
    /// or in the hand-authored allowlist. Catches new workflows that
    /// bypass `generate-workflows`.
    #[command(name = "audit-workflows")]
    AuditWorkflows,
    /// Run a command under `op run`, registering each resolved-secret value
    /// with GitHub Actions log masking first (`::add-mask::<value>`).
    ///
    /// GH auto-masks the literal `secrets.*` values it injects, but not
    /// values `op run` resolves from them (CLOUDFLARE_API_TOKEN,
    /// AWS_SECRET_ACCESS_KEY, etc.). This wrapper enumerates the resolved
    /// env first, emits a mask command for each non-empty new/changed
    /// value, then execs `op run --env-file=<file> -- <args...>`.
    #[command(name = "mask-and-run")]
    MaskAndRun {
        /// Path passed through to `op run --env-file=<path>`.
        #[arg(long)]
        env_file: PathBuf,
        /// Command and args to run under `op run` (separated by `--`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        args: Vec<String>,
    },
}

pub fn run(cmd: CiCommand) {
    match cmd {
        CiCommand::Gate { needs } => gate::run(needs),
        CiCommand::GenerateWorkflows { check } => generate::run(check),
        CiCommand::AuditWorkflows => audit_workflows::run(),
        CiCommand::MaskAndRun { env_file, args } => mask_and_run::run(env_file, args),
    }
}
