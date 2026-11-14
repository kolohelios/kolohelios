//! Consumer-configured writing workflow preparation.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use snafu::ResultExt;

use crate::error::{Result, WritingSnafu};
use crate::writing::{ArtifactReference, Store};
use crate::writing_workflow;

#[derive(Debug, Args)]
pub struct WorkflowArgs {
    #[arg(long, value_name = "PRIVATE_PATH")]
    pub workdir: PathBuf,
    #[command(subcommand)]
    pub action: WorkflowAction,
}

#[derive(Debug, Subcommand)]
pub enum WorkflowAction {
    /// Compose and ensure one deterministic pass experiment without generation.
    Prepare {
        pass: String,
        /// Root beneath which configuration and context files must remain.
        #[arg(long, value_name = "PATH")]
        config_root: PathBuf,
        /// Workflow JSON path relative to --config-root.
        #[arg(long, value_name = "RELATIVE_PATH")]
        config: PathBuf,
        /// Explicit prior candidate as EXPERIMENT/CANDIDATE.
        #[arg(long, value_name = "EXPERIMENT/CANDIDATE")]
        input: Option<ArtifactReference>,
    },
}

pub fn run(args: WorkflowArgs) -> Result<()> {
    let store = Store::open(&args.workdir).context(WritingSnafu)?;
    match args.action {
        WorkflowAction::Prepare {
            pass,
            config_root,
            config,
            input,
        } => println!(
            "{}",
            writing_workflow::prepare(&store, &config_root, &config, &pass, input.as_ref())
                .context(WritingSnafu)?
        ),
    }
    Ok(())
}
