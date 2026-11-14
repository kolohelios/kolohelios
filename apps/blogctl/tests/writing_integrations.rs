//! Private-data workflows tested only with synthetic artifacts and local IO.
use std::fs;
use std::io::{Cursor, Write};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use blogctl::writing::{self, ReviewDecision, Store};
use blogctl::{agent_config, writing_mcp};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing")
        .join(name)
}

fn setup() -> (tempfile::TempDir, Store, String) {
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
    let metadata = writing::read_json(&fixture("generation.json")).unwrap();
    let id = store
        .record_candidate(&prepared, &metadata, "Synthetic candidate body.")
        .unwrap();
    (root, store, id)
}

#[test]
fn reviews_are_append_only_hash_bound_and_never_promote() {
    let (root, store, id) = setup();
    let before = store.read_candidate("synthetic-chapter", &id).unwrap();
    let digest = writing::hash(&before);
    assert!(store.reviews("synthetic-chapter", &id).unwrap().is_empty());
    for decision in [ReviewDecision::Accept, ReviewDecision::Reject] {
        store
            .record_review(
                "synthetic-chapter",
                &id,
                &digest,
                decision,
                "Test reviewer".into(),
                "Synthetic review note".into(),
            )
            .unwrap();
    }
    let records = store.reviews("synthetic-chapter", &id).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].decision, ReviewDecision::Accept);
    assert_eq!(records[1].decision, ReviewDecision::Reject);
    assert_eq!(records[0].candidate_hash, digest);
    assert_eq!(
        store.read_candidate("synthetic-chapter", &id).unwrap(),
        before
    );
    let directory = root
        .path()
        .join(".writing/reviews/synthetic-chapter")
        .join(&id);
    for entry in fs::read_dir(&directory).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o400
        );
    }
    let path = root
        .path()
        .join(".writing/candidates/synthetic-chapter")
        .join(format!("{id}.md"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&path, "changed bytes").unwrap();
    assert!(store.reviews("synthetic-chapter", &id).is_err());
    assert!(store
        .record_review(
            "synthetic-chapter",
            &id,
            &digest,
            ReviewDecision::Accept,
            "Reviewer".into(),
            String::new()
        )
        .is_err());
    assert_eq!(fs::read_dir(directory).unwrap().count(), 2);
    assert!(!root.path().join("published").exists());
}

#[test]
fn cli_review_cannot_use_piped_input_as_human_authorization() {
    let (root, store, id) = setup();
    let mut command = Command::new(env!("CARGO_BIN_EXE_blogctl"));
    command
        .args(["candidate", "--workdir"])
        .arg(root.path())
        .args([
            "review",
            "synthetic-chapter",
            &id,
            "--decision",
            "accept",
            "--reviewer",
            "Test reviewer",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: setsid is an async-signal-safe syscall, removing the controlling terminal.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn().unwrap();
    let _ = child.stdin.take().unwrap().write_all(b"ACCEPT\n");
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("controlling terminal"));
    assert!(store.reviews("synthetic-chapter", &id).unwrap().is_empty());
}

#[test]
fn mcp_tools_are_read_only_strict_and_use_engine_functions() {
    let (root, store, id) = setup();
    let input = fs::read_to_string(fixture("mcp.jsonl"))
        .unwrap()
        .replace("$CANDIDATE", &id);
    let mut output = Vec::new();
    let mut discoveries = 0;
    writing_mcp::run(&store, Cursor::new(input), &mut output, || {
        discoveries += 1;
        Ok(vec!["synthetic-model".into()])
    })
    .unwrap();
    let replies: Vec<serde_json::Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(discoveries, 1); // Notification and invented approval flag do not call models.
    let find = |id: u64| {
        replies
            .iter()
            .find(|reply| reply["id"].as_u64() == Some(id))
            .unwrap()
    };
    assert_eq!(find(0)["error"]["code"].as_i64(), Some(-32002));
    assert_eq!(
        find(1)["result"]["protocolVersion"].as_str(),
        Some("2025-03-26")
    );
    let tools = find(2)["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
    for tool in tools {
        assert_eq!(tool["annotations"]["readOnlyHint"].as_bool(), Some(true));
        let name = tool["name"].as_str().unwrap();
        assert!(!name.contains("generate") && !name.contains("review") && !name.contains("import"));
    }
    let text = |id| {
        find(id)["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(text(3).contains("synthetic-chapter"));
    assert!(text(4).contains("Original seed"));
    assert!(text(5).contains(&id));
    assert!(text(6).contains("Synthetic candidate body"));
    assert!(text(7).contains("synthetic-model"));
    for id in [8, 9, 10, 11, 14, 15] {
        assert!(find(id).get("error").is_some());
    }
    for id in [12, 13] {
        assert_eq!(find(id)["result"]["isError"].as_bool(), Some(true));
    }
    assert!(replies
        .iter()
        .any(|reply| reply["error"]["code"].as_i64() == Some(-32700)));
    assert!(replies
        .iter()
        .any(|reply| reply["error"]["code"].as_i64() == Some(-32600)));
    assert!(!root.path().join(".writing/reviews").exists());
    assert_eq!(store.candidates("synthetic-chapter").unwrap(), [id]);
}

#[test]
fn mcp_bounds_messages_and_recovers_without_protocol_noise() {
    let (_, store, _) = setup();
    let mut input = vec![b'x'; 1024 * 1024 + 50];
    input.push(b'\n');
    input.extend(fs::read(fixture("mcp.jsonl")).unwrap());
    let mut output = Vec::new();
    writing_mcp::run(&store, Cursor::new(input), &mut output, || Err(())).unwrap();
    let replies: Vec<serde_json::Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        replies[0]["error"]["message"].as_str(),
        Some("Request too large")
    );
    assert!(replies.iter().any(|reply| reply["id"].as_u64() == Some(7)
        && reply["result"]["isError"].as_bool() == Some(true)));
}

#[test]
fn stdio_server_uses_only_the_configured_model_helper_and_suppresses_errors() {
    let (root, _, id) = setup();
    let shell = Command::new("sh")
        .args(["-c", "command -v sh"])
        .output()
        .unwrap();
    assert!(shell.status.success());
    let shell = String::from_utf8(shell.stdout).unwrap();
    assert!(Path::new(shell.trim()).is_absolute());
    for (fixture_name, success) in [
        ("models-helper.sh", true),
        ("models-error.sh", false),
        ("models-malformed.sh", false),
    ] {
        let helper = root.path().join("model-helper");
        let source = fs::read_to_string(fixture(fixture_name))
            .unwrap()
            .replace("@SHELL@", shell.trim());
        fs::write(&helper, source).unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        let input = fs::read_to_string(fixture("mcp.jsonl"))
            .unwrap()
            .replace("$CANDIDATE", &id);
        let mut child = Command::new(env!("CARGO_BIN_EXE_blogctl"))
            .args(["mcp", "serve", "--workdir"])
            .arg(root.path())
            .arg("--models-command")
            .arg(&helper)
            .env_remove("NOVELAI_API_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("fake-credential-must-not-leak"));
        let response: serde_json::Value = text
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|value| value["id"].as_u64() == Some(7))
            .unwrap();
        if success {
            assert!(response["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("synthetic-helper-model"));
        } else {
            assert_eq!(response["result"]["isError"].as_bool(), Some(true));
        }
    }
}

#[test]
fn migration_preserves_original_bytes_ids_hashes_and_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let source = fixture("legacy/experiment.json");
    let runs = fixture("legacy/runs");
    let report = writing::migration::migrate(&store, &source, &runs).unwrap();
    assert_eq!(report.copied_runs, 1);
    let id = "20260101T120000000000Z-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert_eq!(store.candidates("legacy-experiment").unwrap(), [id]);
    let original = fs::read_to_string(runs.join(format!("{id}.md"))).unwrap();
    assert_eq!(
        store.read_candidate("legacy-experiment", id).unwrap(),
        original
    );
    assert_eq!(
        fs::read(
            root.path()
                .join(".writing/legacy/novelai-lab/experiment.json")
        )
        .unwrap(),
        fs::read(&source).unwrap()
    );
    let exp = store.experiment("legacy-experiment").unwrap();
    assert_eq!(exp.seed, "A synthetic legacy seed.");
    assert_eq!(exp.variants.len(), 3);
    assert!(exp.prepare("woodland").is_ok());
    let second = writing::migration::migrate(&store, &source, &runs).unwrap();
    assert_eq!(second.copied_runs, 0);
    assert_eq!(second.existing_runs, 1);
    assert!(store.reviews("legacy-experiment", id).unwrap().is_empty());
    store
        .record_review(
            "legacy-experiment",
            id,
            &writing::hash(&original),
            ReviewDecision::Accept,
            "Reviewer".into(),
            String::new(),
        )
        .unwrap();
    assert_eq!(
        store.read_candidate("legacy-experiment", id).unwrap(),
        original
    );
}

#[test]
fn migration_refuses_conflicts_corruption_and_symlinks_without_clobbering() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let source = fixture("legacy/experiment.json");
    let runs = fixture("legacy/runs");
    let directory = root.path().join(".writing/experiments");
    fs::create_dir_all(&directory).unwrap();
    let existing = directory.join("legacy-experiment.json");
    fs::write(&existing, "existing unrelated bytes").unwrap();
    assert!(writing::migration::migrate(&store, &source, &runs).is_err());
    assert_eq!(
        fs::read_to_string(&existing).unwrap(),
        "existing unrelated bytes"
    );
    assert!(!root
        .path()
        .join(".writing/legacy/novelai-lab/experiment.json")
        .exists());
    let bad = tempfile::tempdir().unwrap();
    let id = "20260101T120000000000Z-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.md";
    let original = fs::read_to_string(runs.join(id)).unwrap();
    fs::write(bad.path().join(id), original.replace("501592", "000000")).unwrap();
    assert!(writing::migration::migrate(&store, &source, bad.path()).is_err());
    fs::remove_file(bad.path().join(id)).unwrap();
    symlink(runs.join(id), bad.path().join(id)).unwrap();
    assert!(writing::migration::migrate(&store, &source, bad.path()).is_err());
}

#[test]
fn registration_preserves_unrelated_settings_removes_only_owned_legacy_and_repeats() {
    let home = tempfile::tempdir().unwrap();
    let pi = home.path().join(".pi/agent/mcp.json");
    let codex = home.path().join(".codex/config.toml");
    fs::create_dir_all(pi.parent().unwrap()).unwrap();
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::copy(fixture("agents/pi.json"), &pi).unwrap();
    fs::copy(fixture("agents/codex.toml"), &codex).unwrap();
    let executable = Path::new("/nix/store/test/bin/blogctl");
    agent_config::install(home.path(), executable, home.path(), None).unwrap();
    let before = (fs::read(&pi).unwrap(), fs::read(&codex).unwrap());
    agent_config::install(home.path(), executable, home.path(), None).unwrap();
    assert_eq!(before, (fs::read(&pi).unwrap(), fs::read(&codex).unwrap()));
    let value: serde_json::Value = writing::read_json(&pi).unwrap();
    assert!(value["mcpServers"].get("novelai").is_none());
    assert_eq!(
        value["mcpServers"]["other"]["env"]["SETTING"].as_str(),
        Some("preserve")
    );
    assert_eq!(value["autoEnableCodemode"].as_bool(), Some(false));
    let text = fs::read_to_string(&codex).unwrap();
    assert!(text.contains("# preserve user comments"));
    assert!(text.contains("existing-model"));
    assert!(!text.contains("mcp_servers.novelai"));
    assert_eq!(pi.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    agent_config::install(
        home.path(),
        Path::new("/nix/store/updated/bin/blogctl"),
        home.path(),
        None,
    )
    .unwrap();
    assert!(fs::read_to_string(&codex).unwrap().contains("/updated/"));
}

#[test]
fn registration_validates_both_clients_before_writing_and_refuses_symlinks() {
    let home = tempfile::tempdir().unwrap();
    let pi = home.path().join(".pi/agent/mcp.json");
    let codex = home.path().join(".codex/config.toml");
    fs::create_dir_all(pi.parent().unwrap()).unwrap();
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::copy(fixture("agents/pi.json"), &pi).unwrap();
    fs::copy(fixture("agents/malformed.toml"), &codex).unwrap();
    let before = fs::read(&pi).unwrap();
    let command = Path::new("/nix/store/test/bin/blogctl");
    assert!(agent_config::install(home.path(), command, home.path(), None).is_err());
    assert_eq!(fs::read(&pi).unwrap(), before);
    fs::remove_file(&codex).unwrap();
    fs::copy(fixture("agents/conflict.json"), &pi).unwrap();
    assert!(agent_config::install(home.path(), command, home.path(), None).is_err());
    fs::remove_file(&pi).unwrap();
    symlink(fixture("agents/pi.json"), &pi).unwrap();
    assert!(agent_config::install(home.path(), command, home.path(), None).is_err());
}
