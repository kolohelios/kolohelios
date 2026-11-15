//! Consumer-configured writing workflow preparation and explicit sessions.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::{Args, Subcommand};
use snafu::ResultExt;

use crate::commands::experiment::authorize_review;
use crate::error::{NovelaiSnafu, Result, WritingSnafu};
use crate::novelai::{Client, PreparedGeneration};
use crate::writing::{
    self, ArtifactReference, CandidateContext, GenerationMetadata, ReviewDecision, Store,
};
use crate::writing_session::Session;
use crate::writing_workflow::{self, PassMode, WorkflowDefinition, WorkflowPass};

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
        /// Explicit evidence-pack ID; repeat to include more than one.
        #[arg(long = "evidence", value_name = "EVIDENCE_PACK_ID")]
        evidence: Vec<String>,
    },
    /// Conduct one resumable session; every mutation remains an explicit action.
    Conduct {
        session: String,
        #[arg(long, value_name = "PATH")]
        config_root: PathBuf,
        #[arg(long, value_name = "RELATIVE_PATH")]
        config: PathBuf,
        /// Explicit model used only by preview and generation actions.
        #[arg(long)]
        model: String,
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
            evidence,
        } => println!(
            "{}",
            writing_workflow::prepare_with_evidence(
                &store,
                &config_root,
                &config,
                &pass,
                input.as_ref(),
                &evidence,
            )
            .context(WritingSnafu)?
        ),
        WorkflowAction::Conduct {
            session,
            config_root,
            config,
            model,
        } => {
            writing_workflow::validate_store_boundary(&store, &config_root)
                .context(WritingSnafu)?;
            let definition =
                writing_workflow::definition(&config_root, &config).context(WritingSnafu)?;
            let mut session = Session::open(&store, &session, &definition).context(WritingSnafu)?;
            let stdin = io::stdin();
            let stdout = io::stdout();
            conduct(
                &store,
                &definition,
                &mut session,
                &config_root,
                &config,
                &model,
                stdin.lock(),
                stdout.lock(),
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn conduct(
    store: &Store,
    definition: &WorkflowDefinition,
    session: &mut Session,
    config_root: &Path,
    config: &Path,
    model: &str,
    mut input: impl BufRead,
    mut output: impl Write,
) -> Result<()> {
    loop {
        show_status(session, definition, &mut output)?;
        line(
            &mut output,
            "1 read  2 preview  3 generate  4 import  5 select  6 review  7 advance  8 pass  e evidence  9 stop",
        )?;
        let choice = ask(&mut input, &mut output, "> ")?;
        if choice.is_empty() || choice == "9" {
            return Ok(());
        }
        let result = match choice.as_str() {
            "1" => read_selected(store, session, definition, &mut output),
            "2" => preview(
                store,
                session,
                definition,
                config_root,
                config,
                model,
                &mut output,
            ),
            "3" => generate(
                store,
                session,
                definition,
                config_root,
                config,
                model,
                &mut output,
            ),
            "4" => import_artifact(
                store,
                session,
                definition,
                config_root,
                config,
                &mut input,
                &mut output,
            ),
            "5" => select_candidate(
                store,
                session,
                definition,
                config_root,
                config,
                &mut input,
                &mut output,
            ),
            "6" => review(store, session, &mut input, &mut output),
            "7" => session.advance(definition).context(WritingSnafu),
            "8" => choose_pass(store, session, definition, &mut input, &mut output),
            "e" => choose_evidence(store, session, &mut input, &mut output),
            _ => {
                line(&mut output, "Unknown choice")?;
                continue;
            }
        };
        if let Err(error) = result {
            line(&mut output, &format!("Error: {error}"))?;
        }
    }
}

fn show_status(
    session: &Session,
    definition: &WorkflowDefinition,
    output: &mut impl Write,
) -> Result<()> {
    let pass = session.pass(definition).context(WritingSnafu)?;
    line(output, "")?;
    line(
        output,
        &format!("Pass: {} ({})", pass.name(), pass.mode().as_str()),
    )?;
    line(
        output,
        &format!(
            "Input: {}",
            session
                .input()
                .map(reference)
                .unwrap_or_else(|| "none".into())
        ),
    )?;
    line(
        output,
        &format!(
            "Evidence packs: {}",
            if session.evidence().is_empty() {
                "none".into()
            } else {
                session.evidence().join(", ")
            }
        ),
    )?;
    line(
        output,
        &format!(
            "Prepared experiment: {}",
            session.experiment().unwrap_or("none")
        ),
    )?;
    line(
        output,
        &format!(
            "Selected candidate: {}",
            session
                .selected()
                .map(reference)
                .unwrap_or_else(|| "none".into())
        ),
    )
}

fn preview(
    store: &Store,
    session: &mut Session,
    definition: &WorkflowDefinition,
    config_root: &Path,
    config: &Path,
    model: &str,
    output: &mut impl Write,
) -> Result<()> {
    let pass = session.pass(definition).context(WritingSnafu)?;
    require_generate(pass)?;
    let experiment = session
        .prepare(store, config_root, config)
        .context(WritingSnafu)?;
    let generation = prepared_generation(store, &experiment, pass, model)?;
    line(output, &generation.preview().context(NovelaiSnafu)?)
}

fn generate(
    store: &Store,
    session: &mut Session,
    definition: &WorkflowDefinition,
    config_root: &Path,
    config: &Path,
    model: &str,
    output: &mut impl Write,
) -> Result<()> {
    let pass = session.pass(definition).context(WritingSnafu)?;
    require_generate(pass)?;
    let experiment = session
        .prepare(store, config_root, config)
        .context(WritingSnafu)?;
    let generation = prepared_generation(store, &experiment, pass, model)?;
    let context = CandidateContext {
        artifact_kind: Some(pass.artifact_kind().into()),
        parent: session.input().cloned(),
    };
    store
        .validate_candidate_context(&context)
        .context(WritingSnafu)?;
    let client = Client::from_env().context(NovelaiSnafu)?;
    let candidate = super::novelai::generate_candidate(&client, store, generation, &context)?;
    line(
        output,
        &format!(
            "Generated one unselected candidate: {candidate}. Select it explicitly to continue."
        ),
    )
}

fn prepared_generation(
    store: &Store,
    experiment: &str,
    pass: &WorkflowPass,
    model: &str,
) -> Result<PreparedGeneration> {
    let snapshot = store
        .experiment(experiment)
        .context(WritingSnafu)?
        .prepare(pass.name())
        .context(WritingSnafu)?;
    PreparedGeneration::new(
        snapshot,
        model.into(),
        pass.max_tokens(),
        pass.temperature(),
    )
    .context(NovelaiSnafu)
}

fn require_generate(pass: &WorkflowPass) -> Result<()> {
    if pass.mode() == PassMode::ImportOnly {
        return Err(writing::Error::Invalid {
            reason: "this pass is import-only; no model request was sent".into(),
        })
        .context(WritingSnafu);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn import_artifact(
    store: &Store,
    session: &mut Session,
    definition: &WorkflowDefinition,
    config_root: &Path,
    config: &Path,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    let pass = session.pass(definition).context(WritingSnafu)?;
    let experiment = session
        .prepare(store, config_root, config)
        .context(WritingSnafu)?;
    let body_path = PathBuf::from(ask(input, output, "Body/evidence file: ")?);
    let provenance = ask(input, output, "Provenance note: ")?;
    if provenance.is_empty() {
        return Err(writing::Error::Invalid {
            reason: "a provenance note is required".into(),
        })
        .context(WritingSnafu);
    }
    let body = writing::read_text(&body_path).context(WritingSnafu)?;
    let metadata = GenerationMetadata {
        provider: "external-import".into(),
        model: if pass.mode() == PassMode::ImportOnly {
            "evidence-pack".into()
        } else {
            "author-edit".into()
        },
        ai_provenance: provenance,
        parameters: BTreeMap::from([("pass".into(), pass.name().into())]),
    };
    let prepared = store
        .experiment(&experiment)
        .context(WritingSnafu)?
        .prepare(pass.name())
        .context(WritingSnafu)?;
    let context = CandidateContext {
        artifact_kind: Some(pass.artifact_kind().into()),
        parent: session.input().cloned(),
    };
    let candidate = store
        .record_candidate_with_context(&prepared, &metadata, &body, &context)
        .context(WritingSnafu)?;
    line(
        output,
        &format!("Imported unselected candidate: {candidate}. Select it explicitly to continue."),
    )
}

fn select_candidate(
    store: &Store,
    session: &mut Session,
    definition: &WorkflowDefinition,
    config_root: &Path,
    config: &Path,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    if session.experiment().is_none() {
        session
            .prepare(store, config_root, config)
            .context(WritingSnafu)?;
    }
    let experiment = session
        .experiment()
        .ok_or_else(|| crate::error::Error::Writing {
            source: writing::Error::Invalid {
                reason: "current pass has no prepared experiment".into(),
            },
        })?;
    let candidates = store.candidates(experiment).context(WritingSnafu)?;
    if candidates.is_empty() {
        line(output, "No candidates")?;
        return Ok(());
    }
    for candidate in &candidates {
        line(output, candidate)?;
    }
    let candidate = ask(input, output, "Candidate ID (blank cancels): ")?;
    if candidate.is_empty() {
        return Ok(());
    }
    session.select(store, &candidate).context(WritingSnafu)?;
    check_candidate(store, session, definition, output)
}

fn read_selected(
    store: &Store,
    session: &Session,
    definition: &WorkflowDefinition,
    output: &mut impl Write,
) -> Result<()> {
    let selected = session
        .selected()
        .ok_or_else(|| crate::error::Error::Writing {
            source: writing::Error::Invalid {
                reason: "no candidate is selected".into(),
            },
        })?;
    line(
        output,
        &store
            .read_candidate(&selected.experiment_id, &selected.candidate_id)
            .context(WritingSnafu)?,
    )?;
    check_candidate(store, session, definition, output)
}

fn check_candidate(
    store: &Store,
    session: &Session,
    definition: &WorkflowDefinition,
    output: &mut impl Write,
) -> Result<()> {
    let selected = session
        .selected()
        .ok_or_else(|| crate::error::Error::Writing {
            source: writing::Error::Invalid {
                reason: "no candidate is selected".into(),
            },
        })?;
    let body = &store
        .candidate(&selected.experiment_id, &selected.candidate_id)
        .context(WritingSnafu)?
        .body;
    let pass = session.pass(definition).context(WritingSnafu)?;
    let words = body
        .split(|character: char| {
            !character.is_alphanumeric() && character != '\'' && character != '’'
        })
        .filter(|word| !word.is_empty())
        .count();
    let mut warnings = Vec::new();
    if pass.min_words().is_some_and(|minimum| words < minimum) {
        warnings.push(format!("{words} words is below the pass minimum"));
    }
    if pass.max_words().is_some_and(|maximum| words > maximum) {
        warnings.push(format!("{words} words exceeds the pass maximum"));
    }
    let lowercase = body.to_ascii_lowercase();
    if lowercase.contains("<think") || lowercase.contains("</think") {
        warnings.push("candidate contains a think tag".into());
    }
    if body.trim_end().ends_with('}') {
        warnings.push("candidate ends with a likely leaked instruct brace".into());
    }
    if pass.forbid_workflow_metadata()
        && (lowercase.contains("next action:") || lowercase.contains("status:"))
    {
        warnings.push("candidate contains likely invented workflow metadata".into());
    }
    if pass.require_complete_ending()
        && !matches!(
            body.trim_end().chars().last(),
            Some('.' | '!' | '?' | ')' | ']' | '"' | '\'')
        )
    {
        warnings.push("candidate appears to end mid-sentence".into());
    }
    if warnings.is_empty() {
        line(output, &format!("Advisory checks passed ({words} words)."))
    } else {
        line(
            output,
            "Advisory checks reported issues; the candidate remains immutable and selected:",
        )?;
        for warning in warnings {
            line(output, &format!("- {warning}"))?;
        }
        Ok(())
    }
}

fn review(
    store: &Store,
    session: &mut Session,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    let selected = session
        .selected()
        .cloned()
        .ok_or_else(|| crate::error::Error::Writing {
            source: writing::Error::Invalid {
                reason: "no candidate is selected".into(),
            },
        })?;
    let decision = match ask(input, output, "Decision [accept/reject]: ")?.as_str() {
        "accept" => ReviewDecision::Accept,
        "reject" => ReviewDecision::Reject,
        _ => {
            return Err(writing::Error::Invalid {
                reason: "decision must be accept or reject".into(),
            })
            .context(WritingSnafu);
        }
    };
    let reviewer = ask(input, output, "Reviewer: ")?;
    if reviewer.is_empty() {
        return Err(writing::Error::Invalid {
            reason: "reviewer is required".into(),
        })
        .context(WritingSnafu);
    }
    let notes_path = ask(input, output, "Notes file (blank for none): ")?;
    let notes = if notes_path.is_empty() {
        String::new()
    } else {
        writing::read_text(Path::new(&notes_path)).context(WritingSnafu)?
    };
    let candidate_hash = writing::hash(
        &store
            .read_candidate(&selected.experiment_id, &selected.candidate_id)
            .context(WritingSnafu)?,
    );
    authorize_review(decision, &candidate_hash).context(WritingSnafu)?;
    let review_id = store
        .record_review(
            &selected.experiment_id,
            &selected.candidate_id,
            &candidate_hash,
            decision,
            reviewer,
            notes,
        )
        .context(WritingSnafu)?;
    session
        .record_review(match decision {
            ReviewDecision::Accept => "review-accept",
            ReviewDecision::Reject => "review-reject",
        })
        .context(WritingSnafu)?;
    line(output, &format!("Recorded review: {review_id}"))
}

fn choose_pass(
    store: &Store,
    session: &mut Session,
    definition: &WorkflowDefinition,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    for (index, pass) in definition.passes().iter().enumerate() {
        line(
            output,
            &format!("{}. {} ({})", index + 1, pass.name(), pass.mode().as_str()),
        )?;
    }
    let raw = ask(input, output, "Pass number (blank cancels): ")?;
    if raw.is_empty() {
        return Ok(());
    }
    let index = raw
        .parse::<usize>()
        .ok()
        .and_then(|number| number.checked_sub(1));
    let pass = index
        .and_then(|index| definition.passes().get(index))
        .ok_or_else(|| crate::error::Error::Writing {
            source: writing::Error::Invalid {
                reason: "invalid pass number".into(),
            },
        })?;
    let prior = ask(
        input,
        output,
        "Input EXPERIMENT/CANDIDATE (blank uses selected/none): ",
    )?;
    let candidate = if prior.is_empty() {
        session.selected().cloned()
    } else {
        Some(ArtifactReference::from_str(&prior).context(WritingSnafu)?)
    };
    session
        .choose_pass(definition, pass.name(), candidate, store)
        .context(WritingSnafu)
}

fn choose_evidence(
    store: &Store,
    session: &mut Session,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    let available = store.evidence_packs().context(WritingSnafu)?;
    if available.is_empty() {
        line(output, "No evidence packs are available")?;
    } else {
        line(output, "Available evidence packs:")?;
        for id in available {
            line(output, &format!("- {id}"))?;
        }
    }
    let raw = ask(
        input,
        output,
        "Evidence pack IDs (comma-separated; blank clears): ",
    )?;
    let ids = raw
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    session.set_evidence(store, ids).context(WritingSnafu)
}

fn reference(reference: &ArtifactReference) -> String {
    format!("{}/{}", reference.experiment_id, reference.candidate_id)
}

fn ask(input: &mut impl BufRead, output: &mut impl Write, prompt: &str) -> Result<String> {
    write!(output, "{prompt}").map_err(|source| crate::error::Error::Io {
        path: PathBuf::from("terminal output"),
        source,
    })?;
    output.flush().map_err(|source| crate::error::Error::Io {
        path: PathBuf::from("terminal output"),
        source,
    })?;
    let mut answer = String::new();
    input
        .read_line(&mut answer)
        .map_err(|source| crate::error::Error::Io {
            path: PathBuf::from("terminal input"),
            source,
        })?;
    Ok(answer.trim().into())
}

fn line(output: &mut impl Write, text: &str) -> Result<()> {
    writeln!(output, "{text}").map_err(|source| crate::error::Error::Io {
        path: PathBuf::from("terminal output"),
        source,
    })
}
