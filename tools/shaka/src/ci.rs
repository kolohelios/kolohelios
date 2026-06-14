use std::path::PathBuf;

use clap::Subcommand;

mod audit_workflows;
mod gate;
mod generate;
mod main_workflow;
mod mask_and_run;
mod parse_deploy_url;
mod push_worker_secrets;
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
    ///
    /// `--retry-on <text>` opts into bounded retries: when the command
    /// exits non-zero and its combined output contains `<text>`, the
    /// wrapper re-runs it (up to `--max-retries` times, waiting
    /// `--retry-delay` seconds between attempts). Used to absorb the
    /// first-deploy race where `cloudflare-deploy apply` attaches a
    /// custom domain before the parallel `wrangler deploy` has created
    /// the Worker script (#714). Without `--retry-on`, the wrapper execs
    /// the command directly as before — no behaviour change.
    #[command(name = "mask-and-run")]
    MaskAndRun {
        /// Path passed through to `op run --env-file=<path>`.
        #[arg(long)]
        env_file: PathBuf,
        /// Retry while the command's combined output contains this
        /// substring and it exited non-zero. Empty disables retries.
        #[arg(long, default_value = "")]
        retry_on: String,
        /// Maximum number of retries after the first attempt.
        #[arg(long, default_value_t = 0)]
        max_retries: u32,
        /// Seconds to wait between retry attempts.
        #[arg(long, default_value_t = 30)]
        retry_delay: u64,
        /// Command and args to run under `op run` (separated by `--`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        args: Vec<String>,
    },
    /// Extract a deployed Worker's public URL from captured
    /// `wrangler deploy` output and print it to stdout.
    ///
    /// Prefers the `*.workers.dev` URL; falls back to a
    /// `<host> (custom domain)` trigger line (emitting `https://<host>`)
    /// for Workers that disable workers.dev. Exits non-zero only when
    /// neither is present — a custom-domain-only deploy is valid, not a
    /// failure (#867). Used by the `cf-deploy` reusable workflow's
    /// post-deploy URL step.
    #[command(name = "parse-deploy-url")]
    ParseDeployUrl {
        /// File holding captured `wrangler deploy` stdout.
        log: PathBuf,
    },
    /// Push a project's declared Worker runtime secrets to Cloudflare.
    ///
    /// Reads the secret *names* from the project's `wrangler.secrets`
    /// (in `project.cue`) and the *values* from the ambient environment,
    /// then runs `wrangler secret put <NAME>` for each. Designed to run
    /// under `shaka ci mask-and-run --env-file=.env --`, which resolves
    /// each declared secret's `op://` reference into an env var (and
    /// registers it for log masking) first. `wrangler secret put` is
    /// idempotent, so re-running an unchanged deploy is a no-op. A
    /// project with no declared secrets is a clean no-op (#794).
    ///
    /// `--name` / `--env` mirror `wrangler deploy`'s flags so the secret
    /// targets the same Worker the deploy did — `--name` for a PR-preview
    /// script, `--env` for a named production environment.
    #[command(name = "push-worker-secrets")]
    PushWorkerSecrets {
        /// Directory holding the project's `project.cue`. Defaults to the
        /// current directory (the `cf-deploy` step's `working-directory`).
        #[arg(long, default_value = ".")]
        project_dir: PathBuf,
        /// Override the target Worker script name (PR-preview deploys).
        #[arg(long)]
        name: Option<String>,
        /// Select a named `[env.<name>]` Worker (production environment).
        #[arg(long)]
        environment: Option<String>,
    },
}

pub fn run(cmd: CiCommand) {
    match cmd {
        CiCommand::Gate { needs } => gate::run(needs),
        CiCommand::GenerateWorkflows { check } => generate::run(check),
        CiCommand::AuditWorkflows => audit_workflows::run(),
        CiCommand::MaskAndRun {
            env_file,
            retry_on,
            max_retries,
            retry_delay,
            args,
        } => mask_and_run::run(env_file, args, retry_on, max_retries, retry_delay),
        CiCommand::ParseDeployUrl { log } => parse_deploy_url::run(log),
        CiCommand::PushWorkerSecrets {
            project_dir,
            name,
            environment,
        } => push_worker_secrets::run(project_dir, name, environment),
    }
}
