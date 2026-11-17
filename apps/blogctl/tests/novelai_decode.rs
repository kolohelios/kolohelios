//! Decode public synthetic captures with the shipped CLI, without credentials.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use base64::{engine::general_purpose::STANDARD, Engine};
use blogctl::writing;

#[test]
fn captured_token_response_decodes_without_requests_or_candidate_writes() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join(".writing/provider-responses");
    fs::create_dir_all(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/novelai/tokens");
    let mut record: serde_json::Value = writing::read_json(&fixtures.join("record.json")).unwrap();
    let body = fs::read(fixtures.join("valid/unicode.json")).unwrap();
    record["retained_bytes"] = body.len().into();
    record["body_sha256"] = writing::hash(std::str::from_utf8(&body).unwrap()).into();
    record["body_base64"] = STANDARD.encode(body).into();
    let path = directory.join("response-synthetic.json");
    let original = serde_json::to_vec(&record).unwrap();
    fs::write(&path, &original).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blogctl"))
        .env_clear()
        .current_dir(root.path())
        .args([
            "novelai",
            "decode-response",
            "response-synthetic",
            "--workdir",
        ])
        .arg(root.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let decoded: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let cases: serde_json::Value = writing::read_json(&fixtures.join("valid/cases.json")).unwrap();
    assert_eq!(decoded["body"], cases[0]["body"]);
    assert_eq!(
        decoded["decoding"]["removed_terminal_token_ids"],
        cases[0]["removed"]
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
    assert!(!root.path().join(".writing/candidates").exists());
    assert!(!root.path().join(".writing/sessions").exists());
}
