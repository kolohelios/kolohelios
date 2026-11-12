//! Synthetic-only local writing workflow. No credentials or network required.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use blogctl::writing::{
    self, CandidateProvenance, Experiment, GenerationMetadata, ReviewStatus, Store,
};
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing")
        .join(name)
}

fn setup() -> (TempDir, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).unwrap();
    store
        .import_experiment(&fixture("valid/chapter.json"))
        .unwrap();
    (tmp, store)
}

fn record(store: &Store) -> String {
    let prepared = store
        .experiment("synthetic-chapter")
        .unwrap()
        .prepare("workshop")
        .unwrap();
    let generation = writing::read_json(&fixture("generation.json")).unwrap();
    store
        .record_candidate(
            &prepared,
            &generation,
            &fs::read_to_string(fixture("candidate.txt")).unwrap(),
        )
        .unwrap()
}

fn candidate_dir(root: &Path) -> PathBuf {
    root.join(".writing/candidates/synthetic-chapter")
}

#[test]
fn fixture_definitions_validate_or_fail() {
    for case in fs::read_dir(fixture("valid")).unwrap() {
        let exp: Experiment = writing::read_json(&case.unwrap().path()).unwrap();
        exp.validate().unwrap();
        for variant in exp.variants.keys() {
            let prepared = exp.prepare(variant).unwrap();
            assert!(prepared.prompt().contains(&exp.seed));
            if !exp.fiction {
                assert!(prepared.prompt().contains("Do not invent facts"));
            }
        }
    }
    for case in fs::read_dir(fixture("invalid")).unwrap() {
        let path = case.unwrap().path();
        let outcome = writing::read_json::<Experiment>(&path).and_then(|exp| exp.validate());
        assert!(outcome.is_err(), "unexpectedly accepted {}", path.display());
    }
}

#[test]
fn prompt_is_exact_and_deterministic_for_every_variant() {
    let exp: Experiment = writing::read_json(&fixture("valid/chapter.json")).unwrap();
    assert_eq!(
        exp.prepare("workshop").unwrap().prompt(),
        fs::read_to_string(fixture("workshop-prompt.txt")).unwrap()
    );
    for variant in ["workshop", "woodland", "ordinary"] {
        let one = exp.prepare(variant).unwrap();
        let two = exp.prepare(variant).unwrap();
        assert_eq!(one.prompt(), two.prompt());
        assert!(one.prompt().contains(&exp.variants[variant]));
    }
    assert!(exp.prepare("missing").is_err());
    assert_eq!(
        writing::hash("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_ne!(writing::hash(&exp.seed), writing::hash(exp.seed.trim()));
}

#[test]
fn import_never_overwrites_and_reads_do_not_create_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).unwrap();
    assert!(store.experiments().unwrap().is_empty());
    assert!(store.candidates("missing").unwrap().is_empty());
    assert!(store.experiment("missing").is_err());
    assert!(store.read_candidate("missing", "candidate").is_err());
    assert!(!tmp.path().join(".writing").exists());
    store
        .import_experiment(&fixture("valid/chapter.json"))
        .unwrap();
    let path = tmp
        .path()
        .join(".writing/experiments/synthetic-chapter.json");
    let before = fs::read(&path).unwrap();
    assert!(store
        .import_experiment(&fixture("valid/chapter.json"))
        .is_err());
    assert_eq!(before, fs::read(&path).unwrap());
    assert_eq!(store.experiments().unwrap(), ["synthetic-chapter"]);
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    for path in [
        tmp.path().join(".writing"),
        path.parent().unwrap().to_path_buf(),
    ] {
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    }
}

#[test]
fn immutable_candidates_have_exact_provenance_and_frozen_snapshot() {
    let (tmp, store) = setup();
    let mut exp = store.experiment("synthetic-chapter").unwrap();
    let prepared = exp.prepare("workshop").unwrap();
    let generation: GenerationMetadata = writing::read_json(&fixture("generation.json")).unwrap();
    // Mutating the caller's definition after preparation cannot affect a run.
    let original_seed = exp.seed.clone();
    exp.seed = "A different synthetic idea".into();
    let body = fs::read_to_string(fixture("candidate.txt")).unwrap();
    let first = store
        .record_candidate(&prepared, &generation, &body)
        .unwrap();
    let second = store
        .record_candidate(&prepared, &generation, &body)
        .unwrap();
    assert_ne!(first, second);
    let text = store.read_candidate("synthetic-chapter", &first).unwrap();
    let yaml = text
        .strip_prefix("---\n")
        .unwrap()
        .split_once("---\n")
        .unwrap()
        .0;
    let meta: CandidateProvenance = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(meta.candidate_id, first);
    assert_eq!(meta.experiment_id, "synthetic-chapter");
    assert_eq!(meta.provider, generation.provider);
    assert_eq!(meta.model, generation.model);
    assert_eq!(meta.parameters["temperature"].as_f64(), Some(0.8));
    assert_eq!(meta.variant, "workshop");
    assert_eq!(meta.hash_algorithm, "sha256");
    assert_eq!(meta.review_status, ReviewStatus::Unreviewed);
    assert_eq!(meta.prompt_hash, writing::hash(prepared.prompt()));
    assert_eq!(meta.original_seed_hash, writing::hash(&original_seed));
    assert_eq!(meta.ai_provenance, generation.ai_provenance);
    assert!(meta.fiction);
    assert_eq!(
        time::OffsetDateTime::parse(
            &meta.timestamp,
            &time::format_description::well_known::Rfc3339
        )
        .unwrap()
        .offset(),
        time::UtcOffset::UTC
    );
    assert!(text.contains(&body));
    let snapshot = text
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("\n```")
        .next()
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(snapshot).unwrap();
    assert_eq!(value["prompt"].as_str().unwrap(), prepared.prompt());
    assert_eq!(value["experiment"]["seed"].as_str().unwrap(), original_seed);
    assert_eq!(value["prompt_format_version"].as_u64(), Some(1));
    let mut names = vec![first.clone(), second];
    names.sort();
    assert_eq!(store.candidates("synthetic-chapter").unwrap(), names);
    for entry in fs::read_dir(candidate_dir(tmp.path())).unwrap() {
        let path = entry.unwrap().path();
        assert_eq!(path.extension().unwrap(), "md");
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o400);
    }
    assert_eq!(
        candidate_dir(tmp.path())
            .metadata()
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(!tmp.path().join("concepts").exists());
    assert!(!tmp.path().join(".blog-os.toml").exists());
}

#[test]
fn candidate_metadata_and_empty_body_are_validated_before_write() {
    let (tmp, store) = setup();
    let exp = store.experiment("synthetic-chapter").unwrap();
    let prepared = exp.prepare("ordinary").unwrap();
    let mut meta: GenerationMetadata = writing::read_json(&fixture("generation.json")).unwrap();
    assert!(store.record_candidate(&prepared, &meta, " \n").is_err());
    meta.model.clear();
    assert!(store
        .record_candidate(&prepared, &meta, "Synthetic prose")
        .is_err());
    assert!(!candidate_dir(tmp.path()).exists());
}

#[test]
fn paths_reject_traversal_symlinks_and_nix_store() {
    let (tmp, store) = setup();
    for bad in [
        "../outside",
        "/etc/passwd",
        "a/b",
        ".",
        "..",
        "x.md",
        "",
        &"x".repeat(121),
    ] {
        assert!(store.experiment(bad).is_err());
        assert!(store.candidates(bad).is_err());
        assert!(store.read_candidate("synthetic-chapter", bad).is_err());
    }
    let original = record(&store);
    let outside = tempfile::tempdir().unwrap();
    symlink(
        fixture("candidate.txt"),
        candidate_dir(tmp.path()).join("symlink.md"),
    )
    .unwrap();
    assert!(store
        .read_candidate("synthetic-chapter", "symlink")
        .is_err());
    assert!(store.candidates("synthetic-chapter").is_err());
    symlink(
        fixture("valid/chapter.json"),
        tmp.path().join(".writing/experiments/symlink.json"),
    )
    .unwrap();
    assert!(store.experiment("symlink").is_err());
    assert!(store.experiments().is_err());
    let path = candidate_dir(tmp.path());
    fs::rename(&path, tmp.path().join("saved-candidates")).unwrap();
    symlink(outside.path(), &path).unwrap();
    assert!(store
        .read_candidate("synthetic-chapter", &original)
        .is_err());
    assert!(record_result(&store).is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    let other = tempfile::tempdir().unwrap();
    symlink(outside.path(), other.path().join(".writing")).unwrap();
    let unsafe_store = Store::open(other.path()).unwrap();
    assert!(unsafe_store
        .import_experiment(&fixture("valid/chapter.json"))
        .is_err());
    assert!(unsafe_store.experiments().is_err());
    assert!(Store::open(Path::new("/nix/store")).is_err());
    assert!(Store::open(&tmp.path().join("missing")).is_err());
    assert!(Store::open(&fixture("candidate.txt")).is_err());
}

fn record_result(store: &Store) -> writing::Result<String> {
    let prepared = store.experiment("synthetic-chapter")?.prepare("workshop")?;
    let meta = writing::read_json(&fixture("generation.json"))?;
    store.record_candidate(&prepared, &meta, "Synthetic prose")
}

fn cli(workdir: &Path, command: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_blogctl"))
        .arg(command)
        .arg("--workdir")
        .arg(workdir)
        .args(args)
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("NOVELAI_API_TOKEN")
        .output()
        .unwrap()
}

fn ok(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn cli_local_workflow_works_without_credentials_sync_or_post_stages() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(ok(cli(tmp.path(), "experiment", &["list"])), "");
    let source = fixture("valid/chapter.json");
    assert_eq!(
        ok(cli(
            tmp.path(),
            "experiment",
            &["import", source.to_str().unwrap()]
        )),
        "synthetic-chapter\n"
    );
    assert_eq!(
        ok(cli(tmp.path(), "experiment", &["list"])),
        "synthetic-chapter\n"
    );
    let shown: Experiment = serde_json::from_str(&ok(cli(
        tmp.path(),
        "experiment",
        &["show", "synthetic-chapter"],
    )))
    .unwrap();
    assert_eq!(shown.id, "synthetic-chapter");
    let prompt = ok(cli(
        tmp.path(),
        "experiment",
        &["prepare", "synthetic-chapter", "--variant", "workshop"],
    ));
    assert_eq!(
        prompt,
        fs::read_to_string(fixture("workshop-prompt.txt")).unwrap()
    );
    assert_eq!(
        ok(cli(tmp.path(), "candidate", &["list", "synthetic-chapter"])),
        ""
    );
    let metadata = fixture("generation.json");
    let body = fixture("candidate.txt");
    let id = ok(cli(
        tmp.path(),
        "candidate",
        &[
            "import",
            "synthetic-chapter",
            "--variant",
            "workshop",
            "--metadata-file",
            metadata.to_str().unwrap(),
            "--body-file",
            body.to_str().unwrap(),
        ],
    ));
    let id = id.trim();
    assert_eq!(
        ok(cli(tmp.path(), "candidate", &["list", "synthetic-chapter"])).trim(),
        id
    );
    assert!(ok(cli(
        tmp.path(),
        "candidate",
        &["read", "synthetic-chapter", id]
    ))
    .contains("review_status: unreviewed"));
    assert!(!cli(
        tmp.path(),
        "experiment",
        &["prepare", "synthetic-chapter", "--variant", "missing"]
    )
    .status
    .success());
    assert!(!cli(
        tmp.path(),
        "experiment",
        &["import", source.to_str().unwrap()]
    )
    .status
    .success());
    // No init, credentials, jj repo, or auto-sync was needed; stage dirs absent.
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
}

#[test]
fn parallel_candidates_are_unique_and_complete() {
    let (tmp, store) = setup();
    let ids = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..12).map(|_| scope.spawn(|| record(&store))).collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 12);
    assert_eq!(store.candidates("synthetic-chapter").unwrap().len(), 12);
    assert_eq!(fs::read_dir(candidate_dir(tmp.path())).unwrap().count(), 12);
    for id in ids {
        assert!(store
            .read_candidate("synthetic-chapter", &id)
            .unwrap()
            .contains("review_status: unreviewed"));
    }
}

#[test]
fn writing_dispatch_never_calls_vcs_even_with_sync_enabled() {
    use blogctl::cli::{dispatch_with_jj, Command};
    use blogctl::commands::experiment::{
        CandidateAction, CandidateArgs, ExperimentAction, ExperimentArgs,
    };
    let tmp = tempfile::tempdir().unwrap();
    let jj = blogctl::sync::FakeJj::default();
    blogctl::Repository::unchecked(blogctl::Workdir::new(tmp.path()))
        .init()
        .unwrap();
    dispatch_with_jj(
        Command::Experiment(ExperimentArgs {
            workdir: tmp.path().into(),
            action: ExperimentAction::Import {
                file: fixture("valid/chapter.json"),
            },
        }),
        &jj,
    )
    .unwrap();
    dispatch_with_jj(
        Command::Candidate(CandidateArgs {
            workdir: tmp.path().into(),
            action: CandidateAction::Import {
                experiment: "synthetic-chapter".into(),
                variant: "ordinary".into(),
                metadata_file: fixture("generation.json"),
                body_file: fixture("candidate.txt"),
            },
        }),
        &jj,
    )
    .unwrap();
    assert!(jj.calls().is_empty());
}

#[test]
fn existing_posts_and_configuration_remain_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "--no-sync"],
        vec!["new", "Existing post", "--no-sync"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_blogctl"))
            .args(args)
            .arg("--workdir")
            .arg(tmp.path())
            .output()
            .unwrap();
        ok(out);
    }
    let post = tmp.path().join("concepts/existing-post.md");
    let config = tmp.path().join(".blog-os.toml");
    let before = (fs::read(&post).unwrap(), fs::read(&config).unwrap());
    let store = Store::open(tmp.path()).unwrap();
    store
        .import_experiment(&fixture("valid/chapter.json"))
        .unwrap();
    record(&store);
    assert_eq!(
        before,
        (fs::read(&post).unwrap(), fs::read(&config).unwrap())
    );
    let repo = blogctl::Repository::open(blogctl::Workdir::new(tmp.path())).unwrap();
    let (_, loaded) = repo.load_raw("existing-post").unwrap();
    assert_eq!(loaded.metadata.kind, blogctl::Kind::Post);
}
