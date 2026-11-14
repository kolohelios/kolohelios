//! GitHub push authentication and change selection. No secrets in payloads/logs.
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use snafu::Snafu;
use std::collections::BTreeSet;

pub const KEY_CONTEXT: &[u8] = b"notes-store-webhook-v1";
/// Domain-separated from the cookie key; GitHub never receives SESSION_SECRET.
pub fn signing_key(secret: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(KEY_CONTEXT);
    format!("{:x}", mac.finalize().into_bytes())
}
pub fn valid_signature(secret: &[u8], body: &[u8], signature: &str) -> bool {
    if secret.is_empty() {
        return false;
    }
    let Some(hex) = signature
        .strip_prefix("sha256=")
        .filter(|hex| hex.len() == 64)
    else {
        return false;
    };
    let bytes: Option<Vec<_>> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |c: u8| (c as char).to_digit(16).map(|v| v as u8);
            Some(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect();
    let Some(bytes) = bytes else {
        return false;
    };
    let key = signing_key(secret);
    let mut mac =
        Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body);
    mac.verify_slice(&bytes).is_ok()
}
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("invalid GitHub push payload"))]
    Payload { source: serde_json::Error },
    #[snafu(display("GitHub repository does not match this vault"))]
    Repository,
}
#[derive(Deserialize)]
struct Push {
    repository: Repository,
    #[serde(rename = "ref")]
    reference: String,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    forced: bool,
    size: Option<usize>,
    #[serde(default)]
    commits: Vec<Commit>,
}
#[derive(Deserialize)]
struct Repository {
    full_name: String,
}
#[derive(Deserialize)]
struct Commit {
    #[serde(default)]
    added: Vec<String>,
    #[serde(default)]
    modified: Vec<String>,
    #[serde(default)]
    removed: Vec<String>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Changes {
    Ignore,
    Notes(BTreeSet<String>),
    Rebuild,
}
pub fn changes(body: &[u8], repository: &str, branch: &str) -> Result<Changes, Error> {
    use snafu::ResultExt;
    let push: Push = serde_json::from_slice(body).context(PayloadSnafu)?;
    if !push.repository.full_name.eq_ignore_ascii_case(repository) {
        return Err(Error::Repository);
    }
    if push.reference != format!("refs/heads/{branch}") || push.deleted {
        return Ok(Changes::Ignore);
    }
    if push.forced
        || push.commits.is_empty()
        || push.size.is_some_and(|size| size > push.commits.len())
    {
        return Ok(Changes::Rebuild);
    }
    let ids = push
        .commits
        .into_iter()
        .flat_map(|commit| {
            commit
                .added
                .into_iter()
                .chain(commit.modified)
                .chain(commit.removed)
        })
        .filter_map(|path| {
            path.strip_prefix("notes/")
                .and_then(|path| path.strip_suffix(".md"))
                .map(str::to_owned)
        })
        .filter(|id| crate::route::is_valid_note_id(id))
        .collect();
    Ok(Changes::Notes(ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operator_cookie_and_hook_key_share_the_versioned_wire_contract() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/webhook/auth.json");
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let secret = fixture["secret"].as_str().unwrap().as_bytes();
        assert_eq!(
            crate::auth::verify_session(fixture["cookie"].as_str().unwrap(), secret, 1).unwrap(),
            fixture["did"]
        );
        assert_eq!(signing_key(secret), fixture["webhook_key"]);
        let empty = crate::auth::mint_session("did:plc:test-owner", 200, b"");
        assert!(!crate::auth::authorize_owner(
            Some(&format!("session={empty}")),
            Some(b""),
            Some("did:plc:test-owner"),
            1
        ));
    }
    #[test]
    fn signed_fixtures_select_changes_and_reject_tampering() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/webhook");
        let body = std::fs::read(root.join("push.json")).unwrap();
        assert_eq!(
            changes(&body, "example/notes", "main").unwrap(),
            Changes::Notes(BTreeSet::from([
                "a".into(),
                "folder/b".into(),
                "gone".into()
            ]))
        );
        assert_eq!(
            changes(&body, "example/notes", "other").unwrap(),
            Changes::Ignore
        );
        assert!(changes(&body, "different/repo", "main").is_err());
        assert_eq!(
            changes(
                &std::fs::read(root.join("forced.json")).unwrap(),
                "example/notes",
                "main"
            )
            .unwrap(),
            Changes::Rebuild
        );
        let key = signing_key(b"test-only-key");
        let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
        mac.update(&body);
        let signature = format!("sha256={:x}", mac.finalize().into_bytes());
        assert!(valid_signature(b"test-only-key", &body, &signature));
        assert!(!valid_signature(b"different-key", &body, &signature));
        assert!(!valid_signature(b"test-only-key", b"altered", &signature));
        assert!(!valid_signature(b"test-only-key", &body, "sha256=invalid"));
    }
}
