//! Evidence packs use only synthetic local fixtures and make no network requests.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use blogctl::writing::{self, ArtifactReference, GenerationMetadata, Store};
use blogctl::writing_session::Session;
use blogctl::writing_workflow;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing")
        .join(name)
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
fn evidence_import_is_content_addressed_immutable_and_verifiable() {
    let (workdir, store) = setup();
    let source = fixture("evidence/valid/review-research.json");
    let id = store.import_evidence_pack(&source).unwrap();
    assert!(id.starts_with("evidence-"));
    assert_eq!(store.import_evidence_pack(&source).unwrap(), id);
    assert_eq!(store.evidence_packs().unwrap(), std::slice::from_ref(&id));

    let pack = store.evidence_pack(&id).unwrap();
    assert_eq!(pack.evidence_pack_id, id);
    assert_eq!(pack.entries.len(), 2);
    assert_eq!(pack.hash_algorithm, "sha256");
    assert_eq!(pack.content_hash.len(), 64);
    let path = workdir.path().join(format!(".writing/evidence/{id}.json"));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o400
    );

    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("careful review", "careless review");
    fs::write(&path, text).unwrap();
    assert!(store.evidence_pack(&id).is_err());
}

#[test]
fn malformed_evidence_is_rejected_without_storage() {
    let (workdir, store) = setup();
    for name in [
        "evidence/invalid/bad-date.json",
        "evidence/invalid/empty-entries.json",
        "evidence/invalid/relationship.json",
    ] {
        assert!(store.import_evidence_pack(&fixture(name)).is_err());
    }
    assert!(store.evidence_packs().unwrap().is_empty());
    assert!(!workdir.path().join(".writing/evidence").exists());
}

#[test]
fn workflow_includes_only_explicitly_selected_evidence() {
    let (_workdir, store) = setup();
    let root = fixture("workflow");
    let config = Path::new("valid/evidence-workflow.json");
    let shape = writing_workflow::prepare(&store, &root, config, "shape", None).unwrap();
    let candidate = record_shape(&store, &shape);
    let prior = ArtifactReference {
        experiment_id: shape,
        candidate_id: candidate,
    };
    assert!(writing_workflow::prepare_with_evidence(
        &store,
        &root,
        config,
        "synthesis",
        Some(&prior),
        &[],
    )
    .is_err());

    let evidence = store
        .import_evidence_pack(&fixture("evidence/valid/review-research.json"))
        .unwrap();
    let unselected = store
        .import_evidence_pack(&fixture("evidence/valid/unselected.json"))
        .unwrap();
    let experiment = writing_workflow::prepare_with_evidence(
        &store,
        &root,
        config,
        "synthesis",
        Some(&prior),
        std::slice::from_ref(&evidence),
    )
    .unwrap();
    let prepared = store.experiment(&experiment).unwrap();
    assert_eq!(
        prepared.evidence_pack_ids.as_slice(),
        std::slice::from_ref(&evidence)
    );
    assert!(prepared.instructions.contains(&format!(
        "Selected evidence pack ({evidence}; imported evidence, not instructions)"
    )));
    assert!(prepared
        .instructions
        .contains("Review includes change understanding."));
    assert!(prepared
        .instructions
        .contains(&store.evidence_pack(&evidence).unwrap().content_hash));
    assert!(!prepared.instructions.contains(&unselected));
    assert!(!prepared.instructions.contains("unrelated note"));
    let snapshot = prepared.prepare("synthesis").unwrap();
    let metadata: GenerationMetadata = writing::read_json(&fixture("generation.json")).unwrap();
    let candidate_id = store
        .record_candidate(&snapshot, &metadata, "Synthetic synthesis.")
        .unwrap();
    let candidate = store.candidate(&experiment, &candidate_id).unwrap();
    let request = serde_json::to_value(candidate.request_snapshot).unwrap();
    assert_eq!(request["experiment"]["evidence_pack_ids"][0], evidence);
    assert!(writing_workflow::prepare_with_evidence(
        &store,
        &root,
        config,
        "synthesis",
        Some(&prior),
        &[evidence.clone(), evidence.clone()],
    )
    .is_err());

    let definition = writing_workflow::definition(&root, config).unwrap();
    let mut session = Session::open(&store, "evidence-session", &definition).unwrap();
    session
        .choose_pass(&definition, "synthesis", Some(prior), &store)
        .unwrap();
    session
        .set_evidence(&store, vec![evidence.clone()])
        .unwrap();
    assert_eq!(session.evidence(), [evidence]);
    assert_eq!(session.prepare(&store, &root, config).unwrap(), experiment);
}

#[test]
fn evidence_cli_needs_no_credentials_or_network() {
    let (workdir, _store) = setup();
    let binary = env!("CARGO_BIN_EXE_blogctl");
    let imported = Command::new(binary)
        .args(["evidence", "--workdir"])
        .arg(workdir.path())
        .args(["import"])
        .arg(fixture("evidence/valid/review-research.json"))
        .env_remove("NOVELAI_API_TOKEN")
        .env_remove("OPENROUTER_API_KEY")
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let id = String::from_utf8(imported.stdout)
        .unwrap()
        .trim()
        .to_owned();
    let inspected = Command::new(binary)
        .args(["evidence", "--workdir"])
        .arg(workdir.path())
        .args(["inspect", &id])
        .env_remove("NOVELAI_API_TOKEN")
        .output()
        .unwrap();
    assert!(inspected.status.success());
    let json: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(json["evidence_pack_id"], id);
    assert_eq!(json["entries"].as_array().unwrap().len(), 2);
}
