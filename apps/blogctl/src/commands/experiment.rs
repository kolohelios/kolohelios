//! Local-only writing commands. Deliberately bypass post-stage mutation and
//! automatic VCS sync: a candidate is not a draft or a publishing decision.

use std::io::{BufRead, BufReader, IsTerminal, Write};
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
    /// Copy prototype data from explicit private paths without overwriting files.
    MigrateNovelaiLab {
        #[arg(long)]
        experiment_file: PathBuf,
        #[arg(long)]
        runs_dir: PathBuf,
    },
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
    /// Record a human decision in a separate immutable review (terminal required).
    Review {
        experiment: String,
        id: String,
        #[arg(long, value_enum)]
        decision: writing::ReviewDecision,
        #[arg(long)]
        reviewer: String,
        #[arg(long)]
        notes_file: Option<PathBuf>,
    },
    /// Read review history, checking every decision against current candidate bytes.
    Reviews { experiment: String, id: String },
    /// List candidate IDs for an experiment.
    List { experiment: String },
    /// Print an immutable candidate including provenance and private snapshot.
    Read { experiment: String, id: String },
}

fn authorize_review(
    decision: writing::ReviewDecision,
    candidate_hash: &str,
) -> writing::Result<()> {
    let denied = || writing::Error::Invalid {
        reason: "review requires fresh confirmation at a controlling terminal".into(),
    };
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| denied())?;
    if !tty.is_terminal() {
        return Err(denied());
    }
    let input = tty.try_clone().map_err(|_| denied())?;
    confirm_review(decision, candidate_hash, BufReader::new(input), &mut tty)
}

fn confirm_review(
    decision: writing::ReviewDecision,
    candidate_hash: &str,
    input: impl BufRead,
    output: &mut impl Write,
) -> writing::Result<()> {
    let denied = || writing::Error::Invalid {
        reason: "review was not confirmed".into(),
    };
    let word = match decision {
        writing::ReviewDecision::Accept => "ACCEPT",
        writing::ReviewDecision::Reject => "REJECT",
    };
    writeln!(output, "Decision={word}, candidate_sha256={candidate_hash}. This does NOT promote manuscript text. Type {word}:").map_err(|_| denied())?;
    output.flush().map_err(|_| denied())?;
    let mut answer = String::new();
    input
        .take(256)
        .read_line(&mut answer)
        .map_err(|_| denied())?;
    if answer.trim_end_matches(['\r', '\n']) != word {
        return Err(denied());
    }
    Ok(())
}

pub fn experiment(args: ExperimentArgs) -> Result<()> {
    run_experiment(args).context(WritingSnafu)
}

fn run_experiment(args: ExperimentArgs) -> writing::Result<()> {
    let store = Store::open(&args.workdir)?;
    match args.action {
        ExperimentAction::Import { file } => println!("{}", store.import_experiment(&file)?),
        ExperimentAction::MigrateNovelaiLab {
            experiment_file,
            runs_dir,
        } => {
            let report = writing::migration::migrate(&store, &experiment_file, &runs_dir)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|source| writing::Error::Json {
                    path: args.workdir,
                    source
                })?
            );
        }
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
        CandidateAction::Review {
            experiment,
            id,
            decision,
            reviewer,
            notes_file,
        } => {
            let expected_hash = writing::hash(&store.read_candidate(&experiment, &id)?);
            let notes = match notes_file {
                Some(path) => writing::read_text(&path)?,
                None => String::new(),
            };
            authorize_review(decision, &expected_hash)?;
            println!(
                "{}",
                store.record_review(&experiment, &id, &expected_hash, decision, reviewer, notes)?
            );
        }
        CandidateAction::Reviews { experiment, id } => {
            let records = store.reviews(&experiment, &id)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&records).map_err(|source| writing::Error::Json {
                    path: args.workdir,
                    source
                })?
            );
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

#[cfg(test)]
mod tests;
