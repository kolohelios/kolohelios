//! Integration tests for the per-command sync hook. Tests use
//! `FakeJj` to assert on the order/shape of `jj` calls without
//! shelling out, so they run anywhere blogctl builds.
//!
//! Each test exercises one write-shaped command via the library API
//! and asserts on the call sequence; the actual `jj` binary is never
//! invoked.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use blogctl::commands;
use blogctl::kind::Kind;
use blogctl::storage::{Repository, Workdir};
use blogctl::sync::{Call, FakeJj, PushOutcome};

fn workdir() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().to_path_buf();
    (tmp, path)
}

/// Verify the canonical 6-call shape one write-shaped command produces:
/// Status, Fetch, Rebase, NewChange, SetBookmark, Push (in order).
fn assert_full_sync_flow(calls: &[Call], expected_message: &str) {
    assert_eq!(calls.len(), 6, "expected 6 jj calls, got: {calls:?}");
    assert!(
        matches!(calls[0], Call::Status { .. }),
        "first call must be Status, got: {:?}",
        calls[0]
    );
    assert!(
        matches!(calls[1], Call::Fetch { .. }),
        "second call must be Fetch, got: {:?}",
        calls[1]
    );
    assert!(
        matches!(calls[2], Call::Rebase { .. }),
        "third call must be Rebase, got: {:?}",
        calls[2]
    );
    match &calls[3] {
        Call::NewChange { message, .. } => {
            assert_eq!(message, expected_message, "wrong commit message",)
        }
        other => panic!("fourth call must be NewChange, got: {other:?}"),
    }
    assert!(
        matches!(calls[4], Call::SetBookmark { ref bookmark, .. } if bookmark == "main"),
        "fifth call must be SetBookmark main, got: {:?}",
        calls[4]
    );
    assert!(
        matches!(
            calls[5],
            Call::Push {
                ref remote,
                ref bookmark,
                ..
            } if remote == "origin" && bookmark == "main"
        ),
        "sixth call must be Push origin main, got: {:?}",
        calls[5]
    );
}

#[test]
fn init_drives_full_sync_with_scaffold_message() {
    let jj = FakeJj::new();
    let (_tmp, path) = workdir();
    commands::init::run(&jj, path.clone(), false).unwrap();
    // Even though the workdir isn't a real jj repo, FakeJj answers Ok
    // unconditionally, so the full flow runs against the fake.
    assert_full_sync_flow(&jj.calls(), "chore: scaffold workdir");
    // And the actual file write happened.
    assert!(path.join(".blog-os.toml").is_file());
}

#[test]
fn new_drives_full_sync_with_draft_message() {
    let (_tmp, path) = workdir();
    Repository::unchecked(Workdir::new(&path)).init().unwrap();
    let jj = FakeJj::new();
    commands::new::run(
        &jj,
        "Hello World".to_string(),
        path.clone(),
        None,
        Kind::Post,
        None,
        false,
    )
    .unwrap();
    assert_full_sync_flow(&jj.calls(), "post(hello-world): draft \"Hello World\"");
    assert!(path.join("concepts/hello-world.md").is_file());
}

#[test]
fn promote_drives_full_sync_with_stage_transition_message() {
    let (_tmp, path) = workdir();
    let init_jj = FakeJj::new();
    commands::init::run(&init_jj, path.clone(), false).unwrap();
    commands::new::run(
        &FakeJj::new(),
        "Hello World".to_string(),
        path.clone(),
        None,
        Kind::Post,
        None,
        false,
    )
    .unwrap();

    let jj = FakeJj::new();
    commands::promote::run(&jj, "hello-world".to_string(), path.clone(), false).unwrap();
    // The arrow is U+2192 RIGHTWARDS ARROW per the issue spec.
    assert_full_sync_flow(&jj.calls(), "post(hello-world): concept \u{2192} ideation");
    assert!(path.join("ideation/hello-world.md").is_file());
    assert!(!path.join("concepts/hello-world.md").exists());
}

#[test]
fn demote_drives_full_sync_with_reverse_arrow_message() {
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();
    commands::new::run(
        &FakeJj::new(),
        "Hello".to_string(),
        path.clone(),
        None,
        Kind::Post,
        None,
        false,
    )
    .unwrap();
    commands::promote::run(&FakeJj::new(), "hello".to_string(), path.clone(), false).unwrap();

    let jj = FakeJj::new();
    commands::demote::run(&jj, "hello".to_string(), path.clone(), false).unwrap();
    // U+2190 LEFTWARDS ARROW.
    assert_full_sync_flow(&jj.calls(), "post(hello): ideation \u{2190} concept");
}

#[test]
fn readme_regenerate_drives_full_sync_with_docs_message() {
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();

    let jj = FakeJj::new();
    commands::readme::regenerate(&jj, path, false).unwrap();
    assert_full_sync_flow(&jj.calls(), "docs: regenerate workdir README");
}

#[test]
fn fix_drives_full_sync_with_finding_count_message() {
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();
    // Plant a stage-mismatched post so fix has something to repair.
    let mismatch = concat!(
        "---\n",
        "title: \"oops\"\n",
        "slug: oops\n",
        "kind: post\n",
        "theme: standard\n",
        "status: editing\n",
        "created_at: 2026-05-03T00:00:00Z\n",
        "updated_at: 2026-05-03T00:00:00Z\n",
        "tags: []\n",
        "todoist_task_id: null\n",
        "history_checked: false\n",
        "---\n",
        "body\n",
    );
    fs::write(path.join("concepts/oops.md"), mismatch).unwrap();

    let jj = FakeJj::new();
    commands::fix::run(&jj, path.clone(), false, false).unwrap();
    // Exactly one repair attempted (the stage mismatch).
    assert_full_sync_flow(&jj.calls(), "chore: fix workdir (1 findings)");
}

#[test]
fn fix_dry_run_skips_sync_entirely() {
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();
    fs::write(
        path.join("concepts/oops.md"),
        concat!(
            "---\n",
            "title: \"oops\"\n",
            "slug: oops\n",
            "kind: post\n",
            "theme: standard\n",
            "status: editing\n",
            "created_at: 2026-05-03T00:00:00Z\n",
            "updated_at: 2026-05-03T00:00:00Z\n",
            "tags: []\n",
            "todoist_task_id: null\n",
            "history_checked: false\n",
            "---\n",
            "body\n",
        ),
    )
    .unwrap();

    let jj = FakeJj::new();
    let _ = commands::fix::run(&jj, path, true, false);
    assert!(
        jj.calls().is_empty(),
        "dry-run must not invoke jj: {:?}",
        jj.calls()
    );
}

#[test]
fn no_sync_flag_skips_jj_orchestration() {
    let jj = FakeJj::new();
    let (_tmp, path) = workdir();
    commands::init::run(&jj, path.clone(), /* no_sync = */ true).unwrap();
    // The write happened.
    assert!(path.join(".blog-os.toml").is_file());
    // But not a single jj call fired.
    assert!(jj.calls().is_empty(), "got jj calls: {:?}", jj.calls());
}

#[test]
fn config_disabled_sync_skips_jj_orchestration() {
    let (_tmp, path) = workdir();
    // Init the workdir first (with sync enabled by default for that
    // invocation), then disable sync in the config.
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();
    let cfg_path = path.join(".blog-os.toml");
    let mut raw = fs::read_to_string(&cfg_path).unwrap();
    raw = raw.replace("enabled = true", "enabled = false");
    fs::write(&cfg_path, raw).unwrap();

    let jj = FakeJj::new();
    commands::new::run(
        &jj,
        "Hi".to_string(),
        path,
        None,
        Kind::Post,
        None,
        /* no_sync = */ false,
    )
    .unwrap();
    assert!(
        jj.calls().is_empty(),
        "got jj calls despite [sync] enabled=false: {:?}",
        jj.calls()
    );
}

#[test]
fn push_failure_does_not_fail_the_command() {
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();

    let jj = FakeJj::new().with_push_outcome(PushOutcome::Failed("non-fast-forward".into()));
    let result = commands::new::run(
        &jj,
        "Hi".to_string(),
        path.clone(),
        None,
        Kind::Post,
        None,
        false,
    );
    assert!(
        result.is_ok(),
        "push-failure must not fail the command: {result:?}"
    );
    // The file write still happened.
    assert!(path.join("concepts/hi.md").is_file());
    // And we did go through the full sync flow including the failed push.
    let calls = jj.calls();
    assert_eq!(calls.len(), 6);
    assert!(matches!(calls[5], Call::Push { .. }));
}

#[test]
fn doctor_surfaces_stale_unpushed_commits_finding() {
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();

    // Simulate "push has been failing for 48h" by having FakeJj
    // report 3 unpushed commits, oldest 48h old.
    let jj = FakeJj::new().with_unpushed_summary(Some(blogctl::sync::UnpushedSummary {
        count: 3,
        oldest_age_hours: 48,
    }));
    let err = commands::doctor::run(&jj, path).unwrap_err();
    assert!(
        matches!(err, blogctl::Error::WorkdirUnhealthy(n) if n == 1),
        "expected WorkdirUnhealthy(1), got: {err:?}"
    );
}

#[test]
fn doctor_does_not_surface_stale_finding_when_push_is_current() {
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();

    // No unpushed commits → no sync finding → doctor is happy.
    let jj = FakeJj::new().with_unpushed_summary(None);
    commands::doctor::run(&jj, path).expect("clean workdir should be healthy");
}

#[test]
fn classify_drives_full_sync_with_changed_dimensions_in_message() {
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();
    commands::new::run(
        &FakeJj::new(),
        "Hello".to_string(),
        path.clone(),
        None,
        Kind::Post,
        None,
        false,
    )
    .unwrap();

    let jj = FakeJj::new();
    commands::classify::run(
        &jj,
        commands::classify::ClassifyArgs {
            slug: "hello".into(),
            workdir: path.clone(),
            format: Some("thesis".into()),
            hook: Some("contradiction".into()),
            theme: vec!["ambiguity".into(), "delivery".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_full_sync_flow(&jj.calls(), "post(hello): classify (format,hook,theme)");

    // And the post on disk now carries the new classifications.
    let raw = fs::read_to_string(path.join("concepts/hello.md")).unwrap();
    assert!(raw.contains("format: thesis"));
    assert!(raw.contains("hook: contradiction"));
    assert!(raw.contains("ambiguity"));
    assert!(raw.contains("delivery"));
}

#[test]
fn classify_with_invalid_value_fails_before_any_write() {
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();
    commands::new::run(
        &FakeJj::new(),
        "Hi".to_string(),
        path.clone(),
        None,
        Kind::Post,
        None,
        false,
    )
    .unwrap();

    let pre = fs::read_to_string(path.join("concepts/hi.md")).unwrap();

    let jj = FakeJj::new();
    let err = commands::classify::run(
        &jj,
        commands::classify::ClassifyArgs {
            slug: "hi".into(),
            workdir: path.clone(),
            format: Some("not-a-format".into()),
            ..Default::default()
        },
    )
    .expect_err("invalid value must hard-fail");
    assert!(
        matches!(err, blogctl::Error::InvalidClassification { ref dimension, .. } if dimension == "format"),
        "got: {err:?}"
    );

    // File on disk is unchanged AND no jj calls fired.
    let post = fs::read_to_string(path.join("concepts/hi.md")).unwrap();
    assert_eq!(post, pre, "file must be untouched");
    assert!(
        jj.calls().is_empty(),
        "jj must not be called: {:?}",
        jj.calls()
    );
}

#[test]
fn classify_with_no_changes_skips_sync() {
    // Set classifications once, then run classify again with the same
    // values — nothing changes, so no commit, no push, no jj noise.
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();
    commands::new::run(
        &FakeJj::new(),
        "Stable".to_string(),
        path.clone(),
        None,
        Kind::Post,
        None,
        false,
    )
    .unwrap();

    commands::classify::run(
        &FakeJj::new(),
        commands::classify::ClassifyArgs {
            slug: "stable".into(),
            workdir: path.clone(),
            format: Some("thesis".into()),
            ..Default::default()
        },
    )
    .unwrap();

    // Second invocation with the same value → no-op.
    let jj = FakeJj::new();
    commands::classify::run(
        &jj,
        commands::classify::ClassifyArgs {
            slug: "stable".into(),
            workdir: path.clone(),
            format: Some("thesis".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        jj.calls().is_empty(),
        "no-op classify must not call jj: {:?}",
        jj.calls()
    );
}

#[test]
fn classify_can_repair_post_whose_existing_classification_is_invalid() {
    // The user typo'd `format: thesys` in the past (or removed a value
    // from the taxonomy). `classify` must still be able to load and
    // rewrite the post — otherwise the only fix tool is blocked by
    // the problem it's meant to fix.
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();
    commands::new::run(
        &FakeJj::new(),
        "Repair".to_string(),
        path.clone(),
        None,
        Kind::Post,
        None,
        false,
    )
    .unwrap();
    // Plant the typo by hand-editing the file (simulates a stale post).
    let post_path = path.join("concepts/repair.md");
    let raw = fs::read_to_string(&post_path).unwrap();
    let with_typo = raw.replace(
        "tags: []\n",
        "tags: []\nclassifications:\n  format: thesys\n",
    );
    fs::write(&post_path, with_typo).unwrap();

    // Now classify with a valid value — should succeed and overwrite.
    let jj = FakeJj::new();
    commands::classify::run(
        &jj,
        commands::classify::ClassifyArgs {
            slug: "repair".into(),
            workdir: path.clone(),
            format: Some("thesis".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let fixed = fs::read_to_string(&post_path).unwrap();
    assert!(fixed.contains("format: thesis"));
    assert!(!fixed.contains("thesys"));
}

#[test]
fn rebase_conflict_is_a_hard_error_and_aborts_the_write() {
    let (_tmp, path) = workdir();
    commands::init::run(&FakeJj::new(), path.clone(), false).unwrap();

    let jj = FakeJj::new().with_rebase_outcome(blogctl::sync::RebaseOutcome::Conflicted);
    let result = commands::new::run(
        &jj,
        "Hi".to_string(),
        path.clone(),
        None,
        Kind::Post,
        None,
        false,
    );
    let err = result.expect_err("rebase conflict must hard-fail");
    assert!(
        matches!(err, blogctl::Error::SyncRebaseConflict { .. }),
        "got: {err:?}"
    );
    // Crucially: the file write did NOT happen (the closure never ran).
    assert!(
        !path.join("concepts/hi.md").exists(),
        "post file must not exist after a rebase-conflict abort"
    );
    // Call sequence stopped after the rebase.
    let calls = jj.calls();
    assert_eq!(calls.len(), 3);
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, Call::NewChange { .. } | Call::Push { .. })),
        "must not have called NewChange or Push: {calls:?}"
    );
}
