//! End-to-end lifecycle test: drives a single post from `init → new →
//! classify → promote → metrics` against a fresh tempdir workdir.
//!
//! This catches the kind of regression where one command's output stops
//! matching the next command's expected input (frontmatter rename, stage
//! directory rename, taxonomy mismatch) — gaps unit tests miss because
//! each one stays inside one module's contract.
//!
//! The AI-touching path lands here too via `lifecycle_with_ai_commands`,
//! which drives `commands::draft` and `commands::refine` against a
//! `mockito` server using the `OPENROUTER_API_BASE` seam (#474). The
//! `mock_chat_completion` helper makes adding the next AI command
//! (`final-edit`, ... tracked in #483) one more line, not a copy of
//! the JSON envelope.

use std::env;
use std::path::PathBuf;
use std::sync::Mutex;

use tempfile::TempDir;

use blogctl::commands::{
    self, add_target::AddTargetArgs, classify::ClassifyArgs, draft::DraftArgs,
    final_edit::FinalEditArgs, metrics::UpdateArgs, refine::RefineArgs,
};
use blogctl::error::Error;
use blogctl::kind::Kind;
use blogctl::stage::Stage;
use blogctl::storage::{Repository, Workdir};
use blogctl::sync::FakeJj;
use blogctl::target::Target;

// `tests/ai_endpoint.rs` also touches OPENROUTER_API_BASE; both
// processes run in parallel by default. Process-level mutex keeps the
// env-var dance from racing within this binary; cargo runs each
// integration test binary in its own process so the two files don't
// interfere with each other.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fresh_workdir() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();
    (tmp, path)
}

/// Register a one-shot `/chat/completions` mock that returns `content`
/// wrapped in an OpenRouter chat-completion envelope. Each call queues
/// one canned reply; sequential AI commands consume them in
/// registration order. Adding a new AI-touching command to the
/// lifecycle test is one more `mock_chat_completion(&mut server, ...)`
/// call rather than a new JSON literal.
fn mock_chat_completion(server: &mut mockito::Server, content: &str) -> mockito::Mock {
    let body = format!(
        r###"{{
  "id": "gen-test",
  "object": "chat.completion",
  "model": "test-model",
  "choices": [
    {{
      "index": 0,
      "message": {{ "role": "assistant", "content": {content_json} }},
      "finish_reason": "stop"
    }}
  ]
}}"###,
        content_json = serde_json::to_string(content).expect("content is utf-8")
    );
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create()
}

fn promote_to_published(workdir: &PathBuf, slug: &str) {
    // Concept → Ideation → Editing → FinalEditing → Published is four
    // hops; one promote call per hop, each asserting we advanced.
    for expected in [
        Stage::Ideation,
        Stage::Editing,
        Stage::FinalEditing,
        Stage::Published,
    ] {
        commands::promote::run(&FakeJj::new(), slug.to_string(), workdir.clone(), true)
            .expect("promote");
        let repo = Repository::open(Workdir::new(workdir)).expect("reopen");
        let (handle, _) = repo.load(slug).expect("load after promote");
        assert_eq!(handle.stage, expected, "stage after promote");
    }
}

#[test]
fn lifecycle_init_new_classify_promote_metrics() {
    let (_tmp, workdir) = fresh_workdir();

    // 1. init — scaffolds .blog-os.toml + stage dirs + README.
    commands::init::run(&FakeJj::new(), workdir.clone(), true).expect("init");

    // 2. new — creates a Concept-stage draft.
    commands::new::run(
        &FakeJj::new(),
        "Test post title".into(),
        workdir.clone(),
        None,
        Kind::Post,
        None,
        true,
    )
    .expect("new");

    // The slugifier is deterministic for ASCII input; we know what to
    // look for. Loading via Repository is the same call site the rest
    // of the tool uses, so any divergence between writer and reader
    // surfaces here.
    let slug = "test-post-title";
    let repo = Repository::open(Workdir::new(&workdir)).expect("open after new");
    let (handle, post) = repo.load(slug).expect("load after new");
    assert_eq!(handle.stage, Stage::Concept);
    assert_eq!(post.metadata.slug, slug);
    assert_eq!(post.metadata.title, "Test post title");
    assert!(post.metadata.classifications.format.is_none());

    // 3. classify — set format + hook against the default taxonomy.
    let mut args = ClassifyArgs {
        slug: slug.into(),
        workdir: workdir.clone(),
        no_sync: true,
        ..Default::default()
    };
    args.format = Some("thesis".into());
    args.hook = Some("contradiction".into());
    commands::classify::run(&FakeJj::new(), args).expect("classify");

    let repo = Repository::open(Workdir::new(&workdir)).expect("reopen after classify");
    let (_, post) = repo.load(slug).expect("load after classify");
    assert_eq!(
        post.metadata.classifications.format.as_deref(),
        Some("thesis")
    );
    assert_eq!(
        post.metadata.classifications.hook.as_deref(),
        Some("contradiction")
    );

    // 4. promote — walk Concept → Published, four hops.
    promote_to_published(&workdir, slug);

    // 5. add-target → metrics update. `metrics update` requires a
    // target already in the post's targets[]; `add-target` (added in
    // #527) is how that target gets there. Before #527 this test
    // hand-injected a TargetEntry via Post::render; now the same path
    // exercises the real command.
    commands::add_target::run(
        &FakeJj::new(),
        AddTargetArgs {
            slug: slug.into(),
            workdir: workdir.clone(),
            target: Target::Linkedin,
            no_sync: true,
        },
    )
    .expect("add-target");

    commands::metrics::update(
        &FakeJj::new(),
        UpdateArgs {
            slug: slug.into(),
            workdir: workdir.clone(),
            target: Target::Linkedin,
            impressions: 1_234,
            reactions: 42,
            comments: 7,
            reposts: 3,
            sampled_at: None,
            no_sync: true,
        },
    )
    .expect("metrics update");

    let repo = Repository::open(Workdir::new(&workdir)).expect("reopen final");
    let (_, post) = repo.load(slug).expect("load final");
    let linkedin = post
        .metadata
        .targets
        .iter()
        .find(|t| matches!(t.name, Target::Linkedin))
        .expect("linkedin target present");
    let m = linkedin.metrics.as_ref().expect("metrics block populated");
    assert_eq!(m.impressions, 1_234);
    assert_eq!(m.reactions, 42);
    assert_eq!(m.comments, 7);
    assert_eq!(m.reposts, 3);
}

const DRAFT_REPLY: &str =
    "## Drafted body\n\nFirst paragraph from the mock.\n\nSecond paragraph from the mock.\n";
const REFINE_REPLY: &str =
    "## Refined body\n\nTighter first paragraph.\n\nTighter second paragraph.\n";
const FINAL_EDIT_REPLY: &str =
    "Polished body from the mock.\n\nNo markdown, just words and double line breaks.\n";

/// Drive the AI-touching path of the lifecycle end-to-end against a
/// `mockito` server: draft into an Ideation post, promote to Editing,
/// refine, promote to FinalEditing, final-edit. Asserts on body
/// replacement and the `ai.draft` / `ai.refine` / `ai.final_edit`
/// audit blocks. Also exercises the negative paths (draft from
/// Concept, refine from Ideation, final-edit from Editing) so the
/// stage-gating regressions land here too.
#[test]
fn lifecycle_with_ai_commands_uses_mocked_openrouter() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut server = mockito::Server::new();
    let mock_draft = mock_chat_completion(&mut server, DRAFT_REPLY);
    let mock_refine = mock_chat_completion(&mut server, REFINE_REPLY);
    let mock_final_edit = mock_chat_completion(&mut server, FINAL_EDIT_REPLY);

    let prev_base = env::var("OPENROUTER_API_BASE").ok();
    let prev_key = env::var("OPENROUTER_API_KEY").ok();
    env::set_var(
        "OPENROUTER_API_BASE",
        format!("{}/chat/completions", server.url()),
    );
    env::set_var("OPENROUTER_API_KEY", "test-key");

    let result = run_ai_lifecycle();

    match prev_base {
        Some(v) => env::set_var("OPENROUTER_API_BASE", v),
        None => env::remove_var("OPENROUTER_API_BASE"),
    }
    match prev_key {
        Some(v) => env::set_var("OPENROUTER_API_KEY", v),
        None => env::remove_var("OPENROUTER_API_KEY"),
    }

    result.expect("ai lifecycle should succeed");
    mock_draft.assert();
    mock_refine.assert();
    mock_final_edit.assert();
}

fn run_ai_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let (_tmp, workdir) = fresh_workdir();
    commands::init::run(&FakeJj::new(), workdir.clone(), true)?;
    commands::new::run(
        &FakeJj::new(),
        "Ai test post".into(),
        workdir.clone(),
        None,
        Kind::Post,
        None,
        true,
    )?;
    let slug = "ai-test-post";

    // Draft at Concept must fail with DraftWrongStage. Catch the
    // negative path here so the stage-gating regression doesn't need
    // its own standalone test.
    let err = commands::draft::run(
        &FakeJj::new(),
        DraftArgs {
            slug: slug.into(),
            workdir: workdir.clone(),
            prompt: "go".into(),
            model: "test-model".into(),
            no_sync: true,
        },
    )
    .expect_err("draft at Concept must error");
    assert!(
        matches!(err, Error::DraftWrongStage { .. }),
        "expected DraftWrongStage, got: {err:?}"
    );

    // Concept → Ideation, then draft.
    commands::promote::run(&FakeJj::new(), slug.to_string(), workdir.clone(), true)?;
    commands::draft::run(
        &FakeJj::new(),
        DraftArgs {
            slug: slug.into(),
            workdir: workdir.clone(),
            prompt: "write me three paragraphs about mocking".into(),
            model: "test-model".into(),
            no_sync: true,
        },
    )?;

    let repo = Repository::open(Workdir::new(&workdir))?;
    let (_, post) = repo.load(slug)?;
    assert_eq!(
        post.body.trim(),
        DRAFT_REPLY.trim(),
        "body should match the canned draft reply"
    );
    let ai = post
        .metadata
        .ai
        .as_ref()
        .expect("ai block populated post-draft");
    assert!(ai.draft.is_some(), "ai.draft set post-draft");
    assert!(ai.refine.is_none(), "ai.refine not set yet");

    // Refine at Ideation must fail with RefineWrongStage. Same
    // pattern as the draft check above.
    let err = commands::refine::run(
        &FakeJj::new(),
        RefineArgs {
            slug: slug.into(),
            workdir: workdir.clone(),
            prompt: "tighten".into(),
            model: "test-model".into(),
            no_sync: true,
        },
    )
    .expect_err("refine at Ideation must error");
    assert!(
        matches!(err, Error::RefineWrongStage { .. }),
        "expected RefineWrongStage, got: {err:?}"
    );

    // Ideation → Editing, then refine.
    commands::promote::run(&FakeJj::new(), slug.to_string(), workdir.clone(), true)?;
    commands::refine::run(
        &FakeJj::new(),
        RefineArgs {
            slug: slug.into(),
            workdir: workdir.clone(),
            prompt: "tighten paragraphs".into(),
            model: "test-model".into(),
            no_sync: true,
        },
    )?;

    let repo = Repository::open(Workdir::new(&workdir))?;
    let (_, post) = repo.load(slug)?;
    assert_eq!(
        post.body.trim(),
        REFINE_REPLY.trim(),
        "body should match the canned refine reply"
    );
    let ai = post
        .metadata
        .ai
        .as_ref()
        .expect("ai block populated post-refine");
    let draft = ai.draft.as_ref().expect("ai.draft preserved across refine");
    assert_eq!(draft.prompt, "write me three paragraphs about mocking");
    let refine = ai.refine.as_ref().expect("ai.refine set post-refine");
    assert_eq!(refine.prompt, "tighten paragraphs");
    assert_eq!(refine.model, "test-model");

    // FinalEdit at Editing must fail with FinalEditWrongStage. Same
    // stage-gating pattern as the draft/refine checks above.
    let err = commands::final_edit::run(
        &FakeJj::new(),
        FinalEditArgs {
            slug: slug.into(),
            workdir: workdir.clone(),
            prompt: None,
            model: "test-model".into(),
            no_sync: true,
        },
    )
    .expect_err("final-edit at Editing must error");
    assert!(
        matches!(err, Error::FinalEditWrongStage { .. }),
        "expected FinalEditWrongStage, got: {err:?}"
    );

    // Editing → FinalEditing, then final-edit (using the baked-in
    // LinkedIn polish prompt since --prompt is None).
    commands::promote::run(&FakeJj::new(), slug.to_string(), workdir.clone(), true)?;
    commands::final_edit::run(
        &FakeJj::new(),
        FinalEditArgs {
            slug: slug.into(),
            workdir: workdir.clone(),
            prompt: None,
            model: "test-model".into(),
            no_sync: true,
        },
    )?;

    let repo = Repository::open(Workdir::new(&workdir))?;
    let (_, post) = repo.load(slug)?;
    assert_eq!(
        post.body.trim(),
        FINAL_EDIT_REPLY.trim(),
        "body should match the canned final-edit reply"
    );
    let ai = post
        .metadata
        .ai
        .as_ref()
        .expect("ai block populated post-final-edit");
    let draft = ai
        .draft
        .as_ref()
        .expect("ai.draft preserved across final-edit");
    assert_eq!(draft.prompt, "write me three paragraphs about mocking");
    let refine = ai
        .refine
        .as_ref()
        .expect("ai.refine preserved across final-edit");
    assert_eq!(refine.prompt, "tighten paragraphs");
    let fe = ai
        .final_edit
        .as_ref()
        .expect("ai.final_edit set post-final-edit");
    // The default polish prompt should have been recorded in the audit
    // block since --prompt was None.
    assert!(
        fe.prompt.to_lowercase().contains("linkedin"),
        "default polish prompt should be stored in ai.final_edit: {}",
        fe.prompt
    );
    assert_eq!(fe.model, "test-model");
    Ok(())
}
