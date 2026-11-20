//! Author-facing local review loop. No provider calls live in this module.

use super::*;
use crate::writing_revision::{EditorialRevision, OriginKind, RevisionKind};
use crate::writing_session::SessionBinding;
use crate::writing_workflow::GuardChange;
use std::io::IsTerminal;
use std::process::{Command, Stdio};

pub(super) struct Opened {
    pub session: Session,
    pub definition: WorkflowDefinition,
    pub binding: SessionBinding,
}

fn invalid(reason: impl Into<String>) -> crate::error::Error {
    crate::error::Error::Writing {
        source: writing::Error::Invalid {
            reason: reason.into(),
        },
    }
}

pub(super) fn open(
    store: &Store,
    name: &str,
    root: Option<PathBuf>,
    config: Option<PathBuf>,
    model: Option<String>,
) -> Result<Opened> {
    let mut session = match (root, config) {
        (Some(root), Some(config)) => {
            writing_workflow::validate_store_boundary(store, &root).context(WritingSnafu)?;
            let definition = writing_workflow::definition(&root, &config).context(WritingSnafu)?;
            let names = Session::names(store).context(WritingSnafu)?;
            let mut session = if names.iter().any(|existing| existing == name) {
                Session::read(store, name).context(WritingSnafu)?
            } else {
                Session::open(store, name, &definition).context(WritingSnafu)?
            };
            let model = model
                .or_else(|| session.binding().map(|binding| binding.model.clone()))
                .ok_or_else(|| {
                    invalid("an explicit model is required when registering a session")
                })?;
            session
                .bind(store, &root, &config, &model)
                .context(WritingSnafu)?;
            session
        }
        (Some(root), None) => {
            let mut session = Session::read(store, name).context(WritingSnafu)?;
            session.relocate(store, &root).context(WritingSnafu)?;
            if let Some(model) = model { session.set_model(&model).context(WritingSnafu)?; }
            session
        },
        (None, None) => {
            let mut session = Session::read(store, name).context(WritingSnafu)?;
            if let Some(model) = model {
                session.set_model(&model).context(WritingSnafu)?;
            }
            session
        }
        _ => {
            return Err(invalid(
                "registration needs --config-root and --config; a bound session can resume without either, or relocate with --config-root alone",
            ))
        }
    };
    let definition = session.bound_definition().context(WritingSnafu)?;
    let binding = session
        .binding()
        .cloned()
        .ok_or_else(|| invalid("missing session binding"))?;
    session.refresh().context(WritingSnafu)?;
    Ok(Opened {
        session,
        definition,
        binding,
    })
}

pub(super) fn status(store: &Store, session: &Session, output: &mut impl Write) -> Result<()> {
    line(
        output,
        &format!(
            "Session: {} / pass: {}",
            session.name(),
            session.pass_name()
        ),
    )?;
    if let Some(target) = session.review_target() {
        line(
            output,
            &format!("Review draft (not acceptance): {}", reference(target)),
        )?;
        if let Ok(definition) = session.bound_definition() {
            let document = store
                .candidate(&target.experiment_id, &target.candidate_id)
                .context(WritingSnafu)?;
            show_assessment(
                &crate::writing_assessment::assess(
                    session.pass(&definition).context(WritingSnafu)?,
                    &document,
                ),
                output,
            )?;
        }
    } else {
        line(output, "No output under review. Input is available to read; choose a pass/input, preview, generate once, or import.")?;
    }
    line(output, &format!("Generation input: {}. Using a review draft as input is a separate explicit choice (u).", session.input().map(reference).unwrap_or_else(|| "configured source".into())))?;
    if let Some(working) = session.working_revision() {
        line(output, &format!("Pending working copy: {}. Resume edit, compare, save checkpoint, or explicitly abandon. No other draft action may discard it.", working.file.display()))?;
    } else if session.review_target().is_some() {
        line(output, "Next choices: read, compare, edit, or explicitly accept. A request is a separate choice.")?;
    }
    line(
        output,
        &format!(
            "Selected candidate: {}",
            session
                .selected()
                .map(reference)
                .unwrap_or_else(|| "none".into())
        ),
    )?;
    if let Some(selected) = session.selected() {
        line(
            output,
            &format!(
                "Accepted for this pass: {} (not polished approval).",
                reference(selected)
            ),
        )?;
    }
    line(
        output,
        &format!(
            "Resume: blogctl workflow --workdir {} conduct {}",
            shell_word(&store.root().display().to_string()),
            shell_word(session.name())
        ),
    )
}

pub(super) fn read_draft(
    store: &Store,
    session: &Session,
    width: u16,
    output: &mut impl Write,
) -> Result<()> {
    let body = if let Some(working) = session.working_revision() {
        line(
            output,
            "Pending working text, not an imported checkpoint or accepted draft:",
        )?;
        store.revision_body(&working.file).context(WritingSnafu)?
    } else {
        let target = session
            .review_target()
            .or(session.input())
            .ok_or_else(|| invalid("no draft to read"))?;
        store
            .candidate(&target.experiment_id, &target.candidate_id)
            .context(WritingSnafu)?
            .body
    };
    display(&body, width, output)
}

pub(super) fn compare(
    store: &Store,
    session: &Session,
    width: u16,
    output: &mut impl Write,
) -> Result<()> {
    let diff = if let Some(working) = session.working_revision() {
        let parent = store
            .candidate(&working.parent.experiment_id, &working.parent.candidate_id)
            .context(WritingSnafu)?;
        let body = store.revision_body(&working.file).context(WritingSnafu)?;
        similar::TextDiff::from_lines(&parent.body, &body)
            .unified_diff()
            .header("review parent", "pending working copy")
            .to_string()
    } else {
        let target = session
            .review_target()
            .ok_or_else(|| invalid("no output under review"))?;
        let document = store
            .candidate(&target.experiment_id, &target.candidate_id)
            .context(WritingSnafu)?;
        let parent = document
            .provenance
            .parent
            .as_ref()
            .or(session.input())
            .ok_or_else(|| invalid("review output has no explicit comparison parent"))?;
        store
            .revision_diff(parent, target)
            .context(WritingSnafu)?
            .unified_diff
    };
    if diff.is_empty() {
        return line(output, "No byte changes.");
    }
    display(&diff, width, output)
}

pub(super) fn edit(
    store: &Store,
    session: &mut Session,
    definition: &WorkflowDefinition,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    let prepared = session.begin_revision(store).context(WritingSnafu)?;
    line(
        output,
        &format!("Private working copy: {}", prepared.working_file.display()),
    )?;
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        });
    let Some(editor) = editor else {
        return line(output, "Set VISUAL or EDITOR to open an editor, or edit this file and choose save. The pending copy survives restart.");
    };
    // EDITOR is trusted user configuration, not model text. The path is a
    // positional argument, never interpolated into shell program text.
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("exec {editor} \"$1\""))
        .arg("blogctl-editor")
        .arg(&prepared.working_file)
        .env_remove("NOVELAI_API_TOKEN")
        .env_remove("OPENROUTER_API_KEY")
        .status()
        .map_err(|source| crate::error::Error::Io {
            path: "editor".into(),
            source,
        })?;
    if !status.success() {
        return Err(invalid(
            "editor did not finish successfully; working copy retained for resume/save/abandon",
        ));
    }
    save_revision(store, session, definition, input, output)
}

pub(super) fn save_revision(
    store: &Store,
    session: &mut Session,
    definition: &WorkflowDefinition,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    if session
        .close_unchanged_revision(store)
        .context(WritingSnafu)?
    {
        return line(output, "No text changes; the original review target is retained. No checkpoint or acceptance was invented.");
    }
    let contributor = match session.contributor() {
        Some(name) => name.to_owned(),
        None => {
            let name = ask(
                input,
                output,
                "Contributor label for this session (caller-asserted): ",
            )?;
            if name.is_empty() {
                return line(output, "Save cancelled; working copy retained.");
            }
            session.set_contributor(&name).context(WritingSnafu)?;
            name
        }
    };
    let candidate = session.finish_revision(store, definition, EditorialRevision {
        origin_kind: OriginKind::HumanRevision, revision_kind: RevisionKind::Substantive,
        contributors: vec![contributor], notes: "Author working copy saved through the session review loop; not accepted or advanced.".into(),
    }).context(WritingSnafu)?;
    line(
        output,
        &format!(
            "Checkpoint under review, not selected: {}",
            reference(&candidate)
        ),
    )?;
    let document = store
        .candidate(&candidate.experiment_id, &candidate.candidate_id)
        .context(WritingSnafu)?;
    show_assessment(
        &crate::writing_assessment::assess(
            session.pass(definition).context(WritingSnafu)?,
            &document,
        ),
        output,
    )
}

pub(super) fn abandon(
    session: &mut Session,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    if ask(
        input,
        output,
        "Type ABANDON to stop tracking this working copy (files remain): ",
    )? != "ABANDON"
    {
        return line(output, "Working copy retained.");
    }
    session.abandon_revision().context(WritingSnafu)?;
    line(
        output,
        "Working intent abandoned; files preserved. Review target and selection were not changed.",
    )
}

pub(super) fn focus(
    store: &Store,
    session: &mut Session,
    definition: &WorkflowDefinition,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    if let Some(experiment) = session.experiment() {
        for candidate in store.candidates(experiment).context(WritingSnafu)? {
            line(output, &format!("{experiment}/{candidate}"))?;
        }
    }
    let value = ask(
        input,
        output,
        "Output or direct revision EXPERIMENT/CANDIDATE (blank cancels): ",
    )?;
    if value.is_empty() {
        return Ok(());
    }
    let candidate = ArtifactReference::from_str(&value).context(WritingSnafu)?;
    show_assessment(
        &session
            .focus(store, definition, candidate)
            .context(WritingSnafu)?,
        output,
    )
}

pub(super) fn use_as_input(
    store: &Store,
    session: &mut Session,
    definition: &WorkflowDefinition,
    output: &mut impl Write,
) -> Result<()> {
    let candidate = session
        .review_target()
        .cloned()
        .ok_or_else(|| invalid("no review draft to use as input"))?;
    let pass = session.pass_name().to_owned();
    session
        .choose_pass(definition, &pass, Some(candidate), store)
        .context(WritingSnafu)?;
    line(output, "Review draft is now the explicit input for this same pass. No request, acceptance, or advancement occurred.")
}

pub(super) fn accept(
    store: &Store,
    session: &mut Session,
    definition: &WorkflowDefinition,
    next: bool,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    let target = session
        .review_target()
        .ok_or_else(|| invalid("no draft under review"))?;
    let document = store
        .candidate(&target.experiment_id, &target.candidate_id)
        .context(WritingSnafu)?;
    let assessment = crate::writing_assessment::assess(
        session.pass(definition).context(WritingSnafu)?,
        &document,
    );
    show_assessment(&assessment, output)?;
    if next {
        let index = definition
            .passes()
            .iter()
            .position(|pass| pass.name() == session.pass_name())
            .ok_or_else(|| invalid("unknown pass"))?;
        let Some(next_pass) = definition.passes().get(index + 1) else {
            return Err(invalid("final pass reached; accept without advancement, then use separate polished approval/export"));
        };
        if next_pass
            .protected_text()
            .iter()
            .any(|text| !document.body.contains(text))
        {
            return Err(invalid("next-pass protection is absent from the reviewed draft; use guards for an intentional author change, or revise the output"));
        }
    }
    if !assessment.findings.is_empty()
        && ask(
            input,
            output,
            "Type ACCEPT WITH FINDINGS to select despite these advisory findings: ",
        )? != "ACCEPT WITH FINDINGS"
    {
        return line(output, "Not accepted.");
    }
    session.accept_target(store).context(WritingSnafu)?;
    if next {
        session.advance(definition).context(WritingSnafu)?;
    }
    line(
        output,
        if next {
            "Draft accepted and advanced. No provider request was made."
        } else {
            "Draft accepted for this pass. No advancement or provider request."
        },
    )
}

pub(super) fn guards(
    store: &Store,
    session: &mut Session,
    definition: &mut WorkflowDefinition,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    let target = session
        .review_target()
        .ok_or_else(|| invalid("no draft under review"))?;
    let document = store
        .candidate(&target.experiment_id, &target.candidate_id)
        .context(WritingSnafu)?;
    if document.provenance.editorial_revision.is_none() {
        return Err(invalid("protection changes require an explicit editorial checkpoint, not merely a model output"));
    }
    let missing = session
        .pass(definition)
        .context(WritingSnafu)?
        .protected_text()
        .iter()
        .filter(|text| !document.body.contains(text.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return line(
            output,
            "No protected literal was removed. No policy change is needed.",
        );
    }
    line(output, "Only change protections for intentional author edits, never to hide a failed model result. Old policy and assessments remain frozen.")?;
    let mut changes = Vec::new();
    for previous in missing {
        line(output, &format!("Missing protection: {previous}"))?;
        let replacement = ask(
            input,
            output,
            "New exact protection in this draft, '-' to remove, or blank to cancel: ",
        )?;
        if replacement.is_empty() {
            return line(output, "Protection change cancelled.");
        }
        changes.push(GuardChange {
            previous,
            replacement: if replacement == "-" {
                None
            } else {
                Some(replacement)
            },
        });
    }
    let updated = definition
        .change_guards(&changes, &document.body)
        .context(WritingSnafu)?;
    let proposal = format!(
        "{}-r{}",
        session.name().chars().take(48).collect::<String>(),
        &updated.fingerprint()[..8]
    );
    let name = ask(
        input,
        output,
        &format!("New policy session name (blank uses {proposal}): "),
    )?;
    if ask(
        input,
        output,
        "Type FORK POLICY to record these decisions across matching protections in the new policy: ",
    )? != "FORK POLICY"
    {
        return line(output, "Protection change cancelled.");
    }
    let fork = session
        .fork_guards(
            store,
            if name.is_empty() { &proposal } else { &name },
            changes,
        )
        .context(WritingSnafu)?;
    *definition = fork.bound_definition().context(WritingSnafu)?;
    *session = fork;
    line(output, &format!("Now reviewing in {}. Original session unchanged. Nothing selected or advanced. Resume this name; workflow sessions lists every branch.", session.name()))
}

fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Plain terminal rendering only. Wrapping/control escaping never changes bytes
/// in the immutable candidate or a revision working copy.
fn render(body: &str, width: usize) -> String {
    let mut rendered = String::new();
    for line in body.lines() {
        let mut column = 0;
        for word in line.split_whitespace() {
            let safe = word.chars().filter(|c| !c.is_control()).collect::<String>();
            if column > 0 && column + 1 + safe.chars().count() > width {
                rendered.push('\n');
                column = 0;
            }
            if column > 0 {
                rendered.push(' ');
                column += 1;
            }
            for character in safe.chars() {
                if column >= width {
                    rendered.push('\n');
                    column = 0;
                }
                rendered.push(character);
                column += 1;
            }
        }
        rendered.push('\n');
    }
    rendered
}

fn display(body: &str, width: u16, output: &mut impl Write) -> Result<()> {
    let rendered = render(body, usize::from(width));
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        match Command::new("less")
            .arg("-R")
            .env_remove("NOVELAI_API_TOKEN")
            .env_remove("OPENROUTER_API_KEY")
            .env("LESSHISTFILE", "-")
            .env("LESSSECURE", "1")
            .env_remove("LESSOPEN")
            .env_remove("LESSCLOSE")
            .stdin(Stdio::piped())
            .spawn()
        {
            Ok(mut pager) => {
                let write_error = pager
                    .stdin
                    .take()
                    .and_then(|mut pipe| pipe.write_all(rendered.as_bytes()).err());
                let status = pager.wait().map_err(|source| crate::error::Error::Io {
                    path: "reader".into(),
                    source,
                })?;
                if let Some(source) =
                    write_error.filter(|error| error.kind() != io::ErrorKind::BrokenPipe)
                {
                    return Err(crate::error::Error::Io {
                        path: "reader".into(),
                        source,
                    });
                }
                if !status.success() {
                    return Err(invalid(
                        "reader exited unsuccessfully; no draft state changed",
                    ));
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(crate::error::Error::Io {
                    path: "reader".into(),
                    source,
                })
            }
        }
    }
    line(output, &rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reading_wraps_words_and_long_tokens_without_terminal_controls() {
        let body =
            "A short sentence and another sentence.\n\n1234567890123456789012345\n\u{1b}[2Jtext\n";
        let rendered = render(body, 20);
        assert!(rendered.lines().all(|line| line.chars().count() <= 20));
        assert!(rendered.contains("\n\n"));
        assert!(!rendered.contains('\u{1b}'));
        assert_eq!(render("", 20), "");
        assert_eq!(shell_word("a'b c"), "'a'\\''b c'");
    }
}
