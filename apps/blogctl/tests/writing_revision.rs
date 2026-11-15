//! Editorial revision checkpoints preserve exact local handoffs without selection.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use blogctl::writing::{self, ArtifactReference, CandidateContext, GenerationMetadata, Store};
use blogctl::writing_revision::{EditorialRevision, OriginKind, RevisionKind};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing")
        .join(name)
}

fn setup() -> (tempfile::TempDir, Store, String, String, String) {
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
                artifact_kind: Some("evidence-refinement".into()),
                parent: None,
                editorial_revision: None,
            },
        )
        .unwrap();
    (root, store, "synthetic-chapter".into(), candidate, body)
}

fn revision() -> EditorialRevision {
    EditorialRevision {
        origin_kind: OriginKind::AssistedRevision,
        revision_kind: RevisionKind::Substantive,
        contributors: vec!["Synthetic Author".into(), "Synthetic Editor".into()],
        notes: "Synthetic handoff for revision tests.".into(),
    }
}

#[test]
fn revision_round_trip_is_exact_private_lineaged_and_idempotent() {
    let (root, store, experiment, parent, original) = setup();
    let working = root.path().join("chapter-working.md");
    let preparation = store
        .prepare_revision(&experiment, &parent, &working)
        .unwrap();
    assert_eq!(fs::read_to_string(&working).unwrap(), original);
    assert_eq!(preparation.parent.candidate_id, parent);
    assert_eq!(
        fs::metadata(&working).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&preparation.sidecar)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let edited = format!("{original}\nA deliberate author revision.\n");
    fs::write(&working, &edited).unwrap();
    let checkpoint = store.import_revision(&working, revision()).unwrap();
    assert_eq!(
        store.import_revision(&working, revision()).unwrap(),
        checkpoint
    );

    let document = store.candidate(&experiment, &checkpoint).unwrap();
    assert_eq!(document.body, edited);
    assert_eq!(
        document.provenance.parent,
        Some(ArtifactReference {
            experiment_id: experiment.clone(),
            candidate_id: parent.clone(),
        })
    );
    assert_eq!(
        document.provenance.editorial_revision.as_ref(),
        Some(&revision())
    );
    assert_eq!(
        document.provenance.review_status,
        writing::ReviewStatus::Unreviewed
    );

    let candidate_path = root
        .path()
        .join(".writing/candidates")
        .join(&experiment)
        .join(format!("{checkpoint}.md"));
    assert_eq!(
        fs::metadata(candidate_path).unwrap().permissions().mode() & 0o777,
        0o400
    );

    let diff = store
        .revision_diff(
            &ArtifactReference {
                experiment_id: experiment.clone(),
                candidate_id: parent,
            },
            &ArtifactReference {
                experiment_id: experiment,
                candidate_id: checkpoint,
            },
        )
        .unwrap();
    assert!(diff.changed);
    assert!(diff.unified_diff.contains("+A deliberate author revision."));
}

#[test]
fn revision_paths_and_sidecars_fail_closed() {
    let (root, store, experiment, parent, _body) = setup();
    let working = root.path().join("working.md");
    let preparation = store
        .prepare_revision(&experiment, &parent, &working)
        .unwrap();
    assert!(store
        .prepare_revision(&experiment, &parent, &working)
        .is_err());
    assert!(store
        .prepare_revision(
            &experiment,
            &parent,
            &root.path().join(".writing/managed.md"),
        )
        .is_err());
    assert!(store
        .prepare_revision(&experiment, &parent, Path::new("/nix/store/revision.md"))
        .is_err());

    let linked = root.path().join("linked.md");
    symlink(&working, &linked).unwrap();
    assert!(store.import_revision(&linked, revision()).is_err());

    let mut sidecar: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&preparation.sidecar).unwrap()).unwrap();
    sidecar["body_hash"] = serde_json::Value::String("0".repeat(64));
    fs::write(
        &preparation.sidecar,
        serde_json::to_vec_pretty(&sidecar).unwrap(),
    )
    .unwrap();
    assert!(store.import_revision(&working, revision()).is_err());
}

#[test]
fn revision_metadata_and_working_bytes_are_validated() {
    let (root, store, experiment, parent, _body) = setup();
    let working = root.path().join("validation.md");
    let preparation = store
        .prepare_revision(&experiment, &parent, &working)
        .unwrap();

    let mut invalid = revision();
    invalid.contributors.clear();
    assert!(store.import_revision(&working, invalid).is_err());
    let mut invalid = revision();
    invalid.contributors = vec!["   ".into()];
    assert!(store.import_revision(&working, invalid).is_err());
    let mut invalid = revision();
    invalid.contributors = vec!["author\nname".into()];
    assert!(store.import_revision(&working, invalid).is_err());

    fs::write(&working, "   \n").unwrap();
    assert!(store.import_revision(&working, revision()).is_err());

    fs::write(&working, "Edited body.\n").unwrap();
    fs::set_permissions(&working, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.import_revision(&working, revision()).is_err());
    fs::set_permissions(&working, fs::Permissions::from_mode(0o600)).unwrap();

    let original_sidecar = fs::read(&preparation.sidecar).unwrap();
    fs::write(&preparation.sidecar, b"not json").unwrap();
    assert!(store.import_revision(&working, revision()).is_err());
    fs::write(&preparation.sidecar, &original_sidecar).unwrap();

    let mut sidecar: serde_json::Value = serde_json::from_slice(&original_sidecar).unwrap();
    sidecar["working_file"] = serde_json::Value::String("/private/other.md".into());
    fs::write(
        &preparation.sidecar,
        serde_json::to_vec_pretty(&sidecar).unwrap(),
    )
    .unwrap();
    assert!(store.import_revision(&working, revision()).is_err());

    assert!(store
        .prepare_revision(
            &experiment,
            &parent,
            &root.path().join("nested/../escape.md"),
        )
        .is_err());
}

#[test]
fn revision_import_rejects_stale_parent_files_and_sidecar_versions() {
    let (root, store, experiment, parent, _body) = setup();
    let working = root.path().join("stale.md");
    let preparation = store
        .prepare_revision(&experiment, &parent, &working)
        .unwrap();

    let original_sidecar = fs::read(&preparation.sidecar).unwrap();
    let mut sidecar: serde_json::Value = serde_json::from_slice(&original_sidecar).unwrap();
    sidecar["version"] = serde_json::Value::from(2);
    fs::write(
        &preparation.sidecar,
        serde_json::to_vec_pretty(&sidecar).unwrap(),
    )
    .unwrap();
    assert!(store.import_revision(&working, revision()).is_err());
    fs::write(&preparation.sidecar, original_sidecar).unwrap();

    let candidate = root
        .path()
        .join(".writing/candidates")
        .join(&experiment)
        .join(format!("{parent}.md"));
    fs::set_permissions(&candidate, fs::Permissions::from_mode(0o600)).unwrap();
    let mut bytes = fs::read(&candidate).unwrap();
    bytes.extend_from_slice(b"\n");
    fs::write(&candidate, bytes).unwrap();
    fs::set_permissions(&candidate, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(store.import_revision(&working, revision()).is_err());
}

#[test]
fn revision_cli_needs_no_credentials_or_network() {
    let (root, _store, experiment, parent, _body) = setup();
    let working = root.path().join("cli-working.md");
    let binary = env!("CARGO_BIN_EXE_blogctl");
    let prepared = Command::new(binary)
        .env_clear()
        .current_dir(root.path())
        .args([
            "candidate",
            "--workdir",
            root.path().to_str().unwrap(),
            "revise-prepare",
            &experiment,
            &parent,
            "--output",
            working.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    fs::write(&working, "Edited through the credential-free CLI.\n").unwrap();
    let imported = Command::new(binary)
        .env_clear()
        .current_dir(root.path())
        .args([
            "candidate",
            "--workdir",
            root.path().to_str().unwrap(),
            "revise-import",
            working.to_str().unwrap(),
            "--origin-kind",
            "human-revision",
            "--revision-kind",
            "substantive",
            "--contributor",
            "Synthetic Author",
        ])
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let checkpoint = String::from_utf8(imported.stdout).unwrap();
    let checkpoint = checkpoint.trim();
    assert!(!checkpoint.is_empty());

    let from = format!("{experiment}/{parent}");
    let to = format!("{experiment}/{checkpoint}");
    let diff = Command::new(binary)
        .env_clear()
        .current_dir(root.path())
        .args([
            "candidate",
            "--workdir",
            root.path().to_str().unwrap(),
            "diff",
            &from,
            &to,
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        diff.status.success(),
        "{}",
        String::from_utf8_lossy(&diff.stderr)
    );
    let diff: serde_json::Value = serde_json::from_slice(&diff.stdout).unwrap();
    assert_eq!(diff["changed"], true);
}
