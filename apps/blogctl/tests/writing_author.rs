//! Synthetic, provider-free author review across process restarts.
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use blogctl::writing::{self, ArtifactReference, CandidateContext, GenerationMetadata, Store};
use blogctl::writing_revision::{EditorialRevision, OriginKind, RevisionKind};
use blogctl::writing_session::Session;
use blogctl::writing_workflow::{self, GuardChange, WorkflowDefinition};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing")
        .join(name)
}
fn root() -> PathBuf {
    fixture("workflow")
}
fn config() -> &'static Path {
    Path::new("valid/author-loop.json")
}
fn body(name: &str) -> String {
    fs::read_to_string(fixture(&format!("author/{name}.md"))).unwrap()
}
fn metadata() -> GenerationMetadata {
    writing::read_json(&fixture("generation.json")).unwrap()
}
fn revision() -> EditorialRevision {
    EditorialRevision {
        origin_kind: OriginKind::HumanRevision,
        revision_kind: RevisionKind::Substantive,
        contributors: vec!["Synthetic author".into()],
        notes: "Intentional synthetic wording change.".into(),
    }
}
fn change() -> GuardChange {
    GuardChange {
        previous: "The potter waited.".into(),
        replacement: Some("The potter waited by the kiln.".into()),
    }
}
fn session_path(store: &Store, name: &str) -> PathBuf {
    store.root().join(format!(".writing/sessions/{name}.json"))
}

fn setup() -> (
    tempfile::TempDir,
    Store,
    Session,
    WorkflowDefinition,
    ArtifactReference,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    store
        .import_experiment(&fixture("valid/chapter.json"))
        .unwrap();
    let definition = writing_workflow::definition(&root(), config()).unwrap();
    let shape = writing_workflow::prepare(&store, &root(), config(), "shape", None).unwrap();
    let id = store
        .record_candidate(
            &store.experiment(&shape).unwrap().prepare("shape").unwrap(),
            &metadata(),
            &body("original"),
        )
        .unwrap();
    let input = ArtifactReference {
        experiment_id: shape,
        candidate_id: id,
    };
    let mut session = Session::open(&store, "author", &definition).unwrap();
    session.bind(&store, &root(), config(), "model-a").unwrap();
    session
        .choose_pass(&definition, "voice", Some(input.clone()), &store)
        .unwrap();
    let experiment = session.prepare(&store, &root(), config()).unwrap();
    let id = store
        .record_candidate_with_context(
            &store
                .experiment(&experiment)
                .unwrap()
                .prepare("voice")
                .unwrap(),
            &metadata(),
            &body("original"),
            &CandidateContext {
                artifact_kind: Some("voice".into()),
                parent: Some(input),
                editorial_revision: None,
            },
        )
        .unwrap();
    session
        .record_output(&store, &definition, &id, "generate")
        .unwrap();
    (
        directory,
        store,
        session,
        definition,
        ArtifactReference {
            experiment_id: experiment,
            candidate_id: id,
        },
    )
}

fn cli(store: &Store, args: &[&str], input: &str, editor: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_blogctl"));
    command
        .args(["workflow", "--workdir"])
        .arg(store.root())
        .args(args)
        .env_remove("NOVELAI_API_TOKEN")
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .env("BLOGCTL_TEST_BODY", fixture("author/revised.md"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(editor) = editor {
        command.env("EDITOR", editor);
    }
    let mut process = command.spawn().unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    process.wait_with_output().unwrap()
}
fn success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn review_revision_policy_fork_acceptance_and_next_pass_survive_restart() {
    let (_directory, store, mut session, definition, generated) = setup();
    assert_eq!(session.review_target(), Some(&generated));
    assert!(session.selected().is_none());
    let working = session.begin_revision(&store).unwrap();
    for path in [&working.working_file, &working.sidecar] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    fs::write(&working.working_file, body("revised")).unwrap();
    let mut resumed = Session::read(&store, "author").unwrap();
    assert_eq!(
        resumed.working_revision().unwrap().file,
        working.working_file
    );
    assert!(resumed.accept_target(&store).is_err());
    assert!(resumed.advance(&definition).is_err());
    assert!(resumed.set_evidence(&store, vec![]).is_err());
    assert!(resumed
        .choose_pass(&definition, "line-edit", Some(generated.clone()), &store)
        .is_err());
    assert!(resumed.prepare(&store, &root(), config()).is_err());
    let edited = resumed
        .finish_revision(&store, &definition, revision())
        .unwrap();
    assert_eq!(
        store
            .candidate(&edited.experiment_id, &edited.candidate_id)
            .unwrap()
            .provenance
            .parent,
        Some(generated)
    );
    let old_path = session_path(&store, "author");
    let old = fs::read(&old_path).unwrap();
    let mut fork = resumed
        .fork_guards(&store, "reworded", vec![change()])
        .unwrap();
    assert_eq!(fs::read(&old_path).unwrap(), old);
    let updated = fork.bound_definition().unwrap();
    assert_ne!(updated.fingerprint(), definition.fingerprint());
    assert_eq!(
        updated.pass("line-edit").unwrap().protected_text(),
        ["The potter waited by the kiln."]
    );
    assert_eq!(
        definition.pass("line-edit").unwrap().protected_text(),
        ["The potter waited."]
    );
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(session_path(&store, "reworded")).unwrap()).unwrap();
    let archive = store.root().join(format!(
        ".writing/session-history/{}.json",
        json["policy_origin"]["archive_hash"].as_str().unwrap()
    ));
    assert_eq!(fs::read(&archive).unwrap(), old);
    assert_eq!(
        fs::metadata(&archive).unwrap().permissions().mode() & 0o777,
        0o400
    );
    assert!(json["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|attempt| attempt["workflow_hash"] == definition.fingerprint()));
    assert!(fork.selected().is_none());
    fork.relocate(&store, &root()).unwrap();
    assert_eq!(
        fork.bound_definition().unwrap().fingerprint(),
        updated.fingerprint()
    );
    fork.accept_target(&store).unwrap();
    fork.advance(&updated).unwrap();
    let mut final_resume = Session::read(&store, "reworded").unwrap();
    assert_eq!(final_resume.pass_name(), "line-edit");
    assert_eq!(final_resume.input(), Some(&edited));
    assert!(final_resume.review_target().is_none());
    let next = final_resume.prepare(&store, &root(), config()).unwrap();
    assert_eq!(store.experiment(&next).unwrap().seed, body("revised"));
    assert_eq!(fs::read(old_path).unwrap(), old);
    assert!(Session::names(&store).unwrap().contains(&"reworded".into()));
}

#[test]
fn legacy_journal_focus_and_registration_do_not_require_chat_or_newest_files() {
    let (_directory, store, _session, _definition, generated) = setup();
    let path = session_path(&store, "author");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("binding");
    value.as_object_mut().unwrap().remove("review_target");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let session = Session::read(&store, "author").unwrap();
    assert_eq!(session.review_target(), Some(&generated));
    assert!(session.bound_definition().is_err());
    assert!(cli(&store, &["conduct", "author"], "9\n", None)
        .status
        .code()
        .is_some_and(|code| code != 0));
    assert_eq!(fs::read(&path).unwrap(), before);
    let result = cli(
        &store,
        &[
            "conduct",
            "author",
            "--config-root",
            root().to_str().unwrap(),
            "--config",
            config().to_str().unwrap(),
            "--model",
            "model-a",
        ],
        "9\n",
        None,
    );
    success(&result);
    assert!(Session::read(&store, "author").unwrap().binding().is_some());
    let archive = store.root().join(format!(
        ".writing/session-history/{:x}.json",
        Sha256::digest(&before)
    ));
    assert_eq!(fs::read(archive).unwrap(), before);
}

#[test]
fn conductor_edits_and_forks_policy_without_selection_or_requests_until_explicit_next() {
    let (_directory, store, _session, _definition, _generated) = setup();
    let editor = format!("sh {}", fixture("author/editor.sh").display());
    let first = success(&cli(
        &store,
        &["conduct", "author", "--width", "66"],
        "edit\nSynthetic author\n9\n",
        Some(&editor),
    ));
    assert!(first.contains("protected-text-missing"));
    let session = Session::read(&store, "author").unwrap();
    assert!(session.working_revision().is_none());
    assert!(session.selected().is_none());
    let old = fs::read(session_path(&store, "author")).unwrap();
    let result = success(&cli(
        &store,
        &["conduct", "author"],
        "1\nd\nguards\nThe potter waited by the kiln.\nui-fork\nFORK POLICY\nn\n9\n",
        None,
    ));
    assert!(result.contains("Draft accepted and advanced. No provider request was made."));
    assert!(!result.contains("## Request snapshot (not manuscript)"));
    assert!(!result.contains("prompt_format_version"));
    assert_eq!(fs::read(session_path(&store, "author")).unwrap(), old);
    let fork = Session::read(&store, "ui-fork").unwrap();
    assert_eq!(fork.pass_name(), "line-edit");
    assert!(fork.selected().is_none());
    assert!(!store.root().join(".writing/provider-responses").exists());
    assert!(success(&cli(&store, &["sessions"], "", None)).contains("ui-fork"));
}

#[test]
fn interrupted_editor_save_and_abandon_preserve_working_files() {
    let (_directory, store, _session, _definition, _generated) = setup();
    let editor = format!("sh {}", fixture("author/abort-editor.sh").display());
    let output = success(&cli(
        &store,
        &["conduct", "author"],
        "edit\n9\n",
        Some(&editor),
    ));
    assert!(output.contains("working copy retained"));
    let working = Session::read(&store, "author")
        .unwrap()
        .working_revision()
        .unwrap()
        .file
        .clone();
    let before = fs::read(&working).unwrap();
    success(&cli(
        &store,
        &["conduct", "author"],
        "a\nabandon\nno\n9\n",
        None,
    ));
    assert!(Session::read(&store, "author")
        .unwrap()
        .working_revision()
        .is_some());
    success(&cli(
        &store,
        &["conduct", "author"],
        "abandon\nABANDON\n9\n",
        None,
    ));
    assert_eq!(fs::read(&working).unwrap(), before);
    assert!(Session::read(&store, "author")
        .unwrap()
        .working_revision()
        .is_none());
    let output = success(&cli(&store, &["conduct", "author"], "edit\n9\n", None));
    assert!(output.contains("Set VISUAL or EDITOR"));
    let working = Session::read(&store, "author")
        .unwrap()
        .working_revision()
        .unwrap()
        .file
        .clone();
    fs::write(&working, body("revised")).unwrap();
    let output = success(&cli(
        &store,
        &["conduct", "author"],
        "1\nd\nsave\nSynthetic author\n9\n",
        None,
    ));
    assert!(output.contains("Pending working text"));
    assert!(output.contains("Checkpoint under review"));
    let session = Session::read(&store, "author").unwrap();
    assert!(session.working_revision().is_none());
    assert!(session.selected().is_none());
    assert_eq!(session.contributor(), Some("Synthetic author"));
    assert!(!store.root().join(".writing/provider-responses").exists());
}

#[test]
fn status_and_listing_are_read_only_even_with_unsupported_prototype_session() {
    let (_directory, store, _session, _definition, _generated) = setup();
    fs::copy(
        fixture("author/legacy-unsupported.json"),
        session_path(&store, "prototype"),
    )
    .unwrap();
    let path = session_path(&store, "author");
    let handoff = path.with_extension("handoff.md");
    let before = (fs::read(&path).unwrap(), fs::read(&handoff).unwrap());
    let status = success(&cli(&store, &["status", "author"], "", None));
    assert!(status.contains("Review draft (not acceptance)"));
    let raw: serde_json::Value = serde_json::from_str(&success(&cli(
        &store,
        &["status", "author", "--json"],
        "",
        None,
    )))
    .unwrap();
    assert!(raw["selected"].is_null());
    let listed = success(&cli(&store, &["sessions"], "", None));
    assert!(listed.contains("prototype: unavailable"));
    assert_eq!(
        (fs::read(path).unwrap(), fs::read(handoff).unwrap()),
        before
    );
    assert!(!store.root().join(".writing/provider-responses").exists());
}

#[test]
fn protection_decisions_are_explicit_validated_and_cannot_overwrite_sessions() {
    let (_directory, store, mut session, definition, generated) = setup();
    assert!(session
        .fork_guards(
            &store,
            "generated",
            vec![GuardChange {
                previous: "The potter waited.".into(),
                replacement: None
            }]
        )
        .is_err());
    let working = session.begin_revision(&store).unwrap();
    fs::write(&working.working_file, body("revised")).unwrap();
    session
        .finish_revision(&store, &definition, revision())
        .unwrap();
    for changes in [
        vec![],
        vec![change(), change()],
        vec![GuardChange {
            previous: "missing".into(),
            replacement: None,
        }],
        vec![GuardChange {
            previous: "The potter waited.".into(),
            replacement: Some("not in the draft".into()),
        }],
        vec![GuardChange {
            previous: "The potter waited.".into(),
            replacement: Some(String::new()),
        }],
    ] {
        assert!(session.fork_guards(&store, "invalid", changes).is_err());
    }
    assert!(session
        .fork_guards(&store, "author", vec![change()])
        .is_err());
    let removed = definition
        .change_guards(
            &[GuardChange {
                previous: "The potter waited.".into(),
                replacement: None,
            }],
            &body("revised"),
        )
        .unwrap();
    assert!(removed.pass("voice").unwrap().protected_text().is_empty());
    assert!(definition
        .change_guards(
            &[GuardChange {
                previous: "The potter waited.".into(),
                replacement: Some("unapproved".into())
            }],
            "unapproved"
        )
        .is_err());
    assert!(definition
        .change_guards(
            &[GuardChange {
                previous: "The potter waited.".into(),
                replacement: Some("The potter waited.".into())
            }],
            &body("original")
        )
        .is_err());
    let fork = session
        .fork_guards(&store, "valid", vec![change()])
        .unwrap();
    assert!(session
        .fork_guards(&store, "valid", vec![change()])
        .is_err());
    assert!(fork.selected().is_none());
    assert!(store
        .candidate(&generated.experiment_id, &generated.candidate_id)
        .is_ok());
}

#[test]
fn unsafe_or_stale_working_files_fail_closed_without_discarding_intent() {
    let (_directory, store, mut session, definition, _generated) = setup();
    let working = session.begin_revision(&store).unwrap();
    assert_eq!(
        session.begin_revision(&store).unwrap().working_file,
        working.working_file
    );
    fs::set_permissions(&working.working_file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(session
        .finish_revision(&store, &definition, revision())
        .is_err());
    fs::set_permissions(&working.working_file, fs::Permissions::from_mode(0o600)).unwrap();
    let mut sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(&working.sidecar).unwrap()).unwrap();
    sidecar["candidate_hash"] = "0".repeat(64).into();
    fs::write(&working.sidecar, serde_json::to_vec(&sidecar).unwrap()).unwrap();
    assert!(session.begin_revision(&store).is_err());
    assert!(session
        .finish_revision(&store, &definition, revision())
        .is_err());
    assert!(Session::read(&store, "author")
        .unwrap()
        .working_revision()
        .is_some());
    assert!(session.selected().is_none());
    assert!(!store.root().join(".writing/provider-responses").exists());
}

#[test]
fn unsafe_session_history_and_binding_changes_are_rejected() {
    let (_directory, store, mut session, definition, _generated) = setup();
    assert!(session.set_model("").is_err());
    assert!(session.set_model("model\n").is_err());
    assert!(session.set_contributor("").is_err());
    assert!(session.set_contributor("author\n").is_err());
    session.set_model("model-b").unwrap();
    assert_eq!(session.binding().unwrap().model, "model-b");
    let working = session.begin_revision(&store).unwrap();
    fs::write(working.working_file, body("revised")).unwrap();
    session
        .finish_revision(&store, &definition, revision())
        .unwrap();
    session
        .fork_guards(&store, "child", vec![change()])
        .unwrap();
    let child_path = session_path(&store, "child");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&child_path).unwrap()).unwrap();
    let archive = store.root().join(format!(
        ".writing/session-history/{}.json",
        value["policy_origin"]["archive_hash"].as_str().unwrap()
    ));
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&archive, b"corrupt").unwrap();
    assert!(Session::read(&store, "child").is_err());
    let path = session_path(&store, "author");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["binding"]["workflow_json"] = fs::read_to_string(fixture("workflow/valid/workflow.json"))
        .unwrap()
        .into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(Session::read(&store, "author").is_err());
}

#[test]
fn valid_but_different_sidecar_parent_and_unregistered_context_root_are_refused() {
    let (_directory, store, mut session, definition, _generated) = setup();
    let other_root = tempfile::tempdir().unwrap();
    assert!(session
        .prepare(&store, other_root.path(), config())
        .is_err());
    let input = session.input().unwrap().clone();
    let working = session.begin_revision(&store).unwrap();
    let before = fs::read(session_path(&store, "author")).unwrap();
    let mut sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(&working.sidecar).unwrap()).unwrap();
    sidecar["parent"] = serde_json::to_value(&input).unwrap();
    sidecar["candidate_hash"] = format!(
        "{:x}",
        Sha256::digest(
            store
                .read_candidate(&input.experiment_id, &input.candidate_id)
                .unwrap()
                .as_bytes()
        )
    )
    .into();
    sidecar["body_hash"] = format!(
        "{:x}",
        Sha256::digest(
            store
                .candidate(&input.experiment_id, &input.candidate_id)
                .unwrap()
                .body
                .as_bytes()
        )
    )
    .into();
    fs::write(&working.sidecar, serde_json::to_vec(&sidecar).unwrap()).unwrap();
    assert_eq!(
        store
            .inspect_revision(&working.working_file)
            .unwrap()
            .parent,
        input
    );
    assert!(session.begin_revision(&store).is_err());
    assert!(session
        .finish_revision(&store, &definition, revision())
        .is_err());
    assert_eq!(fs::read(session_path(&store, "author")).unwrap(), before);
}

#[test]
fn pending_paths_and_handoff_symlinks_never_redirect_writes() {
    let (_directory, store, mut session, _definition, _generated) = setup();
    session.begin_revision(&store).unwrap();
    let path = session_path(&store, "author");
    let original = fs::read(&path).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    value["working_revision"]["file"] = "/tmp/outside-author-copy.md".into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(Session::read(&store, "author").is_err());
    fs::write(&path, &original).unwrap();
    let external = store.root().join("external.md");
    fs::write(&external, b"unchanged").unwrap();
    let handoff = path.with_extension("handoff.md");
    fs::remove_file(&handoff).unwrap();
    symlink(&external, &handoff).unwrap();
    assert!(session.abandon_revision().is_err());
    assert_eq!(fs::read(&external).unwrap(), b"unchanged");
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn using_review_as_input_is_explicit_and_does_not_accept_advance_or_request() {
    let (_directory, store, mut session, definition, generated) = setup();
    let input = session.input().unwrap().clone();
    assert!(session.focus(&store, &definition, input).is_err());
    let output = success(&cli(&store, &["conduct", "author"], "u\n2\n9\n", None));
    assert!(output.contains("No request, acceptance, or advancement occurred"));
    let resumed = Session::read(&store, "author").unwrap();
    assert_eq!(resumed.pass_name(), "voice");
    assert_eq!(resumed.input(), Some(&generated));
    assert!(resumed.selected().is_none());
    assert!(!store.root().join(".writing/provider-responses").exists());
}

#[test]
fn review_remains_readable_when_original_context_root_is_unavailable() {
    let (_directory, store, _session, _definition, generated) = setup();
    let path = session_path(&store, "author");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["binding"]["config_root"] = store
        .root()
        .join("missing-consumer")
        .to_str()
        .unwrap()
        .into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let output = success(&cli(&store, &["conduct", "author"], "1\n9\n", None));
    assert!(output.contains("The potter waited."));
    let mut session = Session::read(&store, "author").unwrap();
    assert_eq!(session.review_target(), Some(&generated));
    assert!(session
        .prepare(&store, &store.root().join("missing-consumer"), config())
        .is_err());
    success(&cli(
        &store,
        &[
            "conduct",
            "author",
            "--config-root",
            root().to_str().unwrap(),
        ],
        "9\n",
        None,
    ));
    let relocated = Session::read(&store, "author").unwrap();
    assert_eq!(
        relocated.binding().unwrap().config_root,
        root().canonicalize().unwrap()
    );
    assert_eq!(relocated.review_target(), Some(&generated));
    assert!(!store.root().join(".writing/provider-responses").exists());
}

#[test]
fn unchanged_editor_visit_needs_no_identity_or_new_checkpoint() {
    let (_directory, store, _session, _definition, generated) = setup();
    let before = store.candidates(&generated.experiment_id).unwrap();
    let editor = format!("sh {}", fixture("author/no-change-editor.sh").display());
    let output = success(&cli(
        &store,
        &["conduct", "author"],
        "edit\n9\n",
        Some(&editor),
    ));
    assert!(output.contains("No text changes"));
    assert!(!output.contains("Contributor label"));
    let session = Session::read(&store, "author").unwrap();
    assert!(session.working_revision().is_none());
    assert!(session.contributor().is_none());
    assert_eq!(session.review_target(), Some(&generated));
    assert_eq!(store.candidates(&generated.experiment_id).unwrap(), before);
}

#[test]
fn editor_does_not_inherit_provider_credentials() {
    let (_directory, store, _session, _definition, _generated) = setup();
    let mut command = Command::new(env!("CARGO_BIN_EXE_blogctl"));
    command
        .args(["workflow", "--workdir"])
        .arg(store.root())
        .args(["conduct", "author"])
        .env_remove("VISUAL")
        .env(
            "EDITOR",
            format!("sh {}", fixture("author/editor-no-secrets.sh").display()),
        )
        .env("BLOGCTL_TEST_BODY", fixture("author/revised.md"))
        .env("NOVELAI_API_TOKEN", "synthetic-unused-secret")
        .env("OPENROUTER_API_KEY", "synthetic-unused-secret")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"edit\nSynthetic author\n9\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(success(&output).contains("Checkpoint under review"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Provider credentials reached"));
    assert!(!store.root().join(".writing/provider-responses").exists());
}

#[test]
fn warnings_need_explicit_override_and_final_pass_never_implicitly_advances() {
    let (_directory, store, mut session, definition, generated) = setup();
    let id = store
        .record_candidate(
            &store
                .experiment(&generated.experiment_id)
                .unwrap()
                .prepare("voice")
                .unwrap(),
            &metadata(),
            &body("forbidden"),
        )
        .unwrap();
    session
        .focus(
            &store,
            &definition,
            ArtifactReference {
                experiment_id: generated.experiment_id.clone(),
                candidate_id: id,
            },
        )
        .unwrap();
    success(&cli(&store, &["conduct", "author"], "a\nno\n9\n", None));
    assert!(Session::read(&store, "author")
        .unwrap()
        .selected()
        .is_none());
    success(&cli(
        &store,
        &["conduct", "author"],
        "a\nACCEPT WITH FINDINGS\n9\n",
        None,
    ));
    let mut session = Session::read(&store, "author").unwrap();
    assert!(session.selected().is_some());
    assert_eq!(session.pass_name(), "voice");
    session
        .choose_pass(&definition, "final-draft", Some(generated), &store)
        .unwrap();
    let experiment = session.prepare(&store, &root(), config()).unwrap();
    let id = store
        .record_candidate(
            &store
                .experiment(&experiment)
                .unwrap()
                .prepare("final-draft")
                .unwrap(),
            &metadata(),
            &body("original"),
        )
        .unwrap();
    session
        .record_output(&store, &definition, &id, "import")
        .unwrap();
    let output = success(&cli(&store, &["conduct", "author"], "n\n9\n", None));
    assert!(output.contains("final pass reached"));
    let session = Session::read(&store, "author").unwrap();
    assert!(session.selected().is_none());
    assert_eq!(session.pass_name(), "final-draft");
}

#[test]
fn protection_dialog_cancellation_and_no_credentials_run_keep_review_unselected() {
    let (_directory, store, mut session, definition, _generated) = setup();
    let working = session.begin_revision(&store).unwrap();
    fs::write(working.working_file, body("revised")).unwrap();
    session
        .finish_revision(&store, &definition, revision())
        .unwrap();
    let old = fs::read(session_path(&store, "author")).unwrap();
    let output = success(&cli(&store, &["conduct", "author"], "guards\n\n9\n", None));
    assert!(output.contains("Protection change cancelled"));
    assert_eq!(fs::read(session_path(&store, "author")).unwrap(), old);
    let output = success(&cli(
        &store,
        &["conduct", "author"],
        "guards\nThe potter waited by the kiln.\n\nno\n9\n",
        None,
    ));
    assert!(output.contains("Protection change cancelled"));
    let output = cli(&store, &["run-pass", "author"], "", None);
    assert!(!output.status.success());
    let session = Session::read(&store, "author").unwrap();
    assert!(session.selected().is_none());
    assert!(!store.root().join(".writing/provider-responses").exists());
    assert!(!cli(
        &store,
        &["conduct", "author", "--config", config().to_str().unwrap()],
        "",
        None
    )
    .status
    .success());
}

#[test]
fn review_activity_does_not_revoke_or_replace_an_existing_explicit_selection() {
    let (_directory, store, mut session, definition, generated) = setup();
    session.select(&store, &generated.candidate_id).unwrap();
    let id = store
        .record_candidate(
            &store
                .experiment(&generated.experiment_id)
                .unwrap()
                .prepare("voice")
                .unwrap(),
            &metadata(),
            &body("forbidden"),
        )
        .unwrap();
    session
        .record_output(&store, &definition, &id, "generate")
        .unwrap();
    assert_eq!(session.selected(), Some(&generated));
    let alternate = ArtifactReference {
        experiment_id: generated.experiment_id.clone(),
        candidate_id: id,
    };
    session.focus(&store, &definition, alternate).unwrap();
    assert_eq!(session.selected(), Some(&generated));
    let working = session.begin_revision(&store).unwrap();
    fs::write(working.working_file, body("revised")).unwrap();
    session
        .finish_revision(&store, &definition, revision())
        .unwrap();
    assert_eq!(session.selected(), Some(&generated));
    assert_ne!(session.review_target(), session.selected());
    let child = session
        .fork_guards(&store, "changed-policy", vec![change()])
        .unwrap();
    assert!(child.selected().is_none());
    assert_eq!(
        Session::read(&store, "author").unwrap().selected(),
        Some(&generated)
    );
}

#[test]
fn invalid_store_boundary_creates_no_sessions_and_insecure_session_mode_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    fs::copy(root().join(config()), directory.path().join("policy.json")).unwrap();
    let output = cli(
        &store,
        &[
            "conduct",
            "unsafe",
            "--config-root",
            directory.path().to_str().unwrap(),
            "--config",
            "policy.json",
            "--model",
            "model-a",
        ],
        "9\n",
        None,
    );
    assert!(!output.status.success());
    assert!(!directory.path().join(".writing").exists());
    let (_directory, store, _session, _definition, _generated) = setup();
    let path = session_path(&store, "author");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Session::read(&store, "author").is_err());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
}

#[test]
fn mismatched_policy_cannot_tag_assessments_import_revisions_or_advance() {
    let (_directory, store, mut session, definition, generated) = setup();
    let different = definition
        .change_guards(
            &[GuardChange {
                previous: "The potter waited.".into(),
                replacement: None,
            }],
            &body("original"),
        )
        .unwrap();
    let before = fs::read(session_path(&store, "author")).unwrap();
    assert!(session
        .focus(&store, &different, generated.clone())
        .is_err());
    assert!(session
        .record_output(&store, &different, &generated.candidate_id, "import")
        .is_err());
    assert!(session
        .choose_pass(&different, "line-edit", Some(generated.clone()), &store)
        .is_err());
    assert_eq!(fs::read(session_path(&store, "author")).unwrap(), before);
    session.accept_target(&store).unwrap();
    assert!(session.advance(&different).is_err());
    assert_eq!(session.pass_name(), "voice");
    let working = session.begin_revision(&store).unwrap();
    fs::write(working.working_file, body("revised")).unwrap();
    let candidates = store.candidates(&generated.experiment_id).unwrap();
    assert!(session
        .finish_revision(&store, &different, revision())
        .is_err());
    assert_eq!(
        store.candidates(&generated.experiment_id).unwrap(),
        candidates
    );
    assert!(session.working_revision().is_some());
}

#[test]
fn detached_editorial_checkpoint_can_be_attached_explicitly_not_as_acceptance() {
    let (_directory, store, session, _definition, generated) = setup();
    let path = store.root().join("external-working.md");
    store
        .prepare_revision(&generated.experiment_id, &generated.candidate_id, &path)
        .unwrap();
    fs::write(&path, body("revised")).unwrap();
    let id = store.import_revision(&path, revision()).unwrap();
    let reference = format!("{}/{}", generated.experiment_id, id);
    let output = success(&cli(&store, &["focus", "author", &reference], "", None));
    assert!(output.contains("protected-text-missing"));
    assert!(session.selected().is_none());
    let resumed = Session::read(&store, "author").unwrap();
    assert_eq!(resumed.review_target().unwrap().candidate_id, id);
    assert!(resumed.selected().is_none());
    assert_eq!(resumed.pass_name(), "voice");
}
