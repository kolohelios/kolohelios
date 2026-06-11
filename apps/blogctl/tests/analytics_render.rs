//! Golden-file tests for the analytics text renderers.
//!
//! The hedge-language requirements on `recommendations` (and the
//! visual layout of `summary` / `compare`) deserve a stricter
//! guarantee than the grep-based language gate in the unit tests.
//! These tests render the three commands against a static
//! in-memory fixture and diff the output against a checked-in
//! golden file. A wording change → test failure → review.
//!
//! Regenerate goldens with `UPDATE_GOLDENS=1 cargo test --test
//! analytics_render`. Don't set the env var in CI.

use std::fs;
use std::path::{Path, PathBuf};

use time::OffsetDateTime;

use blogctl::analytics;
use blogctl::classifications::Classifications;
use blogctl::commands::analytics as commands_analytics;
use blogctl::kind::Kind;
use blogctl::post::{Post, PostMetadata};
use blogctl::stage::Stage;
use blogctl::target::{Target, TargetEntry, TargetMetrics, TargetStatus};

/// Fixed `now` for every render — `staleness_days` lands at a
/// stable integer so the recommendations output doesn't drift.
fn fixed_now() -> OffsetDateTime {
    time::macros::datetime!(2026-05-17 00:00:00 UTC)
}

fn post(
    slug: &str,
    title: &str,
    format: &str,
    hook: Option<&str>,
    impressions: u64,
    reactions: u64,
    sampled_at: OffsetDateTime,
) -> Post {
    Post::new(
        PostMetadata {
            title: title.into(),
            slug: slug.into(),
            kind: Kind::Post,
            theme: "standard".into(),
            status: Stage::Published,
            created_at: time::macros::datetime!(2026-05-01 00:00:00 UTC),
            updated_at: time::macros::datetime!(2026-05-08 00:00:00 UTC),
            tags: vec![],
            todoist_task_id: None,
            history_checked: false,
            targets: vec![TargetEntry {
                samples: Vec::new(),
                name: Target::Linkedin,
                status: TargetStatus::Published,
                url: Some("https://www.linkedin.com/posts/example".into()),
                published_at: Some(time::macros::datetime!(2026-05-08 14:32:00 UTC)),
                metrics: Some(TargetMetrics {
                    impressions,
                    reactions,
                    comments: 0,
                    reposts: 0,
                    sampled_at,
                }),
            }],
            classifications: Classifications {
                format: Some(format.into()),
                hook: hook.map(|s| s.into()),
                ..Default::default()
            },
            ai: None,
        },
        "body\n",
    )
}

/// Static fixture exercising the renderer branches:
/// - Three thesis-contradiction posts at decent engagement
///   (drives a DimensionLift + InteractionLift observation, and
///   crosses the LOW_N_THRESHOLD=3 boundary).
/// - Two parable-question posts at low engagement (n=2 → low_n).
/// - One stale-metrics post (sampled 60 days before `now`).
fn fixture_posts() -> Vec<Post> {
    let fresh = time::macros::datetime!(2026-05-14 00:00:00 UTC);
    let stale = time::macros::datetime!(2026-03-10 00:00:00 UTC);
    vec![
        post(
            "t1",
            "Thesis One",
            "thesis",
            Some("contradiction"),
            1000,
            50,
            fresh,
        ),
        post(
            "t2",
            "Thesis Two",
            "thesis",
            Some("contradiction"),
            1200,
            60,
            fresh,
        ),
        post(
            "t3",
            "Thesis Three",
            "thesis",
            Some("contradiction"),
            1500,
            75,
            fresh,
        ),
        post(
            "p1",
            "Parable One",
            "parable",
            Some("question"),
            800,
            16,
            fresh,
        ),
        post(
            "p2",
            "Parable Two",
            "parable",
            Some("question"),
            1200,
            24,
            fresh,
        ),
        post(
            "old",
            "Stale Post",
            "thesis",
            Some("direct-claim"),
            900,
            27,
            stale,
        ),
    ]
}

/// Compare `actual` against the on-disk golden at `relative_path`.
/// With `UPDATE_GOLDENS=1`, overwrite the golden with `actual`
/// (useful when wording changes intentionally — review the diff
/// in the PR).
fn assert_golden(actual: &str, relative_path: &str) {
    let full = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    if std::env::var("UPDATE_GOLDENS").is_ok() {
        // Create parent dirs if missing — useful for first run.
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create golden dir");
        }
        fs::write(&full, actual).expect("write golden");
        return;
    }
    let expected = fs::read_to_string(&full).unwrap_or_else(|_| {
        panic!(
            "golden file missing: {}\nrun `UPDATE_GOLDENS=1 cargo test --test analytics_render` to create it",
            full.display()
        )
    });
    if actual != expected {
        panic!(
            "golden mismatch for {relative_path}\n\n\
             --- expected ---\n{expected}\n\
             --- actual ---\n{actual}\n\
             --- end ---\n\n\
             Run with UPDATE_GOLDENS=1 to regenerate (and review the diff).",
        );
    }
}

fn render_to_string(f: impl FnOnce(&mut dyn std::io::Write) -> std::io::Result<()>) -> String {
    let mut buf: Vec<u8> = Vec::new();
    f(&mut buf).expect("renderer writes never fail to a Vec<u8>");
    String::from_utf8(buf).expect("renderer emits valid UTF-8")
}

#[test]
fn summary_text_matches_golden() {
    let posts = fixture_posts();
    let summary = analytics::summary(&posts, Some(Target::Linkedin), None, fixed_now());
    let actual = render_to_string(|w| commands_analytics::render_summary_text(&summary, w));
    assert_golden(&actual, "tests/fixtures/analytics/golden/summary.txt");
}

#[test]
fn compare_text_matches_golden() {
    let posts = fixture_posts();
    let comparison = analytics::compare(
        &posts,
        "format",
        "hook",
        Some(Target::Linkedin),
        3,
        fixed_now(),
    );
    let actual = render_to_string(|w| commands_analytics::render_compare_text(&comparison, w));
    assert_golden(&actual, "tests/fixtures/analytics/golden/compare.txt");
}

#[test]
fn recommendations_text_matches_golden() {
    let posts = fixture_posts();
    let r = analytics::recommendations(&posts, Some(Target::Linkedin), 3, fixed_now());
    let actual = render_to_string(|w| commands_analytics::render_recommendations_text(&r, w));
    assert_golden(
        &actual,
        "tests/fixtures/analytics/golden/recommendations.txt",
    );
}

/// Sanity check: every observation in the rendered output uses
/// the hedged prefixes ("Early signal:" / "Insufficient data:" /
/// "Stale data:") and never mentions the forbidden words. This is
/// redundant with the unit test in analytics::recommendations but
/// runs at the renderer boundary — catches regressions where the
/// command body bypasses Observation::render.
#[test]
fn recommendations_renderer_keeps_hedges() {
    let posts = fixture_posts();
    let r = analytics::recommendations(&posts, Some(Target::Linkedin), 3, fixed_now());
    let rendered =
        render_to_string(|w| commands_analytics::render_recommendations_text(&r, w)).to_lowercase();
    for word in analytics::FORBIDDEN_WORDS {
        assert!(
            !rendered.contains(word),
            "rendered output contains forbidden word {word:?}:\n{rendered}",
        );
    }
}

/// Future-proofing: a hard-coded path makes the golden test
/// brittle if someone moves the fixtures directory. This test
/// confirms the directory exists in the tree.
#[test]
fn golden_dir_exists() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/analytics/golden");
    assert!(dir.is_dir(), "missing {}", dir.display());
}
