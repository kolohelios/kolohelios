//! All responses come from local mocks and real synthetic fixture files.
use super::*;
use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use crate::commands::novelai::generate_candidate;
use crate::writing::{self, CandidateProvenance, Experiment, ReviewStatus, Store};

const TOKEN: &str = "fake-token-must-not-appear-in-errors";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn generation(model: &str) -> PreparedGeneration {
    let exp: Experiment = writing::read_json(&fixture("writing/valid/chapter.json")).unwrap();
    PreparedGeneration::new(exp.prepare("workshop").unwrap(), model.into()).unwrap()
}

fn client(server: &mockito::ServerGuard) -> Client {
    Client::new(TOKEN.into(), server.url(), Duration::from_secs(5)).unwrap()
}

fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .import_experiment(&fixture("writing/valid/chapter.json"))
        .unwrap();
    (dir, store)
}

#[test]
fn discovery_response_handling() {
    for (file, expected) in [
        ("available.json", vec!["model-a", "model-b"]),
        ("empty.json", vec![]),
    ] {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/models")
            .match_header("authorization", format!("Bearer {TOKEN}").as_str())
            .with_body_from_file(fixture(&format!("novelai/models/valid/{file}")))
            .expect(1)
            .create();
        assert_eq!(client(&server).models().unwrap(), expected);
        mock.assert();
    }
    for case in fs::read_dir(fixture("novelai/models/invalid")).unwrap() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/models")
            .with_body_from_file(case.unwrap().path())
            .expect(1)
            .create();
        assert!(matches!(client(&server).models(), Err(Error::Response)));
        mock.assert();
    }
}

#[test]
fn preview_matches_the_exact_single_completion_request() {
    let generation = generation("model-a");
    let actual: serde_json::Value = serde_json::from_str(&generation.preview().unwrap()).unwrap();
    let expected: serde_json::Value =
        writing::read_json(&fixture("novelai/workshop-request.json")).unwrap();
    assert_eq!(actual, expected);
    assert!(!generation.preview().unwrap().contains(TOKEN));
    let metadata = generation.metadata();
    assert_eq!(metadata.provider, "novelai");
    assert_eq!(metadata.model, "model-a");
    assert_eq!(metadata.parameters.len(), 6);
    assert!(!metadata.parameters.contains_key("prompt"));
    assert!(!metadata.parameters.contains_key("Authorization"));
}

#[test]
fn completion_is_one_post_and_preserves_utf8_whitespace() {
    let mut server = mockito::Server::new();
    let request = fs::read_to_string(fixture("novelai/workshop-request.json")).unwrap();
    let mock = server
        .mock("POST", "/completions")
        .match_header("authorization", format!("Bearer {TOKEN}").as_str())
        .match_header("content-type", "application/json")
        .match_body(mockito::Matcher::JsonString(request))
        .with_body_from_file(fixture("novelai/completions/valid/single.json"))
        .expect(1)
        .create();
    let discovery = server.mock("GET", "/models").expect(0).create();
    let generation = generation("model-a");
    assert_eq!(
        client(&server).generate(&generation).unwrap(),
        "  The potter waited. Café clocks were less patient.\n"
    );
    mock.assert();
    discovery.assert();
}

#[test]
fn malformed_completions_never_retry_or_leak_the_body() {
    for case in fs::read_dir(fixture("novelai/completions/invalid")).unwrap() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/completions")
            .with_body_from_file(case.unwrap().path())
            .expect(1)
            .create();
        let generation = generation("model-a");
        let err = client(&server).generate(&generation).unwrap_err();
        assert!(matches!(err, Error::Response));
        assert!(!format!("{err:?} {err}").contains(TOKEN));
        assert!(err.to_string().contains("outcome may be unknown"));
        mock.assert();
    }
}

#[test]
fn http_errors_are_sanitized_and_never_retried() {
    for status in [400, 401, 403, 429, 500, 502, 503] {
        for method in ["GET", "POST"] {
            let mut server = mockito::Server::new();
            let route = if method == "GET" {
                "/models"
            } else {
                "/completions"
            };
            let mock = server
                .mock(method, route)
                .with_status(status)
                .with_header("retry-after", "0")
                .with_body_from_file(fixture("novelai/error.json"))
                .expect(1)
                .create();
            let client = client(&server);
            let generation = generation("model-a");
            let err = if method == "GET" {
                client.models().map(|_| ())
            } else {
                client.generate(&generation).map(|_| ())
            }
            .unwrap_err();
            match status {
                401 => assert!(matches!(err, Error::Authentication)),
                403 => assert!(matches!(err, Error::ModelAccess)),
                429 => assert!(matches!(err, Error::RateLimited)),
                _ => assert!(matches!(err, Error::Http { .. })),
            }
            let rendered = format!("{err:?} {err}");
            assert!(!rendered.contains(TOKEN));
            assert!(!rendered.contains("response-body-must-not-appear-in-errors"));
            assert!(rendered.contains("retry"));
            mock.assert();
        }
    }
}

#[test]
fn redirects_never_send_a_second_request_or_forward_credentials() {
    for status in [301, 302, 303, 307, 308] {
        let mut source = mockito::Server::new();
        let mut destination = mockito::Server::new();
        let forbidden = destination
            .mock("POST", mockito::Matcher::Any)
            .expect(0)
            .create();
        let forbidden_get = destination
            .mock("GET", mockito::Matcher::Any)
            .expect(0)
            .create();
        let redirect = source
            .mock("POST", "/completions")
            .with_status(status)
            .with_header("location", &format!("{}/leak", destination.url()))
            .expect(1)
            .create();
        let generation = generation("model-a");
        assert!(matches!(
            client(&source).generate(&generation),
            Err(Error::Http { .. })
        ));
        forbidden.assert();
        forbidden_get.assert();
        redirect.assert();
    }
    // GET redirects are refused too, even on the same host.
    let mut server = mockito::Server::new();
    let forbidden = server.mock("GET", "/other").expect(0).create();
    let redirect = server
        .mock("GET", "/models")
        .with_status(302)
        .with_header("location", "/other")
        .expect(1)
        .create();
    assert!(matches!(
        client(&server).models(),
        Err(Error::Http { status: 302 })
    ));
    forbidden.assert();
    redirect.assert();
}

#[test]
fn invalid_models_and_tokens_fail_locally() {
    let exp: Experiment = writing::read_json(&fixture("writing/valid/chapter.json")).unwrap();
    for bad in ["", "   ", "model\nunsafe", &"m".repeat(201)] {
        assert!(matches!(
            PreparedGeneration::new(exp.prepare("ordinary").unwrap(), bad.into()),
            Err(Error::InvalidModel)
        ));
    }
    assert!(matches!(
        Client::new(String::new(), BASE.into(), Duration::from_secs(1)),
        Err(Error::MissingToken)
    ));
    assert!(matches!(
        Client::new(
            "op://Test/NovelAI/credential".into(),
            BASE.into(),
            Duration::from_secs(1)
        ),
        Err(Error::UnresolvedReference)
    ));
    for bad in [
        "has spaces",
        "token\nunsafe",
        "non-ascii-é",
        &"t".repeat(8193),
    ] {
        let err = Client::new(bad.into(), BASE.into(), Duration::from_secs(1))
            .err()
            .unwrap();
        assert!(matches!(err, Error::InvalidToken));
        assert!(!err.to_string().contains(bad));
    }
}

#[test]
fn explicit_programmatic_calls_make_separate_candidates_without_terminal_input() {
    let (_dir, store) = setup();
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/completions")
        .with_body_from_file(fixture("novelai/completions/valid/single.json"))
        .expect(2)
        .create();
    let client = client(&server);
    let first = generate_candidate(&client, &store, generation("model-a")).unwrap();
    assert_eq!(store.candidates("synthetic-chapter").unwrap().len(), 1);
    let second = generate_candidate(&client, &store, generation("model-a")).unwrap();
    assert_ne!(first, second);
    assert_eq!(store.candidates("synthetic-chapter").unwrap().len(), 2);
    mock.assert();
}

#[test]
fn transport_disconnect_has_an_explicit_unknown_outcome() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        // Drop the connection after receiving the request, without a response.
        let mut bytes = [0; 8192];
        let _ = socket.read(&mut bytes).unwrap();
    });
    let client = Client::new(TOKEN.into(), base, Duration::from_secs(1)).unwrap();
    let generation = generation("model-a");
    let err = client.generate(&generation).unwrap_err();
    assert!(matches!(err, Error::Transport));
    assert!(err.to_string().contains("outcome may be unknown"));
    assert!(!format!("{err:?} {err}").contains(TOKEN));
    worker.join().unwrap();
}

#[test]
fn request_timeout_does_not_retry_or_assume_the_request_failed_remotely() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (release, wait) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut bytes = [0; 8192];
        assert!(socket.read(&mut bytes).unwrap() > 0);
        // Hold the accepted request without a response until the client times
        // out. A channel, rather than a sleep, makes the server deterministic.
        wait.recv().unwrap();
    });
    let client = Client::new(TOKEN.into(), base, Duration::from_millis(500)).unwrap();
    let request = generation("model-a");
    let result = client.generate(&request);
    release.send(()).unwrap();
    worker.join().unwrap();
    let err = result.unwrap_err();
    assert!(matches!(err, Error::Transport));
    assert!(err.to_string().contains("outcome may be unknown"));
    assert!(!format!("{err:?} {err}").contains(TOKEN));
}

#[test]
fn response_size_is_bounded() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/models")
        .with_body(vec![b' '; RESPONSE_LIMIT as usize + 1])
        .expect(1)
        .create();
    assert!(matches!(client(&server).models(), Err(Error::Response)));
    mock.assert();
}

#[test]
fn generation_records_one_unreviewed_candidate_from_the_frozen_snapshot() {
    let (dir, store) = setup();
    let mut server = mockito::Server::new();
    let request = generation("model-a");
    let original_hash = writing::hash(request.snapshot.prompt());
    let exp_file = dir
        .path()
        .join(".writing/experiments/synthetic-chapter.json");
    let mut changed: Experiment = writing::read_json(&exp_file).unwrap();
    let original_seed_hash = writing::hash(&changed.seed);
    changed.seed = "A changed synthetic seed".into();
    let changed = serde_json::to_vec(&changed).unwrap();
    let response = fs::read(fixture("novelai/completions/valid/single.json")).unwrap();
    let mock = server
        .mock("POST", "/completions")
        .with_body_from_request(move |_| {
            fs::write(&exp_file, &changed).unwrap();
            response.clone()
        })
        .expect(1)
        .create();
    let id = generate_candidate(&client(&server), &store, request).unwrap();
    let text = store.read_candidate("synthetic-chapter", &id).unwrap();
    let yaml = text
        .strip_prefix("---\n")
        .unwrap()
        .split_once("---\n")
        .unwrap()
        .0;
    let meta: CandidateProvenance = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(meta.review_status, ReviewStatus::Unreviewed);
    assert_eq!(meta.provider, "novelai");
    assert_eq!(meta.model, "model-a");
    assert_eq!(meta.prompt_hash, original_hash);
    assert_eq!(meta.original_seed_hash, original_seed_hash);
    assert_eq!(meta.parameters["n"].as_u64(), Some(1));
    assert_eq!(meta.parameters["best_of"].as_u64(), Some(1));
    assert_eq!(meta.parameters["stream"].as_bool(), Some(false));
    assert!(!text.contains(TOKEN));
    assert_eq!(store.candidates("synthetic-chapter").unwrap(), [id]);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    mock.assert();
}

#[test]
fn unsafe_storage_aborts_before_post_and_successful_request_save_failure_is_clear() {
    let (dir, store) = setup();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), dir.path().join(".writing/candidates")).unwrap();
    let mut server = mockito::Server::new();
    let no_post = server.mock("POST", "/completions").expect(0).create();
    let request = generation("model-a");
    assert!(generate_candidate(&client(&server), &store, request).is_err());
    no_post.assert();
    fs::remove_file(dir.path().join(".writing/candidates")).unwrap();
    let path = dir.path().join(".writing/candidates/synthetic-chapter");
    let response = fs::read(fixture("novelai/completions/valid/single.json")).unwrap();
    let mock = server
        .mock("POST", "/completions")
        .with_body_from_request(move |_| {
            fs::remove_dir(&path).unwrap();
            fs::write(&path, "blocked by a concurrent change").unwrap();
            response.clone()
        })
        .expect(1)
        .create();
    let request = generation("model-a");
    let err = generate_candidate(&client(&server), &store, request).unwrap_err();
    assert!(matches!(
        err,
        crate::error::Error::NovelaiCandidateNotSaved { .. }
    ));
    assert!(err.to_string().contains("request was already made"));
    assert!(!err.to_string().contains(TOKEN));
    mock.assert();
}
