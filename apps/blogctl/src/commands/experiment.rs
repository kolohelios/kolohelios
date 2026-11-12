//! Local-only writing commands. Deliberately bypass post-stage mutation and
//! automatic VCS sync: a candidate is not a draft or a publishing decision.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use snafu::ResultExt;

use crate::error::{Result, WritingSnafu};
use crate::writing::{self, Store};

/// Explicit workdir avoids silently writing private experiments into cwd.
#[derive(Debug, Args)]
pub struct ExperimentArgs {
    #[arg(long, value_name = "PRIVATE_PATH")]
    pub workdir: PathBuf,
    #[command(subcommand)]
    pub action: ExperimentAction,
}

#[derive(Debug, Subcommand)]
pub enum ExperimentAction {
    /// Import a private JSON definition; refuses an existing ID.
    Import { file: PathBuf },
    /// List experiment IDs without printing seeds.
    List,
    /// Print a private experiment definition as JSON.
    Show { id: String },
    /// Preview the exact prompt locally, without credentials or API calls.
    Prepare {
        id: String,
        #[arg(long)]
        variant: String,
    },
}

#[derive(Debug, Args)]
pub struct CandidateArgs {
    #[arg(long, value_name = "PRIVATE_PATH")]
    pub workdir: PathBuf,
    #[command(subcommand)]
    pub action: CandidateAction,
}

#[derive(Debug, Subcommand)]
pub enum CandidateAction {
    /// Record externally obtained text; provenance is asserted by the caller.
    /// No generation, review, promotion, or VCS sync occurs.
    Import {
        experiment: String,
        #[arg(long)]
        variant: String,
        #[arg(long)]
        metadata_file: PathBuf,
        #[arg(long)]
        body_file: PathBuf,
    },
    /// List candidate IDs for an experiment.
    List { experiment: String },
    /// Print an immutable candidate including provenance and private snapshot.
    Read { experiment: String, id: String },
}

pub fn experiment(args: ExperimentArgs) -> Result<()> {
    run_experiment(args).context(WritingSnafu)
}

fn run_experiment(args: ExperimentArgs) -> writing::Result<()> {
    let store = Store::open(&args.workdir)?;
    match args.action {
        ExperimentAction::Import { file } => println!("{}", store.import_experiment(&file)?),
        ExperimentAction::List => {
            for id in store.experiments()? {
                println!("{id}");
            }
        }
        ExperimentAction::Show { id } => {
            let exp = store.experiment(&id)?;
            let json =
                serde_json::to_string_pretty(&exp).map_err(|source| writing::Error::Json {
                    path: args.workdir,
                    source,
                })?;
            println!("{json}");
        }
        ExperimentAction::Prepare { id, variant } => {
            print!("{}", store.experiment(&id)?.prepare(&variant)?.prompt());
        }
    }
    Ok(())
}

pub fn candidate(args: CandidateArgs) -> Result<()> {
    run_candidate(args).context(WritingSnafu)
}

fn run_candidate(args: CandidateArgs) -> writing::Result<()> {
    let store = Store::open(&args.workdir)?;
    match args.action {
        CandidateAction::Import {
            experiment,
            variant,
            metadata_file,
            body_file,
        } => {
            let prepared = store.experiment(&experiment)?.prepare(&variant)?;
            let metadata = writing::read_json(&metadata_file)?;
            let body = writing::read_text(&body_file)?;
            println!("{}", store.record_candidate(&prepared, &metadata, &body)?);
        }
        CandidateAction::List { experiment } => {
            for id in store.candidates(&experiment)? {
                println!("{id}");
            }
        }
        CandidateAction::Read { experiment, id } => {
            print!("{}", store.read_candidate(&experiment, &id)?);
        }
    }
    Ok(())
}
