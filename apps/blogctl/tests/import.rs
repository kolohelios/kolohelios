//! Integration tests for `blogctl import`. The happy path goes
//! through `commands::import::run` against a fresh tempdir workdir;
//! the body comes from the canonical fixture under
//! `tests/fixtures/import/`. The slug-collision path goes through the
//! same command run twice to confirm the existing `DuplicateSlug`
//! invariant fires.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use blogctl::commands::{self, import::ImportArgs};
use blogctl::error::Error;
use blogctl::kind::Kind;
use blogctl::stage::Stage;
use blogctl::storage::{Repository, Workdir};
use blogctl::sync::FakeJj;
use blogctl::target::{Target, TargetStatus};

fn fresh_workdir() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();
    (tmp, path)
}

fn fixture_body_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/import/linkedin-sample.md")
}

fn import_args(workdir: &Path) -> ImportArgs {
    ImportArgs {
        title: "Backfill is the shape".into(),
        workdir: workdir.to_path_buf(),
        slug: None,
        kind: Kind::Post,
        theme: None,
        target: Target::Linkedin,
        url: "https://www.linkedin.com/posts/example".into(),
        published_at: "2026-05-08T14:32:00Z".into(),
        body_file: fixture_body_path(),
        tags: vec!["meta".into(), "tooling".into()],
        no_sync: true,
    }
}

#[test]
fn import_writes_published_post_with_target_entry() {
    let (_tmp, workdir) = fresh_workdir();
    commands::init::run(&FakeJj::new(), workdir.clone(), true).expect("init");

    commands::import::run(&FakeJj::new(), import_args(&workdir)).expect("import");

    let repo = Repository::open(Workdir::new(&workdir)).expect("reopen");
    let (handle, post) = repo
        .load("backfill-is-the-shape")
        .expect("load imported post");

    assert_eq!(handle.stage, Stage::Published);
    assert_eq!(post.metadata.status, Stage::Published);
    assert_eq!(post.metadata.title, "Backfill is the shape");
    assert_eq!(post.metadata.slug, "backfill-is-the-shape");
    assert_eq!(post.metadata.kind, Kind::Post);
    assert_eq!(post.metadata.tags, vec!["meta", "tooling"]);
    assert_eq!(post.metadata.targets.len(), 1);

    let entry = &post.metadata.targets[0];
    assert_eq!(entry.name, Target::Linkedin);
    assert_eq!(entry.status, TargetStatus::Published);
    assert_eq!(
        entry.url.as_deref(),
        Some("https://www.linkedin.com/posts/example")
    );
    assert!(entry.published_at.is_some());

    // created_at == updated_at == --published-at: the editorial pipeline
    // never touched this post, so timestamps should collapse to the
    // moment it actually went live on LinkedIn.
    assert_eq!(post.metadata.created_at, post.metadata.updated_at);
    assert_eq!(Some(post.metadata.created_at), entry.published_at);

    assert!(post.body.contains("Most teams treat backfill"));
    assert!(post.body.contains("Build the importer once"));
}

#[test]
fn import_with_slug_override_uses_provided_slug() {
    let (_tmp, workdir) = fresh_workdir();
    commands::init::run(&FakeJj::new(), workdir.clone(), true).expect("init");

    let mut args = import_args(&workdir);
    args.slug = Some("custom-name".into());
    commands::import::run(&FakeJj::new(), args).expect("import");

    let repo = Repository::open(Workdir::new(&workdir)).expect("reopen");
    let (_handle, post) = repo.load("custom-name").expect("load custom-named post");
    assert_eq!(post.metadata.slug, "custom-name");
}

#[test]
fn import_refuses_when_slug_already_exists() {
    let (_tmp, workdir) = fresh_workdir();
    commands::init::run(&FakeJj::new(), workdir.clone(), true).expect("init");

    commands::import::run(&FakeJj::new(), import_args(&workdir)).expect("first import");
    let second = commands::import::run(&FakeJj::new(), import_args(&workdir));

    match second {
        Err(Error::DuplicateSlug { slug, .. }) => {
            assert_eq!(slug, "backfill-is-the-shape");
        }
        other => panic!("expected DuplicateSlug, got {other:?}"),
    }
}

#[test]
fn import_rejects_malformed_published_at() {
    let (_tmp, workdir) = fresh_workdir();
    commands::init::run(&FakeJj::new(), workdir.clone(), true).expect("init");

    let mut args = import_args(&workdir);
    args.published_at = "not-a-date".into();
    let result = commands::import::run(&FakeJj::new(), args);

    match result {
        Err(Error::InvalidPublishedAt { value, .. }) => {
            assert_eq!(value, "not-a-date");
        }
        other => panic!("expected InvalidPublishedAt, got {other:?}"),
    }
}

#[test]
fn import_rejects_unknown_theme() {
    let (_tmp, workdir) = fresh_workdir();
    commands::init::run(&FakeJj::new(), workdir.clone(), true).expect("init");

    let mut args = import_args(&workdir);
    args.theme = Some("noir".into());
    let result = commands::import::run(&FakeJj::new(), args);

    match result {
        Err(Error::UnknownTheme { theme, .. }) => assert_eq!(theme, "noir"),
        other => panic!("expected UnknownTheme, got {other:?}"),
    }
}
