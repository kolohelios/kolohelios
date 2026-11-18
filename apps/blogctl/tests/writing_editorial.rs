//! Public synthetic fixtures exercise the workflow contract without a provider.
use std::fs;
use std::io::Write;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use blogctl::writing::{self, ArtifactReference, GenerationMetadata, Store};
use blogctl::writing_assessment::{self, AssessmentStatus};
use blogctl::writing_session::Session;
use blogctl::writing_workflow::{self, WorkflowDefinition};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing")
        .join(name)
}

fn setup() -> (
    tempfile::TempDir,
    Store,
    WorkflowDefinition,
    ArtifactReference,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    store
        .import_experiment(&fixture("valid/chapter.json"))
        .unwrap();
    let root = fixture("workflow");
    let config = Path::new("valid/authoritative-workflow.json");
    let definition = writing_workflow::definition(&root, config).unwrap();
    let experiment = writing_workflow::prepare(&store, &root, config, "shape", None).unwrap();
    let metadata: GenerationMetadata = writing::read_json(&fixture("generation.json")).unwrap();
    let candidate = store
        .record_candidate(
            &store
                .experiment(&experiment)
                .unwrap()
                .prepare("shape")
                .unwrap(),
            &metadata,
            &fs::read_to_string(fixture("assessment/valid/preserved.md")).unwrap(),
        )
        .unwrap();
    (
        directory,
        store,
        definition,
        ArtifactReference {
            experiment_id: experiment,
            candidate_id: candidate,
        },
    )
}

fn session(store: &Store, definition: &WorkflowDefinition, input: &ArtifactReference) -> Session {
    let mut session = Session::open(store, "editorial", definition).unwrap();
    session
        .choose_pass(definition, "voice", Some(input.clone()), store)
        .unwrap();
    session
        .prepare(
            store,
            &fixture("workflow"),
            Path::new("valid/authoritative-workflow.json"),
        )
        .unwrap();
    session
}

fn record(store: &Store, session: &Session, body: &Path) -> String {
    let metadata: GenerationMetadata = writing::read_json(&fixture("generation.json")).unwrap();
    let experiment = session.experiment().unwrap();
    store
        .record_candidate(
            &store
                .experiment(experiment)
                .unwrap()
                .prepare("voice")
                .unwrap(),
            &metadata,
            &fs::read_to_string(body).unwrap(),
        )
        .unwrap()
}

#[test]
fn authoritative_preparation_never_sends_historical_seed_or_source_instructions() {
    let (_directory, store, definition, input) = setup();
    let session = session(&store, &definition, &input);
    let compiled = store.experiment(session.experiment().unwrap()).unwrap();
    let original = store.experiment("synthetic-chapter").unwrap();
    let selected = store
        .candidate(&input.experiment_id, &input.candidate_id)
        .unwrap();
    assert_eq!(compiled.seed, selected.body);
    assert_eq!(compiled.target, original.target);
    assert!(!compiled.instructions.contains(&original.instructions));
    let request = blogctl::novelai::PreparedGeneration::new(
        compiled.prepare("voice").unwrap(),
        "model-a".into(),
        None,
        None,
    )
    .unwrap();
    let wire: serde_json::Value = serde_json::from_str(&request.preview().unwrap()).unwrap();
    let messages = wire["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["content"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!messages.contains(original.seed.trim()));
    assert!(!messages.contains(&original.instructions));
    assert!(messages.contains("The selected draft is authoritative"));
    assert!(messages.contains("Output constraints:"));
    assert_eq!(messages.matches(&selected.body).count(), 1);
    assert!(compiled.source_material.contains("Voice profile"));
    assert!(!compiled.source_material.contains("Voice sample"));
    assert!(session.selected().is_none());
}

#[test]
fn malformed_policy_and_missing_protected_input_fail_before_preparation() {
    let (_directory, store, _definition, input) = setup();
    for name in [
        "authoritative-without-input",
        "conflicting-constraints",
        "protected-with-source",
    ] {
        assert!(writing_workflow::definition(
            &fixture("workflow"),
            Path::new(&format!("invalid/{name}.json"))
        )
        .is_err());
    }
    let source = store
        .experiment(&input.experiment_id)
        .unwrap()
        .prepare("shape")
        .unwrap();
    let metadata: GenerationMetadata = writing::read_json(&fixture("generation.json")).unwrap();
    let wrong = store
        .record_candidate(
            &source,
            &metadata,
            &fs::read_to_string(fixture("assessment/invalid/missing-protected.md")).unwrap(),
        )
        .unwrap();
    let count = store.experiments().unwrap().len();
    assert!(writing_workflow::prepare(
        &store,
        &fixture("workflow"),
        Path::new("valid/authoritative-workflow.json"),
        "voice",
        Some(&ArtifactReference {
            candidate_id: wrong,
            experiment_id: input.experiment_id
        })
    )
    .is_err());
    assert_eq!(store.experiments().unwrap().len(), count);
}

#[test]
fn every_output_is_assessed_without_becoming_an_accepted_or_selected_pass() {
    let (directory, store, definition, input) = setup();
    let mut session = session(&store, &definition, &input);
    for category in ["valid", "invalid"] {
        for entry in fs::read_dir(fixture(&format!("assessment/{category}"))).unwrap() {
            let body = entry.unwrap().path();
            let candidate = record(&store, &session, &body);
            let original = store
                .read_candidate(session.experiment().unwrap(), &candidate)
                .unwrap();
            let assessment = session
                .record_output(&store, &definition, &candidate, "import")
                .unwrap();
            assert_eq!(
                assessment.status,
                if category == "valid" {
                    AssessmentStatus::NeedsReview
                } else {
                    AssessmentStatus::ConstraintViolation
                },
                "{}",
                body.display()
            );
            assert!(session.selected().is_none());
            assert_eq!(session.pass_name(), "voice");
            assert_eq!(
                store
                    .read_candidate(session.experiment().unwrap(), &candidate)
                    .unwrap(),
                original
            );
        }
    }
    let folder = directory.path().join(".writing/sessions");
    for name in ["editorial.json", "editorial.handoff.md"] {
        assert_eq!(
            fs::metadata(folder.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let handoff = fs::read_to_string(folder.join("editorial.handoff.md")).unwrap();
    assert!(handoff.contains(&input.candidate_id));
    assert!(handoff.contains("protected-text-missing"));
    assert!(handoff.contains("forbidden-output"));
    assert!(handoff.contains("No recorded output is selected automatically"));
    let resumed = Session::open(&store, "editorial", &definition).unwrap();
    assert!(resumed.selected().is_none());
}

#[test]
fn standalone_assessment_is_credential_free_read_only_and_explicit() {
    let (directory, store, definition, input) = setup();
    let session = session(&store, &definition, &input);
    let id = record(
        &store,
        &session,
        &fixture("assessment/invalid/forbidden.md"),
    );
    let path = directory.path().join(".writing/sessions/editorial.json");
    let before = fs::read(&path).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_blogctl"))
        .env_clear()
        .current_dir(directory.path())
        .args(["workflow", "--workdir"])
        .arg(directory.path())
        .args([
            "assess",
            "voice",
            &format!("{}/{id}", session.experiment().unwrap()),
            "--config-root",
        ])
        .arg(fixture("workflow"))
        .args(["--config", "valid/authoritative-workflow.json", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "constraint-violation");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!directory
        .path()
        .join(".writing/provider-responses")
        .exists());
}

#[test]
fn credential_failure_is_recorded_without_a_provider_request_or_selection() {
    let (directory, _store, _definition, _input) = setup();
    let mut child = Command::new(env!("CARGO_BIN_EXE_blogctl"))
        .env_clear()
        .current_dir(directory.path())
        .args(["workflow", "--workdir"])
        .arg(directory.path())
        .args(["conduct", "failed-attempt", "--config-root"])
        .arg(fixture("workflow"))
        .args([
            "--config",
            "valid/authoritative-workflow.json",
            "--model",
            "model-a",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"3\n9\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let path = directory
        .path()
        .join(".writing/sessions/failed-attempt.json");
    let state: serde_json::Value = writing::read_json(&path).unwrap();
    assert_eq!(state["attempts"].as_array().unwrap().len(), 1);
    assert!(state["attempts"][0]["failure"]
        .as_str()
        .unwrap()
        .contains("NOVELAI_API_TOKEN is missing"));
    assert!(state["selected"].is_null());
    assert!(!directory
        .path()
        .join(".writing/provider-responses")
        .exists());
    let handoff = fs::read_to_string(path.with_extension("handoff.md")).unwrap();
    assert!(handoff.contains("NOVELAI_API_TOKEN is missing"));
}

#[test]
fn run_pass_is_one_explicit_action_and_records_credential_failure_without_retry() {
    let (directory, _store, _definition, _input) = setup();
    let output = Command::new(env!("CARGO_BIN_EXE_blogctl"))
        .env_clear()
        .current_dir(directory.path())
        .args(["workflow", "--workdir"])
        .arg(directory.path())
        .args(["run-pass", "one-action", "--config-root"])
        .arg(fixture("workflow"))
        .args([
            "--config",
            "valid/authoritative-workflow.json",
            "--model",
            "model-a",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let state: serde_json::Value =
        writing::read_json(&directory.path().join(".writing/sessions/one-action.json")).unwrap();
    assert_eq!(state["pass"], "shape");
    assert_eq!(state["attempts"].as_array().unwrap().len(), 1);
    assert!(state["selected"].is_null());
    assert!(!directory
        .path()
        .join(".writing/provider-responses")
        .exists());
}

#[test]
fn handoff_symlinks_are_refused_without_rewriting_session_or_external_files() {
    let (directory, store, definition, input) = setup();
    let mut session = session(&store, &definition, &input);
    let candidate = record(&store, &session, &fixture("assessment/valid/preserved.md"));
    let path = directory.path().join(".writing/sessions/editorial.json");
    let before = fs::read(&path).unwrap();
    let handoff = path.with_extension("handoff.md");
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), &before).unwrap();
    fs::remove_file(&handoff).unwrap();
    symlink(outside.path(), handoff).unwrap();
    assert!(Session::open(&store, "editorial", &definition).is_err());
    let error = session
        .record_output(&store, &definition, &candidate, "generate")
        .unwrap_err();
    assert!(error.to_string().contains(&candidate));
    assert!(store
        .candidate(session.experiment().unwrap(), &candidate)
        .is_ok());
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(fs::read(outside.path()).unwrap(), before);
}

#[test]
fn provider_status_and_reasoning_are_not_editorial_acceptance() {
    let (_directory, store, definition, input) = setup();
    let session = session(&store, &definition, &input);
    let id = record(&store, &session, &fixture("assessment/valid/preserved.md"));
    let mut document = store.candidate(session.experiment().unwrap(), &id).unwrap();
    document.provenance.provider = "novelai".into();
    document
        .provenance
        .parameters
        .insert("finish_reason".into(), "length".into());
    document
        .provenance
        .parameters
        .insert("reasoning_content".into(), "Synthetic reasoning".into());
    let assessment = writing_assessment::assess(definition.pass("voice").unwrap(), &document);
    assert_eq!(assessment.status, AssessmentStatus::ConstraintViolation);
    assert!(assessment
        .findings
        .iter()
        .any(|finding| finding.code == "completion-not-established"));
    assert!(assessment
        .findings
        .iter()
        .any(|finding| finding.code == "provider-reasoning"));
}
