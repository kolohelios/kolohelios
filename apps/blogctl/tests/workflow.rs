//! End-to-end lifecycle test: drives a single post from `init → new →
//! classify → promote → metrics` against a fresh tempdir workdir.
//!
//! This catches the kind of regression where one command's output stops
//! matching the next command's expected input (frontmatter rename, stage
//! directory rename, taxonomy mismatch) — gaps unit tests miss because
//! each one stays inside one module's contract.
//!
//! No AI here. `commands::ai::ping` is the only OpenRouter call site
//! today and isn't wired into the lifecycle; the seam from #474 is
//! exercised separately by `tests/ai_endpoint.rs`. Once the
//! AI-integrated commands tracked in #483 land (`draft`, `refine`,
//! `final-edit`), they'll slot into this file rather than a new one.

use std::path::PathBuf;

use tempfile::TempDir;

use blogctl::commands::{self, classify::ClassifyArgs, metrics::UpdateArgs};
use blogctl::kind::Kind;
use blogctl::stage::Stage;
use blogctl::storage::{Repository, Workdir};
use blogctl::sync::FakeJj;
use blogctl::target::{Target, TargetEntry, TargetStatus};

fn fresh_workdir() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();
    (tmp, path)
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

    // 5. metrics — `update` requires a target already in the post's
    // targets[]. Adding a target is currently a manual frontmatter edit
    // (no `add-target` command exists; tracked in #483 as part of the
    // AI-integrated workflow gap survey). We rewrite the post directly
    // to seed a `planned` linkedin target so metrics has something to
    // update.
    let repo = Repository::open(Workdir::new(&workdir)).expect("reopen pre-metrics");
    let (handle, mut post) = repo.load_raw(slug).expect("load_raw pre-metrics");
    post.metadata.targets.push(TargetEntry {
        name: Target::Linkedin,
        status: TargetStatus::Planned,
        url: None,
        published_at: None,
        metrics: None,
    });
    std::fs::write(&handle.path, post.render().expect("render")).expect("write seeded target");

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
