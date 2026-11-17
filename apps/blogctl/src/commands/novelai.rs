//! Explicit NovelAI operations alongside, not inside, the post stage pipeline.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use snafu::ResultExt;

use crate::error::{NovelaiCandidateNotSavedSnafu, NovelaiSnafu, Result, WritingSnafu};
use crate::novelai::{self, Client, PreparedGeneration};
use crate::writing::{ArtifactReference, CandidateContext, Store};

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
    /// Decode a captured response locally; no credentials, request, or candidate write.
    DecodeResponse {
        record: String,
        #[arg(long, value_name = "PRIVATE_PATH")]
        workdir: PathBuf,
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
        /// Chat executes editorial instructions; completion is legacy raw continuation.
        #[arg(long, value_enum, default_value = "chat")]
        transport: novelai::Transport,
        /// Bound completion length for this one request (default 600, maximum 8192).
        #[arg(long)]
        max_tokens: Option<u32>,
        /// Sampling temperature for this one request (default 0.8, range 0–2).
        #[arg(long)]
        temperature: Option<f64>,
        /// Pass or artifact kind recorded separately from prompt prose.
        #[arg(long, value_name = "KIND")]
        artifact_kind: Option<String>,
        /// Explicit parent as EXPERIMENT/CANDIDATE.
        #[arg(long, value_name = "EXPERIMENT/CANDIDATE")]
        parent: Option<ArtifactReference>,
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
        NovelaiAction::DecodeResponse { record, workdir } => {
            let store = Store::open(&workdir).context(WritingSnafu)?;
            let completion =
                novelai::decode_response_record(&store, &record).context(NovelaiSnafu)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&completion)
                    .map_err(|_| novelai::Error::InvalidRequest)
                    .context(NovelaiSnafu)?
            );
        }
        NovelaiAction::Generate {
            experiment,
            workdir,
            variant,
            model,
            transport,
            max_tokens,
            temperature,
            artifact_kind,
            parent,
            dry_run,
        } => {
            let store = Store::open(&workdir).context(WritingSnafu)?;
            let snapshot = store
                .experiment(&experiment)
                .context(WritingSnafu)?
                .prepare(&variant)
                .context(WritingSnafu)?;
            let generation = PreparedGeneration::with_transport(
                snapshot,
                model,
                max_tokens,
                temperature,
                transport,
            )
            .context(NovelaiSnafu)?;
            let context = CandidateContext {
                artifact_kind,
                parent,
                editorial_revision: None,
            };
            store
                .validate_candidate_context(&context)
                .context(WritingSnafu)?;
            if dry_run {
                println!("{}", generation.preview().context(NovelaiSnafu)?);
                return Ok(());
            }
            let client = Client::from_env().context(NovelaiSnafu)?;
            println!(
                "{}",
                generate_candidate(&client, &store, generation, &context)?
            );
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
    context: &CandidateContext,
) -> Result<String> {
    // Catch known path/permission problems before sending a generation. This
    // is a probe, not a guarantee against later disk failure or concurrent edits.
    store
        .check_candidate_storage(generation.snapshot())
        .context(WritingSnafu)?;
    let completion = client.generate(&generation, store).context(NovelaiSnafu)?;
    if let Some(record) = &completion.response_record {
        eprintln!("NovelAI response saved privately: .writing/provider-responses/{record}.json");
    }
    if let Some(decoding) = &completion.decoding {
        eprintln!("Decoded {} response tokens with the pinned tokenizer; removed terminal protocol token IDs: {:?}.", decoding.token_count, decoding.removed_terminal_token_ids);
    }
    let id = store
        .record_candidate_with_context(
            generation.snapshot(),
            &completion.metadata(&generation),
            &completion.body,
            context,
        )
        .context(NovelaiCandidateNotSavedSnafu)?;
    for warning in completion.warnings() {
        eprintln!("Warning: {warning}. Saved unselected candidate {id}; no retry was made.");
    }
    Ok(id)
}
