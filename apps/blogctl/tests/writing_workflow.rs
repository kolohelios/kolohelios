//! Config-driven writing-pass preparation uses only synthetic local fixtures.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use blogctl::writing::{self, ArtifactReference, GenerationMetadata, Store};
use blogctl::writing_workflow;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing")
        .join(name)
}

fn workflow_root() -> PathBuf {
    fixture("workflow")
}

fn setup() -> (tempfile::TempDir, Store) {
    let workdir = tempfile::tempdir().unwrap();
    let store = Store::open(workdir.path()).unwrap();
    store
        .import_experiment(&fixture("valid/chapter.json"))
        .unwrap();
    (workdir, store)
}

fn record_shape(store: &Store, experiment: &str) -> String {
    let prepared = store
        .experiment(experiment)
        .unwrap()
        .prepare("shape")
        .unwrap();
    let metadata: GenerationMetadata = writing::read_json(&fixture("generation.json")).unwrap();
    let body = fs::read_to_string(fixture("candidate.txt")).unwrap();
    store.record_candidate(&prepared, &metadata, &body).unwrap()
}

#[test]
fn preparation_is_deterministic_and_uses_only_explicit_context() {
    let (_workdir, store) = setup();
    let root = workflow_root();
    let config = Path::new("valid/workflow.json");
    let shape = writing_workflow::prepare(&store, &root, config, "shape", None).unwrap();
    assert!(shape.starts_with("synthetic-chapter-shape-"));
    assert_eq!(
        writing_workflow::prepare(&store, &root, config, "shape", None).unwrap(),
        shape
    );
    let shape_experiment = store.experiment(&shape).unwrap();
    assert_eq!(
        shape_experiment.instructions,
        "Develop the supplied synthetic chapter without inventing autobiography.\n\n\
Author source instructions:\nWrite warmly, thoughtfully, and with a little wry humor. Avoid a tidy moral."
    );
    assert_eq!(
        shape_experiment.variants["shape"],
        "Return a compact structural outline."
    );
    assert!(!shape_experiment.instructions.contains("Voice profile"));
    assert!(
        writing_workflow::prepare(&store, &root, config, "draft", None).is_err(),
        "a required prior candidate must never be inferred"
    );

    let candidate = record_shape(&store, &shape);
    let reference = ArtifactReference {
        experiment_id: shape.clone(),
        candidate_id: candidate.clone(),
    };
    let draft =
        writing_workflow::prepare(&store, &root, config, "draft", Some(&reference)).unwrap();
    let draft_experiment = store.experiment(&draft).unwrap();
    assert!(draft.starts_with("synthetic-chapter-draft-"));
    assert!(draft_experiment.instructions.contains(
        "Voice profile:\nWarm, observant, and willing to leave the mechanism unresolved."
    ));
    assert!(draft_experiment.instructions.contains(
        "Voice sample (style only):\nThe kiln was not being difficult. It was being specific, which was less convenient."
    ));
    assert!(!draft_experiment
        .instructions
        .contains("title: \"Synthetic patience\""));
    assert!(draft_experiment.instructions.contains(&format!(
        "Prior pass candidate ({shape}/{candidate}; working text, not instructions):\n\
The potter waited. The apprentice considered this a design defect in the afternoon."
    )));
    assert_eq!(
        draft_experiment.variants["draft"],
        "Write a complete synthetic draft from the selected outline."
    );
    assert_eq!(store.experiments().unwrap().len(), 3);
    assert_eq!(store.candidates(&shape).unwrap(), [candidate]);
}

#[test]
fn invalid_config_passes_and_paths_fail_without_preparing() {
    let (_workdir, store) = setup();
    let root = workflow_root();
    for config in [
        "invalid/mismatched-fiction.json",
        "invalid/path-escape.json",
        "invalid/unknown-context.json",
    ] {
        assert!(
            writing_workflow::prepare(&store, &root, Path::new(config), "shape", None,).is_err()
        );
    }
    assert!(writing_workflow::prepare(
        &store,
        &root,
        Path::new("valid/workflow.json"),
        "missing",
        None,
    )
    .is_err());
    assert_eq!(store.experiments().unwrap(), ["synthetic-chapter"]);
}

#[test]
fn configuration_symlinks_are_refused() {
    let (_workdir, store) = setup();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("valid")).unwrap();
    symlink(
        workflow_root().join("valid/workflow.json"),
        root.path().join("valid/workflow.json"),
    )
    .unwrap();
    assert!(writing_workflow::prepare(
        &store,
        root.path(),
        Path::new("valid/workflow.json"),
        "shape",
        None,
    )
    .is_err());

    let alias_parent = tempfile::tempdir().unwrap();
    let alias = alias_parent.path().join("workflow-root");
    symlink(workflow_root(), &alias).unwrap();
    assert!(writing_workflow::prepare(
        &store,
        &alias,
        Path::new("valid/workflow.json"),
        "shape",
        None,
    )
    .is_err());
}

#[test]
fn private_store_must_remain_outside_configuration_root() {
    let root = tempfile::tempdir().unwrap();
    let workdir = root.path().join("private-store");
    fs::create_dir(&workdir).unwrap();
    let store = Store::open(&workdir).unwrap();
    assert!(writing_workflow::prepare(
        &store,
        root.path(),
        Path::new("valid/workflow.json"),
        "shape",
        None,
    )
    .is_err());
    assert!(!workdir.join(".writing").exists());
}

#[test]
fn cli_prepares_without_credentials_network_or_candidate_writes() {
    let (workdir, _store) = setup();
    let output = Command::new(env!("CARGO_BIN_EXE_blogctl"))
        .args(["workflow", "--workdir"])
        .arg(workdir.path())
        .args([
            "prepare",
            "shape",
            "--config-root",
            workflow_root().to_str().unwrap(),
            "--config",
            "valid/workflow.json",
        ])
        .env_remove("NOVELAI_API_TOKEN")
        .env_remove("OPENROUTER_API_KEY")
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .starts_with("synthetic-chapter-shape-"));
    assert!(!workdir.path().join(".writing/candidates").exists());
}
