//! Integration tests for `blogctl backfill --import`. Builds a real
//! workdir, drops a JSON import file alongside it, runs the command,
//! and asserts on the resulting frontmatter.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use blogctl::commands;
use blogctl::kind::Kind;
use blogctl::sync::FakeJj;

fn add_published_linkedin_target(post_path: &Path) {
    let raw = fs::read_to_string(post_path).unwrap();
    let updated = raw.replace(
        "tags: []\n",
        concat!(
            "tags: []\n",
            "targets:\n",
            "  - name: linkedin\n",
            "    status: published\n",
            "    url: https://www.linkedin.com/posts/example\n",
            "    published_at: 2026-05-08T14:32:00Z\n",
        ),
    );
    fs::write(post_path, updated).unwrap();
}

fn fixture_workdir() -> TempDir {
    let tmp = TempDir::new().unwrap();
    commands::init::run(&FakeJj::new(), tmp.path().to_path_buf(), false).unwrap();
    // One bare post with a LinkedIn target but no metrics + no
    // classifications. Backfill should fill them in.
    commands::new::run(
        &FakeJj::new(),
        "Bare Post".to_string(),
        tmp.path().to_path_buf(),
        Some("bare-post".to_string()),
        Kind::Post,
        None,
        false,
    )
    .unwrap();
    add_published_linkedin_target(&tmp.path().join("concepts/bare-post.md"));
    tmp
}

fn write_import(workdir: &Path, contents: &str) -> std::path::PathBuf {
    let path = workdir.join("backfill.json");
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn import_fills_classifications_and_metrics_for_bare_post() {
    let tmp = fixture_workdir();
    let import = write_import(
        tmp.path(),
        r#"[
            {
                "slug": "bare-post",
                "classifications": {
                    "format": "thesis",
                    "hook": "contradiction",
                    "theme": ["ambiguity"]
                },
                "metrics": {
                    "linkedin": {
                        "impressions": 1842,
                        "reactions": 67,
                        "comments": 14,
                        "reposts": 5,
                        "sampled_at": "2026-05-14T00:00:00Z"
                    }
                }
            }
        ]"#,
    );

    commands::backfill::run(
        &FakeJj::new(),
        commands::backfill::BackfillArgs {
            workdir: tmp.path().to_path_buf(),
            import: Some(import),
            no_sync: false,
        },
    )
    .unwrap();

    let raw = fs::read_to_string(tmp.path().join("concepts/bare-post.md")).unwrap();
    assert!(raw.contains("format: thesis"));
    assert!(raw.contains("hook: contradiction"));
    assert!(raw.contains("ambiguity"));
    assert!(raw.contains("impressions: 1842"));
    assert!(raw.contains("sampled_at: 2026-05-14T00:00:00Z"));
}

#[test]
fn import_re_run_is_idempotent_no_extra_writes() {
    let tmp = fixture_workdir();
    let import = write_import(
        tmp.path(),
        r#"[
            {
                "slug": "bare-post",
                "classifications": { "format": "thesis" }
            }
        ]"#,
    );
    commands::backfill::run(
        &FakeJj::new(),
        commands::backfill::BackfillArgs {
            workdir: tmp.path().to_path_buf(),
            import: Some(import.clone()),
            no_sync: false,
        },
    )
    .unwrap();
    let raw_after_first = fs::read_to_string(tmp.path().join("concepts/bare-post.md")).unwrap();

    // Second run with the same JSON — should be a complete no-op
    // because every field already matches.
    let jj = FakeJj::new();
    commands::backfill::run(
        &jj,
        commands::backfill::BackfillArgs {
            workdir: tmp.path().to_path_buf(),
            import: Some(import),
            no_sync: false,
        },
    )
    .unwrap();
    let raw_after_second = fs::read_to_string(tmp.path().join("concepts/bare-post.md")).unwrap();
    assert_eq!(
        raw_after_first, raw_after_second,
        "second import on already-applied data should not modify the file"
    );
    // And nothing went through the jj graph on the second run.
    assert!(
        jj.calls().is_empty(),
        "no-op backfill must not produce jj calls: {:?}",
        jj.calls()
    );
}

#[test]
fn import_with_unknown_slug_warns_continues_and_returns_partial_failure() {
    let tmp = fixture_workdir();
    let import = write_import(
        tmp.path(),
        r#"[
            { "slug": "bare-post", "classifications": { "format": "thesis" } },
            { "slug": "does-not-exist", "classifications": { "format": "essay" } }
        ]"#,
    );
    let result = commands::backfill::run(
        &FakeJj::new(),
        commands::backfill::BackfillArgs {
            workdir: tmp.path().to_path_buf(),
            import: Some(import),
            no_sync: false,
        },
    );
    let err = result.expect_err("unknown slug should produce a partial-failure error");
    assert!(
        matches!(err, blogctl::Error::BackfillPartialFailure { warnings: n } if n == 1),
        "got: {err:?}"
    );
    // Partial progress preserved: the known slug got its update.
    let raw = fs::read_to_string(tmp.path().join("concepts/bare-post.md")).unwrap();
    assert!(raw.contains("format: thesis"));
}

#[test]
fn import_with_metrics_for_missing_target_warns() {
    let tmp = fixture_workdir();
    // bare-post has a LinkedIn target but no Blog target.
    let import = write_import(
        tmp.path(),
        r#"[
            {
                "slug": "bare-post",
                "metrics": {
                    "blog": {
                        "impressions": 100,
                        "reactions": 5,
                        "comments": 0,
                        "reposts": 0,
                        "sampled_at": "2026-05-14T00:00:00Z"
                    }
                }
            }
        ]"#,
    );
    let result = commands::backfill::run(
        &FakeJj::new(),
        commands::backfill::BackfillArgs {
            workdir: tmp.path().to_path_buf(),
            import: Some(import),
            no_sync: false,
        },
    );
    let err = result.expect_err("missing target should produce a partial-failure error");
    assert!(
        matches!(err, blogctl::Error::BackfillPartialFailure { warnings: n } if n == 1),
        "got: {err:?}"
    );
}

#[test]
fn import_with_invalid_classification_warns() {
    let tmp = fixture_workdir();
    let import = write_import(
        tmp.path(),
        r#"[
            { "slug": "bare-post", "classifications": { "format": "made-up-format" } }
        ]"#,
    );
    let result = commands::backfill::run(
        &FakeJj::new(),
        commands::backfill::BackfillArgs {
            workdir: tmp.path().to_path_buf(),
            import: Some(import),
            no_sync: false,
        },
    );
    let err = result.expect_err("invalid value should produce a partial-failure error");
    assert!(
        matches!(err, blogctl::Error::BackfillPartialFailure { warnings: n } if n == 1),
        "got: {err:?}"
    );
    // File on disk must be untouched (the entry errored before write).
    let raw = fs::read_to_string(tmp.path().join("concepts/bare-post.md")).unwrap();
    assert!(!raw.contains("made-up-format"));
}

#[test]
fn import_full_batch_lands_in_one_commit() {
    let tmp = fixture_workdir();
    // Add a second bare post so we have two entries.
    commands::new::run(
        &FakeJj::new(),
        "Second".to_string(),
        tmp.path().to_path_buf(),
        Some("second".to_string()),
        Kind::Post,
        None,
        false,
    )
    .unwrap();
    add_published_linkedin_target(&tmp.path().join("concepts/second.md"));

    let import = write_import(
        tmp.path(),
        r#"[
            { "slug": "bare-post", "classifications": { "format": "thesis" } },
            { "slug": "second",    "classifications": { "format": "essay" } }
        ]"#,
    );
    let jj = FakeJj::new();
    commands::backfill::run(
        &jj,
        commands::backfill::BackfillArgs {
            workdir: tmp.path().to_path_buf(),
            import: Some(import),
            no_sync: false,
        },
    )
    .unwrap();

    // One commit covers the whole batch.
    let calls = jj.calls();
    let new_changes: Vec<_> = calls
        .iter()
        .filter(|c| matches!(c, blogctl::sync::Call::NewChange { .. }))
        .collect();
    assert_eq!(
        new_changes.len(),
        1,
        "expected exactly one commit for the batch, got: {calls:?}",
    );
    if let blogctl::sync::Call::NewChange { message, .. } = new_changes[0] {
        assert_eq!(message, "chore: backfill 2 posts");
    }
}

/// Build a fixture workdir, promote `bare-post` all the way to
/// published, so the interactive walk actually sees it.
fn fixture_workdir_with_published() -> TempDir {
    let tmp = fixture_workdir();
    // Promote bare-post: concept → ideation → editing → final-editing
    // → published. classify + metrics would normally be done at
    // various stages; here we just walk it through.
    for _ in 0..4 {
        commands::promote::run(
            &FakeJj::new(),
            "bare-post".to_string(),
            tmp.path().to_path_buf(),
            false,
        )
        .unwrap();
    }
    tmp
}

#[test]
fn interactive_skips_dimensions_via_blank_input_then_quits() {
    // Drive: every classification prompt → empty line (skip), then
    // metrics prompts: skip the whole post (S). One published post,
    // no changes, exit clean.
    let tmp = fixture_workdir_with_published();
    // 5 single-valued dimensions, each prompted; skip each. Then
    // metrics block — skip the whole post.
    let input_bytes = b"\n\n\n\n\nS\n";
    let mut input = std::io::Cursor::new(&input_bytes[..]);
    let mut output: Vec<u8> = Vec::new();
    commands::backfill::run_interactive(
        &FakeJj::new(),
        &commands::backfill::BackfillArgs {
            workdir: tmp.path().to_path_buf(),
            import: None,
            no_sync: false,
        },
        &mut input,
        &mut output,
    )
    .unwrap();
    let stdout = String::from_utf8(output).unwrap();
    assert!(
        stdout.contains("walked 1"),
        "expected walk summary in output, got: {stdout}"
    );
    // No changes written — frontmatter still has empty classifications.
    let raw = fs::read_to_string(tmp.path().join("published/bare-post.md")).unwrap();
    assert!(!raw.contains("format: thesis"));
}

#[test]
fn interactive_writes_classification_picked_by_number() {
    let tmp = fixture_workdir_with_published();
    // Sequence:
    //   format: 2  (second item in current_v1's format list = "thesis")
    //   hook:   skip
    //   tone:   skip
    //   audience: skip
    //   metrics for linkedin: skip-post (S)
    let input_bytes = b"2\n\n\n\nS\n";
    let mut input = std::io::Cursor::new(&input_bytes[..]);
    let mut output: Vec<u8> = Vec::new();
    commands::backfill::run_interactive(
        &FakeJj::new(),
        &commands::backfill::BackfillArgs {
            workdir: tmp.path().to_path_buf(),
            import: None,
            no_sync: false,
        },
        &mut input,
        &mut output,
    )
    .unwrap();
    let raw = fs::read_to_string(tmp.path().join("published/bare-post.md")).unwrap();
    assert!(
        raw.contains("format: thesis"),
        "expected format=thesis after picking #2, got:\n{raw}"
    );
}

#[test]
fn interactive_quits_immediately_on_q() {
    let tmp = fixture_workdir_with_published();
    // First prompt sees `q` — quit immediately. File on disk is
    // untouched (no commit).
    let pre = fs::read_to_string(tmp.path().join("published/bare-post.md")).unwrap();
    let input_bytes = b"q\n";
    let mut input = std::io::Cursor::new(&input_bytes[..]);
    let mut output: Vec<u8> = Vec::new();
    commands::backfill::run_interactive(
        &FakeJj::new(),
        &commands::backfill::BackfillArgs {
            workdir: tmp.path().to_path_buf(),
            import: None,
            no_sync: false,
        },
        &mut input,
        &mut output,
    )
    .unwrap();
    let post = fs::read_to_string(tmp.path().join("published/bare-post.md")).unwrap();
    assert_eq!(pre, post, "file should be untouched after immediate quit");
    let stdout = String::from_utf8(output).unwrap();
    assert!(stdout.contains("quitting"));
}
