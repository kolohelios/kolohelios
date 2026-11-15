//! Polished approval and export are local-only and use synthetic candidates.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use blogctl::writing::{self, CandidateContext, GenerationMetadata, ReviewDecision, Store};
use blogctl::writing_polished::PolishedDecision;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing")
        .join(name)
}

fn setup_candidate(
    artifact_kind: Option<&str>,
) -> (tempfile::TempDir, Store, String, String, String) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    store
        .import_experiment(&fixture("valid/chapter.json"))
        .unwrap();
    let prepared = store
        .experiment("synthetic-chapter")
        .unwrap()
        .prepare("workshop")
        .unwrap();
    let metadata: GenerationMetadata = writing::read_json(&fixture("generation.json")).unwrap();
    let body = fs::read_to_string(fixture("candidate.txt")).unwrap();
    let candidate = store
        .record_candidate_with_context(
            &prepared,
            &metadata,
            &body,
            &CandidateContext {
                artifact_kind: artifact_kind.map(str::to_owned),
                parent: None,
            },
        )
        .unwrap();
    (root, store, "synthetic-chapter".into(), candidate, body)
}

fn decide(store: &Store, experiment: &str, candidate: &str, decision: PolishedDecision) -> String {
    let digest = writing::hash(&store.read_candidate(experiment, candidate).unwrap());
    store
        .record_polished_decision(
            experiment,
            candidate,
            &digest,
            decision,
            "Synthetic Author".into(),
            "Reviewed complete synthetic chapter.".into(),
        )
        .unwrap()
}

#[test]
fn polished_decisions_are_hash_bound_immutable_and_idempotent() {
    let (root, store, experiment, candidate, _body) = setup_candidate(Some("final-draft"));
    let approval = decide(&store, &experiment, &candidate, PolishedDecision::Approve);
    assert_eq!(
        decide(&store, &experiment, &candidate, PolishedDecision::Approve,),
        approval
    );
    let rejection = decide(&store, &experiment, &candidate, PolishedDecision::Reject);
    assert_ne!(approval, rejection);

    let records = store.polished_decisions(&experiment, &candidate).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].candidate_hash.len(), 64);
    assert_eq!(records[0].body_hash.len(), 64);
    let directory = root
        .path()
        .join(".writing/polished")
        .join(&experiment)
        .join(&candidate);
    for entry in fs::read_dir(directory).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o400
        );
    }
}

#[test]
fn export_requires_an_exact_approval_and_never_clobbers() {
    let (root, store, experiment, candidate, body) = setup_candidate(Some("final-draft"));
    let candidate_hash = writing::hash(&store.read_candidate(&experiment, &candidate).unwrap());
    let ordinary_review = store
        .record_review(
            &experiment,
            &candidate,
            &candidate_hash,
            ReviewDecision::Accept,
            "Synthetic Reviewer".into(),
            String::new(),
        )
        .unwrap();
    let export_dir = tempfile::tempdir().unwrap();
    assert!(store
        .export_polished(
            &experiment,
            &candidate,
            &ordinary_review,
            &export_dir.path().join("ordinary-review.md"),
        )
        .is_err());

    let rejection = decide(&store, &experiment, &candidate, PolishedDecision::Reject);
    let destination = export_dir.path().join("chapter.md");
    assert!(store
        .export_polished(&experiment, &candidate, &rejection, &destination)
        .is_err());

    let approval = decide(&store, &experiment, &candidate, PolishedDecision::Approve);
    let first = store
        .export_polished(&experiment, &candidate, &approval, &destination)
        .unwrap();
    assert!(first.created);
    assert_eq!(fs::read_to_string(&destination).unwrap(), body);
    assert_eq!(
        fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let repeated = store
        .export_polished(&experiment, &candidate, &approval, &destination)
        .unwrap();
    assert!(!repeated.created);
    assert_eq!(repeated.body_hash, first.body_hash);

    let cli_destination = export_dir.path().join("cli-chapter.md");
    let output = Command::new(env!("CARGO_BIN_EXE_blogctl"))
        .args(["candidate", "--workdir"])
        .arg(root.path())
        .args(["export", &experiment, &candidate, "--approval", &approval])
        .arg("--destination")
        .arg(&cli_destination)
        .env_remove("NOVELAI_API_TOKEN")
        .env_remove("OPENROUTER_API_KEY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["created"], true);
    assert_eq!(fs::read_to_string(cli_destination).unwrap(), body);

    let conflicting = export_dir.path().join("conflict.md");
    fs::write(&conflicting, "different bytes").unwrap();
    assert!(store
        .export_polished(&experiment, &candidate, &approval, &conflicting)
        .is_err());
    let linked = export_dir.path().join("linked.md");
    symlink(&destination, &linked).unwrap();
    assert!(store
        .export_polished(&experiment, &candidate, &approval, &linked)
        .is_err());
    assert!(store
        .export_polished(
            &experiment,
            &candidate,
            &approval,
            Path::new("../traversal.md"),
        )
        .is_err());
    assert!(store
        .export_polished(
            &experiment,
            &candidate,
            &approval,
            &root.path().join(".writing/export.md"),
        )
        .is_err());

    let later_rejection = decide(&store, &experiment, &candidate, PolishedDecision::Reject);
    assert_ne!(approval, later_rejection);
    assert!(store
        .export_polished(
            &experiment,
            &candidate,
            &approval,
            &export_dir.path().join("revoked.md"),
        )
        .is_err());
}

#[test]
fn stale_or_non_final_candidates_fail_closed() {
    let (root, store, experiment, candidate, _body) = setup_candidate(Some("final-draft"));
    let approval = decide(&store, &experiment, &candidate, PolishedDecision::Approve);
    let candidate_path = root
        .path()
        .join(".writing/candidates")
        .join(&experiment)
        .join(format!("{candidate}.md"));
    fs::set_permissions(&candidate_path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&candidate_path, "altered candidate").unwrap();
    assert!(store.polished_decisions(&experiment, &candidate).is_err());
    assert!(store
        .export_polished(
            &experiment,
            &candidate,
            &approval,
            &tempfile::tempdir().unwrap().path().join("chapter.md"),
        )
        .is_err());

    let (_root, store, experiment, candidate, _body) = setup_candidate(Some("draft"));
    let digest = writing::hash(&store.read_candidate(&experiment, &candidate).unwrap());
    assert!(store
        .record_polished_decision(
            &experiment,
            &candidate,
            &digest,
            PolishedDecision::Approve,
            "Synthetic Author".into(),
            String::new(),
        )
        .is_err());
}
