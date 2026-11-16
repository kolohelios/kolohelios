//! All responses come from local mocks and real synthetic fixture files.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::commands::novelai::generate_candidate;
use crate::writing::{
    self, CandidateContext, CandidateProvenance, Experiment, ReviewStatus, Store,
};

const TOKEN: &str = "fake-token-must-not-appear-in-errors";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn generation(model: &str) -> PreparedGeneration {
    let exp: Experiment = writing::read_json(&fixture("writing/valid/chapter.json")).unwrap();
    PreparedGeneration::with_transport(
        exp.prepare("workshop").unwrap(),
        model.into(),
        None,
        None,
        Transport::Completion,
    )
    .unwrap()
}

fn client(server: &mockito::ServerGuard) -> Client {
    Client::new(TOKEN.into(), server.url(), Duration::from_secs(5)).unwrap()
}

fn captured(client: &Client, generation: &PreparedGeneration) -> Result<GeneratedCompletion> {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    client.generate(generation, &store)
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
fn chat_uses_roles_disables_thinking_and_freezes_the_exact_request() {
    let (_dir, store) = setup();
    let mut exp: Experiment = writing::read_json(&fixture("writing/valid/chapter.json")).unwrap();
    exp.source_material = "Synthetic prior prose and selected evidence.".into();
    let generation = PreparedGeneration::new(
        exp.prepare("workshop").unwrap(),
        "model-a".into(),
        Some(2400),
        Some(0.2),
    )
    .unwrap();
    let preview: serde_json::Value = serde_json::from_str(&generation.preview().unwrap()).unwrap();
    let expected: serde_json::Value =
        writing::read_json(&fixture("novelai/chat-request.json")).unwrap();
    assert_eq!(preview, expected);
    assert!(preview.get("prompt").is_none());
    assert_eq!(preview["enable_thinking"], false);
    assert_eq!(preview["messages"][0]["role"], "system");
    assert_eq!(preview["messages"][1]["role"], "user");
    assert!(!preview["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains(&exp.source_material));
    let user = preview["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains(&exp.source_material));
    assert!(!user.ends_with("Candidate:\n"));
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", format!("Bearer {TOKEN}").as_str())
        .match_body(mockito::Matcher::Json(preview.clone()))
        .with_body_from_file(fixture("novelai/chat/valid/stop.json"))
        .expect(1)
        .create();
    let legacy = server.mock("POST", "/completions").expect(0).create();
    let discovery = server.mock("GET", "/models").expect(0).create();
    let completion = client(&server).generate(&generation, &store).unwrap();
    assert_eq!(
        completion.body,
        "  The potter waited. Café clocks were less patient.\n"
    );
    assert!(completion.warnings().is_empty());
    let metadata = completion.metadata(&generation);
    assert_eq!(metadata.parameters["request"], preview);
    assert_eq!(metadata.parameters["endpoint"], "chat/completions");
    assert_eq!(metadata.parameters["finish_reason"], "stop");
    assert_eq!(metadata.parameters["usage"]["completion_tokens"], 15);
    let id = store
        .record_candidate(generation.snapshot(), &metadata, &completion.body)
        .unwrap();
    let saved = store.candidate("synthetic-chapter", &id).unwrap();
    assert_eq!(saved.provenance.parameters["request"], preview);
    assert_eq!(saved.body, completion.body);
    mock.assert();
    legacy.assert();
    discovery.assert();
}

#[test]
fn chat_preserves_truncation_reasoning_and_scaffolding_without_retry_or_cleanup() {
    for case in fs::read_dir(fixture("novelai/chat/valid")).unwrap() {
        let (_dir, store) = setup();
        let path = case.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        let source: serde_json::Value = writing::read_json(&path).unwrap();
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/chat/completions")
            .with_body_from_file(&path)
            .expect(1)
            .create();
        let exp: Experiment = writing::read_json(&fixture("writing/valid/chapter.json")).unwrap();
        let generation = PreparedGeneration::new(
            exp.prepare("workshop").unwrap(),
            "model-a".into(),
            None,
            None,
        )
        .unwrap();
        let id = generate_candidate(
            &client(&server),
            &store,
            generation,
            &CandidateContext::default(),
        )
        .unwrap();
        let candidate = store.candidate("synthetic-chapter", &id).unwrap();
        assert_eq!(
            candidate.body,
            source["choices"][0]["message"]["content"].as_str().unwrap()
        );
        assert_eq!(
            candidate.provenance.parameters["finish_reason"],
            source["choices"][0]["finish_reason"]
        );
        assert_eq!(candidate.provenance.review_status, ReviewStatus::Unreviewed);
        if name == "reasoning.json" {
            assert_eq!(
                candidate.provenance.parameters["reasoning_content"],
                source["choices"][0]["message"]["reasoning_content"]
            );
            assert!(!candidate.body.contains("Synthetic planning"));
        }
        assert_eq!(store.candidates("synthetic-chapter").unwrap().len(), 1);
        mock.assert();
    }
}

#[test]
fn response_warnings_distinguish_normal_stops_from_incomplete_or_leaked_output() {
    for (name, warning_count) in [
        ("stop.json", 0),
        ("length.json", 1),
        ("reasoning.json", 1),
        ("scaffolding.json", 2),
        ("missing-finish.json", 1),
    ] {
        let bytes = fs::read(fixture(&format!("novelai/chat/valid/{name}"))).unwrap();
        let completion = parse_completion(&bytes, Transport::Chat).unwrap();
        let original = completion.body.clone();
        assert_eq!(completion.warnings().len(), warning_count, "{name}");
        assert_eq!(completion.body, original);
    }
}

#[test]
fn malformed_chat_responses_fail_without_fallback_or_retry() {
    for case in fs::read_dir(fixture("novelai/chat/invalid")).unwrap() {
        let (_dir, store) = setup();
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/chat/completions")
            .with_body_from_file(case.unwrap().path())
            .expect(1)
            .create();
        let fallback = server.mock("POST", "/completions").expect(0).create();
        let exp: Experiment = writing::read_json(&fixture("writing/valid/chapter.json")).unwrap();
        let generation = PreparedGeneration::new(
            exp.prepare("workshop").unwrap(),
            "model-a".into(),
            None,
            None,
        )
        .unwrap();
        let error = generate_candidate(
            &client(&server),
            &store,
            generation,
            &CandidateContext::default(),
        )
        .unwrap_err();
        assert!(!format!("{error:?} {error}").contains(TOKEN));
        assert!(store.candidates("synthetic-chapter").unwrap().is_empty());
        mock.assert();
        fallback.assert();
    }
}

fn response_record(directory: &Path, id: &str) -> serde_json::Value {
    let path = directory
        .join(".writing/provider-responses")
        .join(format!("{id}.json"));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o400
    );
    assert_eq!(
        fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    writing::read_json(&path).unwrap()
}

#[test]
fn diagnostic_fixtures_report_safe_stages_without_losing_response_evidence() {
    let cases: Vec<serde_json::Value> =
        writing::read_json(&fixture("novelai/diagnostics/cases.json")).unwrap();
    for case in cases {
        let (directory, store) = setup();
        let path = fixture(&format!(
            "novelai/diagnostics/{}",
            case["file"].as_str().unwrap()
        ));
        let bytes = fs::read(&path).unwrap();
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/chat/completions")
            .with_body_from_file(path)
            .expect(1)
            .create();
        let exp: Experiment = writing::read_json(&fixture("writing/valid/chapter.json")).unwrap();
        let request = PreparedGeneration::new(
            exp.prepare("workshop").unwrap(),
            "model-a".into(),
            None,
            None,
        )
        .unwrap();
        let error = client(&server).generate(&request, &store).unwrap_err();
        let rendered = format!("{error:?} {error}");
        assert!(
            rendered.contains(case["diagnostic"].as_str().unwrap()),
            "{rendered}"
        );
        assert!(!rendered.contains("response-body-must-not-appear-in-errors"));
        assert!(!rendered.contains(TOKEN));
        let Error::CapturedResponse { capture_id, .. } = error else {
            panic!("missing record")
        };
        let record = response_record(directory.path(), &capture_id);
        assert_eq!(
            STANDARD
                .decode(record["body_base64"].as_str().unwrap())
                .unwrap(),
            bytes
        );
        assert!(store.candidates("synthetic-chapter").unwrap().is_empty());
        mock.assert();
    }
}

#[test]
fn failed_generation_retains_exact_response_and_precise_private_diagnostics() {
    let (directory, store) = setup();
    let path = fixture("novelai/chat/invalid/reasoning-only.json");
    let bytes = fs::read(&path).unwrap();
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_body_from_file(path)
        .expect(1)
        .create();
    let exp: Experiment = writing::read_json(&fixture("writing/valid/chapter.json")).unwrap();
    let request = PreparedGeneration::new(
        exp.prepare("workshop").unwrap(),
        "model-a".into(),
        None,
        None,
    )
    .unwrap();
    let expected_request: serde_json::Value =
        serde_json::from_str(&request.preview().unwrap()).unwrap();
    let error = client(&server).generate(&request, &store).unwrap_err();
    let rendered = format!("{error:?} {error}");
    let Error::CapturedResponse {
        capture_id,
        problem: ResponseProblem::ReasoningOnly,
    } = error
    else {
        panic!("{rendered}")
    };
    let record = response_record(directory.path(), &capture_id);
    assert_eq!(
        STANDARD
            .decode(record["body_base64"].as_str().unwrap())
            .unwrap(),
        bytes
    );
    assert_eq!(record["body_sha256"], writing::hash_bytes(&bytes));
    assert_eq!(record["status"], 200);
    assert_eq!(record["request"], expected_request);
    assert_eq!(record["truncated"], false);
    assert!(!rendered.contains("Synthetic reasoning"));
    assert!(!rendered.contains(TOKEN));
    assert!(store.candidates("synthetic-chapter").unwrap().is_empty());
    mock.assert();
}

#[test]
fn captures_http_status_but_withholds_credentials_reflected_by_provider() {
    let (directory, store) = setup();
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/completions")
        .with_status(401)
        .with_body_from_file(fixture("novelai/error.json"))
        .expect(1)
        .create();
    let error = client(&server)
        .generate(&generation("model-a"), &store)
        .unwrap_err();
    let Error::CapturedResponse {
        capture_id,
        problem: ResponseProblem::CredentialEcho,
    } = error
    else {
        panic!("unexpected error")
    };
    let record = response_record(directory.path(), &capture_id);
    assert_eq!(record["status"], 401);
    assert_eq!(record["credential_echo_detected"], true);
    assert_eq!(record["retained_bytes"], 0);
    assert_eq!(record["body_base64"], "");
    assert!(!serde_json::to_string(&record).unwrap().contains(TOKEN));
    mock.assert();

    let mock = server
        .mock("POST", "/completions")
        .with_status(429)
        .with_body_from_file(fixture("novelai/models/valid/empty.json"))
        .expect(1)
        .create();
    let error = client(&server)
        .generate(&generation("model-a"), &store)
        .unwrap_err();
    let Error::CapturedResponse {
        capture_id,
        problem: ResponseProblem::Http { status: 429 },
    } = error
    else {
        panic!("unexpected error")
    };
    assert_eq!(
        response_record(directory.path(), &capture_id)["status"],
        429
    );
    mock.assert();
}

#[test]
fn capture_path_failure_aborts_before_post_and_late_failure_does_not_follow_symlinks() {
    let (directory, store) = setup();
    let outside = tempfile::tempdir().unwrap();
    let capture_directory = directory.path().join(".writing/provider-responses");
    symlink(outside.path(), &capture_directory).unwrap();
    let mut server = mockito::Server::new();
    let no_post = server.mock("POST", "/completions").expect(0).create();
    assert!(matches!(
        client(&server).generate(&generation("model-a"), &store),
        Err(Error::CapturePreparation)
    ));
    no_post.assert();
    fs::remove_file(&capture_directory).unwrap();
    let moved = directory.path().join("retired-private-captures");
    let outside_path = outside.path().to_owned();
    let mock = server
        .mock("POST", "/completions")
        .with_body_from_request(move |_| {
            fs::rename(&capture_directory, &moved).unwrap();
            symlink(&outside_path, &capture_directory).unwrap();
            fs::read(fixture("novelai/completions/valid/single.json")).unwrap()
        })
        .expect(1)
        .create();
    assert!(matches!(
        client(&server).generate(&generation("model-a"), &store),
        Err(Error::CaptureSave { .. })
    ));
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert!(store.candidates("synthetic-chapter").unwrap().is_empty());
    mock.assert();
}

#[test]
fn response_capture_publication_never_clobbers_an_existing_artifact() {
    let (directory, store) = setup();
    let capture = Capture::prepare(&store).unwrap();
    let folder = directory.path().join(".writing/provider-responses");
    let temporary = fs::read_dir(&folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::metadata(temporary).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let destination = folder.join(format!("{}.json", capture.id));
    fs::copy(fixture("novelai/chat/valid/stop.json"), &destination).unwrap();
    let original = fs::read(&destination).unwrap();
    let body = CapturedBody::read(std::io::Cursor::new(original.clone()), TOKEN);
    assert!(matches!(
        capture.save(&store, &generation("model-a"), Some(200), Some(&body)),
        Err(Error::CaptureSave { .. })
    ));
    assert_eq!(fs::read(destination).unwrap(), original);
}

#[test]
fn response_capture_limits_bytes_and_preserves_partial_reads() {
    let (directory, store) = setup();
    let body = CapturedBody::read(
        std::io::Cursor::new(vec![b'x'; RESPONSE_LIMIT as usize + 20]),
        TOKEN,
    );
    assert!(matches!(body.problem, Some(ResponseProblem::Oversized)));
    let id = Capture::prepare(&store)
        .unwrap()
        .save(&store, &generation("model-a"), Some(200), Some(&body))
        .unwrap();
    let record = response_record(directory.path(), &id);
    assert_eq!(record["truncated"], true);
    assert_eq!(record["retained_bytes"], RESPONSE_LIMIT);
    assert_eq!(
        STANDARD
            .decode(record["body_base64"].as_str().unwrap())
            .unwrap()
            .len() as u64,
        RESPONSE_LIMIT
    );

    struct Interrupted {
        bytes: std::io::Cursor<Vec<u8>>,
    }
    impl Read for Interrupted {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let count = self.bytes.read(buffer)?;
            if count == 0 {
                Err(std::io::Error::other("synthetic read failure"))
            } else {
                Ok(count)
            }
        }
    }
    let expected = fs::read(fixture("novelai/chat/valid/stop.json")).unwrap();
    let body = CapturedBody::read(
        Interrupted {
            bytes: std::io::Cursor::new(expected.clone()),
        },
        TOKEN,
    );
    assert!(matches!(body.problem, Some(ResponseProblem::Read)));
    let id = Capture::prepare(&store)
        .unwrap()
        .save(&store, &generation("model-a"), Some(200), Some(&body))
        .unwrap();
    let record = response_record(directory.path(), &id);
    assert_eq!(record["read_failed"], true);
    assert_eq!(
        STANDARD
            .decode(record["body_base64"].as_str().unwrap())
            .unwrap(),
        expected
    );
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
    assert_eq!(metadata.parameters.len(), 9);
    assert!(!metadata.parameters.contains_key("prompt"));
    assert!(!metadata.parameters.contains_key("Authorization"));
}

#[test]
fn bounded_parameters_match_preview_and_provenance() {
    let exp: Experiment = writing::read_json(&fixture("writing/valid/chapter.json")).unwrap();
    let generation = PreparedGeneration::new(
        exp.prepare("workshop").unwrap(),
        "model-a".into(),
        Some(1200),
        Some(0.25),
    )
    .unwrap();
    let preview: serde_json::Value = serde_json::from_str(&generation.preview().unwrap()).unwrap();
    assert_eq!(preview["max_tokens"], 1200);
    assert_eq!(preview["temperature"], 0.25);
    let metadata = generation.metadata();
    assert_eq!(metadata.parameters["max_tokens"], 1200);
    assert_eq!(metadata.parameters["temperature"], 0.25);

    for max_tokens in [0, MAX_TOKENS_LIMIT + 1] {
        assert!(matches!(
            PreparedGeneration::new(
                exp.prepare("workshop").unwrap(),
                "model-a".into(),
                Some(max_tokens),
                None,
            ),
            Err(Error::InvalidMaxTokens)
        ));
    }
    for temperature in [-0.1, MAX_TEMPERATURE + 0.1, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            PreparedGeneration::new(
                exp.prepare("workshop").unwrap(),
                "model-a".into(),
                None,
                Some(temperature),
            ),
            Err(Error::InvalidTemperature)
        ));
    }
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
        captured(&client(&server), &generation).unwrap().body,
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
        let err = captured(&client(&server), &generation).unwrap_err();
        assert!(matches!(err, Error::CapturedResponse { .. }));
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
                captured(&client, &generation).map(|_| ())
            }
            .unwrap_err();
            if method == "POST" {
                assert!(matches!(
                    err,
                    Error::CapturedResponse {
                        problem: ResponseProblem::CredentialEcho,
                        ..
                    }
                ));
            } else {
                match status {
                    401 => assert!(matches!(err, Error::Authentication)),
                    403 => assert!(matches!(err, Error::ModelAccess)),
                    429 => assert!(matches!(err, Error::RateLimited)),
                    _ => assert!(matches!(err, Error::Http { .. })),
                }
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
            captured(&client(&source), &generation),
            Err(Error::CapturedResponse {
                problem: ResponseProblem::Http { .. },
                ..
            })
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
            PreparedGeneration::new(exp.prepare("ordinary").unwrap(), bad.into(), None, None),
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
    let first = generate_candidate(
        &client,
        &store,
        generation("model-a"),
        &CandidateContext::default(),
    )
    .unwrap();
    assert_eq!(store.candidates("synthetic-chapter").unwrap().len(), 1);
    let second = generate_candidate(
        &client,
        &store,
        generation("model-a"),
        &CandidateContext::default(),
    )
    .unwrap();
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
    let err = captured(&client, &generation).unwrap_err();
    assert!(matches!(
        err,
        Error::CapturedResponse {
            problem: ResponseProblem::Transport,
            ..
        }
    ));
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
    let result = captured(&client, &request);
    release.send(()).unwrap();
    worker.join().unwrap();
    let err = result.unwrap_err();
    assert!(matches!(
        err,
        Error::CapturedResponse {
            problem: ResponseProblem::Transport,
            ..
        }
    ));
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
    let id = generate_candidate(
        &client(&server),
        &store,
        request,
        &CandidateContext::default(),
    )
    .unwrap();
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
    assert!(generate_candidate(
        &client(&server),
        &store,
        request,
        &CandidateContext::default(),
    )
    .is_err());
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
    let err = generate_candidate(
        &client(&server),
        &store,
        request,
        &CandidateContext::default(),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        crate::error::Error::NovelaiCandidateNotSaved { .. }
    ));
    assert!(err.to_string().contains("request was already made"));
    assert!(!err.to_string().contains(TOKEN));
    mock.assert();
}
