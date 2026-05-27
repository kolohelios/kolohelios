//! Integration tests for `blogctl metrics collect`. Builds a real
//! workdir, walks a stale post via canned stdin, and asserts on the
//! resulting frontmatter — same shape as `tests/backfill.rs`.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use tempfile::TempDir;

use blogctl::commands;
use blogctl::kind::Kind;
use blogctl::sync::FakeJj;
use blogctl::target::Target;

fn add_published_linkedin_target(post_path: &Path, metrics_block: Option<&str>) {
    let raw = fs::read_to_string(post_path).unwrap();
    let metrics = metrics_block.unwrap_or("");
    let injected = format!(
        concat!(
            "tags: []\n",
            "targets:\n",
            "  - name: linkedin\n",
            "    status: published\n",
            "    url: https://www.linkedin.com/posts/example\n",
            "    published_at: 2026-05-08T14:32:00Z\n",
            "{}",
        ),
        metrics,
    );
    fs::write(post_path, raw.replace("tags: []\n", &injected)).unwrap();
}

fn fresh_metrics_block_now() -> String {
    // Sampled "now" → never stale under any reasonable `--stale-after`.
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    format!(
        concat!(
            "    metrics:\n",
            "      impressions: 100\n",
            "      reactions: 5\n",
            "      comments: 1\n",
            "      reposts: 0\n",
            "      sampled_at: {}\n",
        ),
        now,
    )
}

fn old_metrics_block() -> &'static str {
    // Sampled in 2024 → always stale under any 7d cutoff.
    concat!(
        "    metrics:\n",
        "      impressions: 100\n",
        "      reactions: 5\n",
        "      comments: 1\n",
        "      reposts: 0\n",
        "      sampled_at: 2024-01-01T00:00:00Z\n",
    )
}

/// Build a workdir with one Published post that has a `linkedin`
/// target. Caller supplies the metrics block (or `None` for "never
/// sampled").
fn workdir_with_published_linkedin(slug: &str, metrics_block: Option<&str>) -> TempDir {
    let tmp = TempDir::new().unwrap();
    commands::init::run(&FakeJj::new(), tmp.path().to_path_buf(), false).unwrap();
    commands::new::run(
        &FakeJj::new(),
        slug.to_string(),
        tmp.path().to_path_buf(),
        Some(slug.to_string()),
        Kind::Post,
        None,
        false,
    )
    .unwrap();
    add_published_linkedin_target(
        &tmp.path().join(format!("concepts/{slug}.md")),
        metrics_block,
    );
    // concept → ideation → editing → final-editing → published
    for _ in 0..4 {
        commands::promote::run(
            &FakeJj::new(),
            slug.to_string(),
            tmp.path().to_path_buf(),
            false,
        )
        .unwrap();
    }
    tmp
}

fn collect_args(workdir: &Path) -> commands::metrics::CollectArgs {
    commands::metrics::CollectArgs {
        workdir: workdir.to_path_buf(),
        target: Target::Linkedin,
        stale_after: "7d".to_string(),
        batch_size: 10,
        no_sync: false,
    }
}

#[test]
fn collect_walks_stale_post_and_records_entered_metrics() {
    let tmp = workdir_with_published_linkedin("bare-post", Some(old_metrics_block()));
    // Prompt sequence for one post: impressions, reactions, comments, reposts.
    let input_bytes = b"1842\n67\n14\n5\n";
    let mut input = Cursor::new(&input_bytes[..]);
    let mut output: Vec<u8> = Vec::new();

    commands::metrics::collect(
        &FakeJj::new(),
        collect_args(tmp.path()),
        &mut input,
        &mut output,
    )
    .unwrap();

    let raw = fs::read_to_string(tmp.path().join("published/bare-post.md")).unwrap();
    assert!(raw.contains("impressions: 1842"), "got:\n{raw}");
    assert!(raw.contains("reactions: 67"), "got:\n{raw}");
    assert!(raw.contains("reposts: 5"), "got:\n{raw}");
    // The 2024 timestamp is overwritten by `now`.
    assert!(!raw.contains("2024-01-01T00:00:00Z"), "got:\n{raw}");
}

#[test]
fn collect_nothing_to_refresh_when_all_fresh() {
    let tmp = workdir_with_published_linkedin("bare-post", Some(&fresh_metrics_block_now()));
    let mut input = Cursor::new(&b""[..]);
    let mut output: Vec<u8> = Vec::new();

    let jj = FakeJj::new();
    commands::metrics::collect(&jj, collect_args(tmp.path()), &mut input, &mut output).unwrap();

    let stdout = String::from_utf8(output).unwrap();
    assert!(
        stdout.contains("nothing to refresh"),
        "expected nothing-to-refresh, got: {stdout}",
    );
    // No write, no commit — the FakeJj shouldn't have received any
    // calls.
    assert!(
        jj.calls().is_empty(),
        "fresh-only workdir must not invoke jj: {:?}",
        jj.calls(),
    );
}

#[test]
fn collect_picks_never_sampled_before_stale_sampled() {
    // Two eligible posts. With batch_size=1, the never-sampled one
    // (sorts first under "None < Some") must be the one we walk.
    let tmp = workdir_with_published_linkedin("never-sampled", None);
    // Add a second post sampled long ago.
    commands::new::run(
        &FakeJj::new(),
        "stale-sampled".to_string(),
        tmp.path().to_path_buf(),
        Some("stale-sampled".to_string()),
        Kind::Post,
        None,
        false,
    )
    .unwrap();
    add_published_linkedin_target(
        &tmp.path().join("concepts/stale-sampled.md"),
        Some(old_metrics_block()),
    );
    for _ in 0..4 {
        commands::promote::run(
            &FakeJj::new(),
            "stale-sampled".to_string(),
            tmp.path().to_path_buf(),
            false,
        )
        .unwrap();
    }

    // Single-post batch, enter values for whichever the walker picks.
    let input_bytes = b"1\n2\n3\n4\n";
    let mut input = Cursor::new(&input_bytes[..]);
    let mut output: Vec<u8> = Vec::new();
    commands::metrics::collect(
        &FakeJj::new(),
        commands::metrics::CollectArgs {
            batch_size: 1,
            ..collect_args(tmp.path())
        },
        &mut input,
        &mut output,
    )
    .unwrap();

    let never_raw = fs::read_to_string(tmp.path().join("published/never-sampled.md")).unwrap();
    let stale_raw = fs::read_to_string(tmp.path().join("published/stale-sampled.md")).unwrap();
    // The never-sampled post got the entered values.
    assert!(
        never_raw.contains("impressions: 1"),
        "never-sampled should be picked first; got:\n{never_raw}"
    );
    // The stale-sampled post is untouched (still has its 2024 mtime).
    assert!(
        stale_raw.contains("2024-01-01T00:00:00Z"),
        "stale-sampled should be deferred; got:\n{stale_raw}"
    );
}

#[test]
fn collect_quit_mid_walk_commits_accumulated_writes() {
    let tmp = workdir_with_published_linkedin("first", None);
    commands::new::run(
        &FakeJj::new(),
        "second".to_string(),
        tmp.path().to_path_buf(),
        Some("second".to_string()),
        Kind::Post,
        None,
        false,
    )
    .unwrap();
    add_published_linkedin_target(&tmp.path().join("concepts/second.md"), None);
    for _ in 0..4 {
        commands::promote::run(
            &FakeJj::new(),
            "second".to_string(),
            tmp.path().to_path_buf(),
            false,
        )
        .unwrap();
    }

    // First post: real values. Second post's first prompt: `q`.
    // Accumulated change for `first` must still land.
    let input_bytes = b"100\n10\n2\n1\nq\n";
    let mut input = Cursor::new(&input_bytes[..]);
    let mut output: Vec<u8> = Vec::new();
    commands::metrics::collect(
        &FakeJj::new(),
        collect_args(tmp.path()),
        &mut input,
        &mut output,
    )
    .unwrap();

    let stdout = String::from_utf8(output).unwrap();
    assert!(
        stdout.contains("quit early"),
        "expected quit-early notice, got: {stdout}"
    );

    // Either ordering is permissible (both are "never sampled");
    // whichever was walked first got committed.
    let first_raw = fs::read_to_string(tmp.path().join("published/first.md")).unwrap();
    let second_raw = fs::read_to_string(tmp.path().join("published/second.md")).unwrap();
    let one_committed =
        first_raw.contains("impressions: 100") ^ second_raw.contains("impressions: 100");
    assert!(
        one_committed,
        "exactly one of the two posts should have committed metrics; \
         first:\n{first_raw}\nsecond:\n{second_raw}"
    );
}
