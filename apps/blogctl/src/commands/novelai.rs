//! Explicit NovelAI operations alongside, not inside, the post stage pipeline.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use snafu::ResultExt;

use crate::error::{NovelaiCandidateNotSavedSnafu, NovelaiSnafu, Result, WritingSnafu};
use crate::novelai::{self, Client, PreparedGeneration};
use crate::writing::Store;

#[derive(Debug, Args)]
pub struct NovelaiArgs {
    #[command(subcommand)]
    pub action: NovelaiAction,
}

#[derive(Debug, Subcommand)]
pub enum NovelaiAction {
    /// Discover models with one authenticated GET, not a generation.
    Models {
        /// Machine-readable model IDs for invocation-boundary adapters.
        #[arg(long)]
        json: bool,
    },
    /// Generate one candidate; --dry-run previews locally without credentials.
    /// Supply NOVELAI_API_TOKEN through 1Password, never as an argument.
    Generate {
        experiment: String,
        #[arg(long, value_name = "PRIVATE_PATH")]
        workdir: PathBuf,
        #[arg(long)]
        variant: String,
        /// Explicit model ID from `novelai models`; no fallback or substitution.
        #[arg(long)]
        model: String,
        /// Preview the request without credentials, network access, or writes.
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(args: NovelaiArgs) -> Result<()> {
    match args.action {
        NovelaiAction::Models { json } => {
            let models = Client::from_env()
                .context(NovelaiSnafu)?
                .models()
                .context(NovelaiSnafu)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&models)
                        .map_err(|_| novelai::Error::InvalidRequest)
                        .context(NovelaiSnafu)?
                );
            } else {
                for model in models {
                    println!("{model}");
                }
            }
        }
        NovelaiAction::Generate {
            experiment,
            workdir,
            variant,
            model,
            dry_run,
        } => {
            let store = Store::open(&workdir).context(WritingSnafu)?;
            let snapshot = store
                .experiment(&experiment)
                .context(WritingSnafu)?
                .prepare(&variant)
                .context(WritingSnafu)?;
            let generation = PreparedGeneration::new(snapshot, model).context(NovelaiSnafu)?;
            if dry_run {
                println!("{}", generation.preview().context(NovelaiSnafu)?);
                return Ok(());
            }
            let client = Client::from_env().context(NovelaiSnafu)?;
            println!("{}", generate_candidate(&client, &store, generation)?);
        }
    }
    Ok(())
}

/// One explicit request, without a VCS hook, stage update, manuscript write,
/// model lookup, retry, or automatic follow-up.
pub(crate) fn generate_candidate(
    client: &Client,
    store: &Store,
    generation: PreparedGeneration,
) -> Result<String> {
    // Catch known path/permission problems before sending a generation. This
    // is a probe, not a guarantee against later disk failure or concurrent edits.
    store
        .check_candidate_storage(generation.snapshot())
        .context(WritingSnafu)?;
    let body = client.generate(&generation).context(NovelaiSnafu)?;
    store
        .record_candidate(generation.snapshot(), &generation.metadata(), &body)
        .context(NovelaiCandidateNotSavedSnafu)
}
