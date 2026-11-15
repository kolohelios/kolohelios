//! Import-only writing evidence packs.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use snafu::ResultExt;

use crate::error::{Result, WritingSnafu};
use crate::writing::Store;

#[derive(Debug, Args)]
pub struct EvidenceArgs {
    #[arg(long, value_name = "PRIVATE_PATH")]
    pub workdir: PathBuf,
    #[command(subcommand)]
    pub action: EvidenceAction,
}

#[derive(Debug, Subcommand)]
pub enum EvidenceAction {
    /// Import one local evidence-pack JSON file; never browse or generate.
    Import { file: PathBuf },
    /// List evidence-pack IDs without printing private evidence.
    List,
    /// Print and verify one structured evidence pack as JSON.
    Inspect { id: String },
}

pub fn run(args: EvidenceArgs) -> Result<()> {
    let store = Store::open(&args.workdir).context(WritingSnafu)?;
    match args.action {
        EvidenceAction::Import { file } => {
            println!(
                "{}",
                store.import_evidence_pack(&file).context(WritingSnafu)?
            );
        }
        EvidenceAction::List => {
            for id in store.evidence_packs().context(WritingSnafu)? {
                println!("{id}");
            }
        }
        EvidenceAction::Inspect { id } => {
            let pack = store.evidence_pack(&id).context(WritingSnafu)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&pack)
                    .map_err(|source| {
                        crate::writing::Error::Json {
                            path: args.workdir,
                            source,
                        }
                    })
                    .context(WritingSnafu)?
            );
        }
    }
    Ok(())
}
