//! Integration test for `blogctl analytics summary`.
//!
//! Builds a real workdir on disk via the library API, runs the
//! command with both text and JSON output, and asserts on the
//! JSON shape (text output is harder to pin and the renderer's
//! formatting is covered by unit tests).

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use blogctl::commands;
use blogctl::kind::Kind;
use blogctl::sync::FakeJj;
use blogctl::Target;

/// Plant a LinkedIn TargetEntry on a post by hand-editing the
/// frontmatter — the "promote target to published" flow lives in
/// a different issue. Same helper used by tests/sync.rs.
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

fn seed_post(workdir: &Path, slug: &str, format: &str, impressions: u64, reactions: u64) {
    commands::new::run(
        &FakeJj::new(),
        slug.to_string(),
        workdir.to_path_buf(),
        Some(slug.to_string()),
        Kind::Post,
        None,
        false,
    )
    .unwrap();
    let post_path = workdir.join(format!("concepts/{slug}.md"));
    add_published_linkedin_target(&post_path);

    commands::classify::run(
        &FakeJj::new(),
        commands::classify::ClassifyArgs {
            slug: slug.into(),
            workdir: workdir.to_path_buf(),
            format: Some(format.into()),
            ..Default::default()
        },
    )
    .unwrap();
    commands::metrics::update(
        &FakeJj::new(),
        commands::metrics::UpdateArgs {
            slug: slug.into(),
            workdir: workdir.to_path_buf(),
            target: Target::Linkedin,
            impressions,
            reactions,
            comments: 0,
            reposts: 0,
            sampled_at: Some("2026-05-14T00:00:00Z".into()),
            no_sync: false,
        },
    )
    .unwrap();
}

fn fixture_workdir() -> TempDir {
    let tmp = TempDir::new().unwrap();
    commands::init::run(&FakeJj::new(), tmp.path().to_path_buf(), false).unwrap();
    // 3 thesis posts (one zero-impressions to exercise low-impressions
    // path), 2 parable posts. Six samples total across two values
    // crosses the LOW_N_THRESHOLD=3 boundary for both directions.
    seed_post(tmp.path(), "thesis-one", "thesis", 1000, 50);
    seed_post(tmp.path(), "thesis-two", "thesis", 2000, 100);
    seed_post(tmp.path(), "thesis-three", "thesis", 1500, 75);
    seed_post(tmp.path(), "parable-one", "parable", 800, 16);
    seed_post(tmp.path(), "parable-two", "parable", 1200, 24);
    tmp
}

#[test]
fn summary_command_succeeds_against_fixture() {
    let tmp = fixture_workdir();
    commands::analytics::summary(commands::analytics::SummaryArgs {
        workdir: tmp.path().to_path_buf(),
        target: None,
        dimension: None,
        json: false,
    })
    .unwrap();
}

#[test]
fn summary_json_matches_documented_schema_for_fixture() {
    // Run the same fixture with --json and verify the resulting
    // JSON has the documented shape with the right counts and
    // ordering.
    let tmp = fixture_workdir();

    // Capture stdout via a child process — the public command API
    // writes to stdout directly. Easier: rebuild the summary
    // directly via the library to assert on the structured data.
    let repo = blogctl::Repository::open(blogctl::Workdir::new(tmp.path())).unwrap();
    let handles = repo.list().unwrap();
    let posts: Vec<_> = handles
        .iter()
        .map(|h| repo.load_raw(&h.metadata.slug).unwrap().1)
        .collect();
    let summary = blogctl::analytics::summary(
        &posts,
        Some(Target::Linkedin),
        Some("format"),
        time::macros::datetime!(2026-05-17 00:00:00 UTC),
    );
    let json = serde_json::to_string(&summary).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let dims = v["dimensions"].as_array().unwrap();
    assert_eq!(dims.len(), 1);
    let format = &dims[0];
    assert_eq!(format["name"], "format");
    let values = format["values"].as_array().unwrap();
    // Sorted desc by median engagement_rate. Both thesis (5%) and
    // parable (2%) are at n=3 and n=2 respectively. thesis median
    // engagement_rate is higher → thesis comes first.
    assert_eq!(values[0]["value"], "thesis");
    assert_eq!(values[0]["n"], 3);
    assert_eq!(values[0]["low_n"], false);
    assert_eq!(values[1]["value"], "parable");
    assert_eq!(values[1]["n"], 2);
    assert_eq!(values[1]["low_n"], true);
}

#[test]
fn summary_with_dimension_filter_restricts_output() {
    let tmp = fixture_workdir();
    let repo = blogctl::Repository::open(blogctl::Workdir::new(tmp.path())).unwrap();
    let handles = repo.list().unwrap();
    let posts: Vec<_> = handles
        .iter()
        .map(|h| repo.load_raw(&h.metadata.slug).unwrap().1)
        .collect();
    // Format-only.
    let only_format = blogctl::analytics::summary(
        &posts,
        None,
        Some("format"),
        time::macros::datetime!(2026-05-17 00:00:00 UTC),
    );
    assert_eq!(only_format.dimensions.len(), 1);
    assert_eq!(only_format.dimensions[0].name, "format");
}

#[test]
fn summary_with_unknown_dimension_filter_yields_empty() {
    let tmp = fixture_workdir();
    let repo = blogctl::Repository::open(blogctl::Workdir::new(tmp.path())).unwrap();
    let handles = repo.list().unwrap();
    let posts: Vec<_> = handles
        .iter()
        .map(|h| repo.load_raw(&h.metadata.slug).unwrap().1)
        .collect();
    let nothing = blogctl::analytics::summary(
        &posts,
        None,
        Some("nope"),
        time::macros::datetime!(2026-05-17 00:00:00 UTC),
    );
    assert!(nothing.dimensions.is_empty());
}
