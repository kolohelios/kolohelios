//! Integration tests for `blogctl linkedin import`.
//!
//! Seeds a workdir with published posts whose LinkedIn target URLs carry
//! activity ids in the `/posts/…-<id>-<code>` share form, writes
//! synthetic exports (both the old `Content_` and new
//! `AggregateAnalytics_` filename formats, carrying the
//! `urn:li:activity:<id>` form), and drives the import end-to-end against
//! a `FakeJj` so the sync path runs without a real `jj` binary.

use std::path::Path;

use rust_xlsxwriter::Workbook;
use tempfile::TempDir;
use time::macros::{date, datetime};

use blogctl::commands;
use blogctl::fetch::FakeFetcher;
use blogctl::kind::Kind;
use blogctl::post::{Post, PostMetadata};
use blogctl::stage::Stage;
use blogctl::storage::{Repository, Workdir};
use blogctl::sync::FakeJj;
use blogctl::target::{MetricSample, Target, TargetEntry, TargetStatus};

fn workdir() -> (TempDir, Repository) {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::unchecked(Workdir::new(tmp.path()));
    repo.init().unwrap();
    (tmp, repo)
}

/// Seed a published post whose LinkedIn target stores the share-form URL
/// (the activity id sits between the slug and a short code).
fn seed_post(repo: &Repository, slug: &str, activity_id: &str) {
    let url = format!("https://www.linkedin.com/posts/kolohelios_{slug}-share-{activity_id}-AbCd/");
    let post = Post::new(
        PostMetadata {
            title: slug.into(),
            slug: slug.into(),
            kind: Kind::Post,
            theme: "standard".into(),
            status: Stage::Published,
            created_at: datetime!(2026-05-01 00:00:00 UTC),
            updated_at: datetime!(2026-05-01 00:00:00 UTC),
            tags: vec![],
            todoist_task_id: None,
            history_checked: false,
            targets: vec![TargetEntry {
                name: Target::Linkedin,
                status: TargetStatus::Published,
                url: Some(url),
                published_at: Some(datetime!(2026-05-01 00:00:00 UTC)),
                metrics: None,
                samples: Vec::new(),
            }],
            classifications: Default::default(),
            ai: None,
        },
        "body\n",
    );
    repo.create_post(&post).unwrap();
}

/// Write one old-format `Content_<date>_<date>_Synthetic.xlsx` export
/// (dates first, name last).
fn write_export(dir: &Path, date: &str, rows: &[(&str, u64, u64)]) {
    write_workbook(
        &dir.join(format!("Content_{date}_{date}_Synthetic.xlsx")),
        rows,
    );
}

/// Write one new-format `AggregateAnalytics_<name>_<date>_<date>.xlsx`
/// export (name first, dates last). The sheet contents are identical to
/// the old format — only the filename differs.
fn write_aggregate_export(dir: &Path, date: &str, rows: &[(&str, u64, u64)]) {
    write_workbook(
        &dir.join(format!("AggregateAnalytics_Synthetic_{date}_{date}.xlsx")),
        rows,
    );
}

/// Write the `TOP POSTS` sheet to `path`. Each row is
/// `(activity_id, impressions, engagements)` and is written to both the
/// engagements-ranked (A:C) and impressions-ranked (E:G) tables.
fn write_workbook(path: &Path, rows: &[(&str, u64, u64)]) {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("TOP POSTS").unwrap();
    sheet.write_string(2, 0, "Post URL").unwrap();
    sheet.write_string(2, 1, "Post publish date").unwrap();
    sheet.write_string(2, 2, "Engagements").unwrap();
    sheet.write_string(2, 4, "Post URL").unwrap();
    sheet.write_string(2, 5, "Post publish date").unwrap();
    sheet.write_string(2, 6, "Impressions").unwrap();
    for (i, (id, impressions, engagements)) in rows.iter().enumerate() {
        let r = 3 + i as u32;
        let url = format!("https://www.linkedin.com/feed/update/urn:li:activity:{id}");
        sheet.write_string(r, 0, url.as_str()).unwrap();
        sheet.write_string(r, 1, "5/1/2026").unwrap();
        sheet.write_number(r, 2, *engagements as f64).unwrap();
        sheet.write_string(r, 4, url.as_str()).unwrap();
        sheet.write_string(r, 5, "5/1/2026").unwrap();
        sheet.write_number(r, 6, *impressions as f64).unwrap();
    }
    workbook.save(path).unwrap();
}

fn samples_of(repo: &Repository, slug: &str) -> Vec<MetricSample> {
    let (_handle, post) = repo.load_raw(slug).unwrap();
    post.metadata
        .targets
        .iter()
        .find(|t| t.name == Target::Linkedin)
        .expect("linkedin target")
        .samples
        .clone()
}

fn import_args(
    workdir: &Path,
    xlsx: &Path,
    no_fetch: bool,
    dry_run: bool,
) -> commands::linkedin::ImportArgs {
    commands::linkedin::ImportArgs {
        workdir: workdir.to_path_buf(),
        xlsx_dir: Some(xlsx.to_path_buf()),
        no_fetch,
        dry_run,
        no_sync: false,
    }
}

#[test]
fn records_per_day_samples_for_matched_posts() {
    let (tmp, repo) = workdir();
    seed_post(&repo, "alpha", "7400000000000000001");
    seed_post(&repo, "beta", "7400000000000000002");
    let exports = TempDir::new().unwrap();
    write_export(
        exports.path(),
        "2026-05-22",
        &[
            ("7400000000000000001", 100, 10),
            ("7400000000000000002", 200, 20),
        ],
    );
    write_export(
        exports.path(),
        "2026-05-23",
        &[("7400000000000000001", 150, 12)],
    );

    let summary = commands::linkedin::run(
        &FakeJj::new(),
        &FakeFetcher::new(),
        import_args(tmp.path(), exports.path(), true, false),
    )
    .unwrap();
    assert_eq!(summary.added, 3);
    assert_eq!(summary.skipped, 0);
    assert!(summary.unmatched.is_empty());

    let alpha = samples_of(&repo, "alpha");
    assert_eq!(alpha.len(), 2);
    assert_eq!(alpha[0].date, date!(2026 - 05 - 22));
    assert_eq!(alpha[0].impressions, Some(100));
    assert_eq!(alpha[0].engagements, Some(10));
    assert_eq!(alpha[1].date, date!(2026 - 05 - 23));
    assert_eq!(alpha[1].impressions, Some(150));

    assert_eq!(samples_of(&repo, "beta").len(), 1);
}

#[test]
fn imports_mixed_prefix_exports_without_duplicates() {
    // A `linkedin-exports/` dir holding both the old `Content_` format
    // and the new `AggregateAnalytics_` format must import every file —
    // the default `[linkedin] export_filename_prefixes` covers both —
    // and re-running adds no duplicate data points.
    let (tmp, repo) = workdir();
    seed_post(&repo, "alpha", "7400000000000000001");
    let exports = TempDir::new().unwrap();
    write_export(
        exports.path(),
        "2026-05-22",
        &[("7400000000000000001", 100, 10)],
    );
    write_aggregate_export(
        exports.path(),
        "2026-05-23",
        &[("7400000000000000001", 150, 12)],
    );

    let summary = commands::linkedin::run(
        &FakeJj::new(),
        &FakeFetcher::new(),
        import_args(tmp.path(), exports.path(), true, false),
    )
    .unwrap();
    assert_eq!(summary.added, 2);
    assert!(summary.unmatched.is_empty());

    let alpha = samples_of(&repo, "alpha");
    assert_eq!(alpha.len(), 2);
    assert_eq!(alpha[0].date, date!(2026 - 05 - 22));
    assert_eq!(alpha[1].date, date!(2026 - 05 - 23));

    // Re-running the same mixed dir is idempotent on `(urn, date)`.
    let second = commands::linkedin::run(
        &FakeJj::new(),
        &FakeFetcher::new(),
        import_args(tmp.path(), exports.path(), true, false),
    )
    .unwrap();
    assert_eq!(second.added, 0);
    assert_eq!(second.skipped, 2);
    assert_eq!(samples_of(&repo, "alpha").len(), 2);
}

#[test]
fn re_running_overlapping_exports_is_idempotent() {
    let (tmp, repo) = workdir();
    seed_post(&repo, "alpha", "7400000000000000001");
    let exports = TempDir::new().unwrap();
    write_export(
        exports.path(),
        "2026-05-22",
        &[("7400000000000000001", 100, 10)],
    );

    let first = commands::linkedin::run(
        &FakeJj::new(),
        &FakeFetcher::new(),
        import_args(tmp.path(), exports.path(), true, false),
    )
    .unwrap();
    assert_eq!(first.added, 1);

    let second = commands::linkedin::run(
        &FakeJj::new(),
        &FakeFetcher::new(),
        import_args(tmp.path(), exports.path(), true, false),
    )
    .unwrap();
    assert_eq!(second.added, 0);
    assert_eq!(second.skipped, 1);
    // No duplicate data point for the same (urn, date).
    assert_eq!(samples_of(&repo, "alpha").len(), 1);
}

#[test]
fn unmatched_urns_are_reported_not_created() {
    let (tmp, repo) = workdir();
    seed_post(&repo, "alpha", "7400000000000000001");
    let exports = TempDir::new().unwrap();
    write_export(
        exports.path(),
        "2026-05-22",
        &[
            ("7400000000000000001", 100, 10),
            ("7400000000000000999", 50, 5),
        ],
    );

    let summary = commands::linkedin::run(
        &FakeJj::new(),
        &FakeFetcher::new(),
        import_args(tmp.path(), exports.path(), true, false),
    )
    .unwrap();
    assert_eq!(
        summary.unmatched,
        vec!["urn:li:activity:7400000000000000999".to_string()],
    );
    // The unmatched URN did not create a post.
    assert_eq!(repo.list().unwrap().len(), 1);
}

#[test]
fn dry_run_previews_without_writing() {
    let (tmp, repo) = workdir();
    seed_post(&repo, "alpha", "7400000000000000001");
    let exports = TempDir::new().unwrap();
    write_export(
        exports.path(),
        "2026-05-22",
        &[("7400000000000000001", 100, 10)],
    );

    let summary = commands::linkedin::run(
        &FakeJj::new(),
        &FakeFetcher::new(),
        import_args(tmp.path(), exports.path(), true, true),
    )
    .unwrap();
    // The preview reports what would be added...
    assert_eq!(summary.added, 1);
    // ...but nothing is persisted.
    assert!(samples_of(&repo, "alpha").is_empty());
}

fn linkedin_url(id: &str) -> String {
    format!("https://www.linkedin.com/feed/update/urn:li:activity:{id}")
}

/// Build minimal post HTML with a `SocialMediaPosting` JSON-LD block.
/// Keep `headline`/`body` free of `"` and newlines so the inlined JSON
/// stays valid.
fn html_post(
    headline: &str,
    body: &str,
    date: &str,
    likes: u64,
    comments: u64,
    shares: u64,
) -> String {
    let json = format!(
        r#"{{"@context":"https://schema.org","@type":"SocialMediaPosting","headline":"{headline}","articleBody":"{body}","datePublished":"{date}","interactionStatistic":[{{"interactionType":"http://schema.org/LikeAction","userInteractionCount":{likes}}},{{"interactionType":"https://schema.org/CommentAction","userInteractionCount":{comments}}},{{"interactionType":"https://schema.org/ShareAction","userInteractionCount":{shares}}}]}}"#
    );
    format!(
        "<html><head><script type=\"application/ld+json\">{json}</script></head><body>x</body></html>"
    )
}

#[test]
fn creates_published_stub_for_unmatched_urn() {
    let (tmp, repo) = workdir();
    // No matching post seeded — the URN is unmatched.
    let id = "7400000000000000050";
    let url = linkedin_url(id);
    let fetcher = FakeFetcher::new().with(
        url.clone(),
        html_post(
            "The Test Post",
            "Body line one. Body line two.",
            "2026-05-10T12:00:00Z",
            5,
            2,
            1,
        ),
    );
    let exports = TempDir::new().unwrap();
    write_export(exports.path(), "2026-05-10", &[(id, 100, 7)]);

    let summary = commands::linkedin::run(
        &FakeJj::new(),
        &fetcher,
        import_args(tmp.path(), exports.path(), false, false),
    )
    .unwrap();
    assert_eq!(summary.created, vec!["the-test-post".to_string()]);
    assert!(summary.unmatched.is_empty());

    let (handle, post) = repo.load_raw("the-test-post").unwrap();
    assert_eq!(handle.stage, Stage::Published);
    assert_eq!(post.metadata.title, "The Test Post");
    assert!(post.body.contains("Body line one"));
    assert_eq!(post.metadata.created_at, datetime!(2026-05-10 12:00:00 UTC));

    let target = &post.metadata.targets[0];
    assert_eq!(target.name, Target::Linkedin);
    assert_eq!(target.url.as_deref(), Some(url.as_str()));
    assert_eq!(
        target.published_at,
        Some(datetime!(2026-05-10 12:00:00 UTC))
    );
    let metrics = target.metrics.as_ref().unwrap();
    assert_eq!(metrics.impressions, 100); // from the export
    assert_eq!(metrics.reactions, 5); // from the HTML LikeAction
    assert_eq!(metrics.comments, 2);
    assert_eq!(metrics.reposts, 1);
    assert_eq!(target.samples.len(), 1);
    assert_eq!(target.samples[0].impressions, Some(100));
}

#[test]
fn creating_a_stub_is_idempotent_by_urn() {
    let (tmp, repo) = workdir();
    let id = "7400000000000000050";
    let fetcher = FakeFetcher::new().with(
        linkedin_url(id),
        html_post("The Test Post", "Body.", "2026-05-10T12:00:00Z", 1, 0, 0),
    );
    let exports = TempDir::new().unwrap();
    write_export(exports.path(), "2026-05-10", &[(id, 100, 7)]);

    let first = commands::linkedin::run(
        &FakeJj::new(),
        &fetcher,
        import_args(tmp.path(), exports.path(), false, false),
    )
    .unwrap();
    assert_eq!(first.created.len(), 1);
    assert_eq!(repo.list().unwrap().len(), 1);

    // Re-run: the URN now matches the created post → no second post, and
    // its same-day sample is an idempotent skip.
    let second = commands::linkedin::run(
        &FakeJj::new(),
        &fetcher,
        import_args(tmp.path(), exports.path(), false, false),
    )
    .unwrap();
    assert!(second.created.is_empty());
    assert_eq!(second.added, 0);
    assert_eq!(repo.list().unwrap().len(), 1);
}

#[test]
fn no_fetch_skips_stub_creation() {
    let (tmp, repo) = workdir();
    let id = "7400000000000000050";
    let fetcher = FakeFetcher::new().with(
        linkedin_url(id),
        html_post("X", "B", "2026-05-10T12:00:00Z", 0, 0, 0),
    );
    let exports = TempDir::new().unwrap();
    write_export(exports.path(), "2026-05-10", &[(id, 100, 7)]);

    let summary = commands::linkedin::run(
        &FakeJj::new(),
        &fetcher,
        import_args(tmp.path(), exports.path(), true, false),
    )
    .unwrap();
    assert!(summary.created.is_empty());
    assert_eq!(summary.unmatched, vec![format!("urn:li:activity:{id}")]);
    assert_eq!(repo.list().unwrap().len(), 0);
}

#[test]
fn dry_run_does_not_create_stub() {
    let (tmp, repo) = workdir();
    let id = "7400000000000000050";
    let fetcher = FakeFetcher::new().with(
        linkedin_url(id),
        html_post("The Test Post", "B", "2026-05-10T12:00:00Z", 0, 0, 0),
    );
    let exports = TempDir::new().unwrap();
    write_export(exports.path(), "2026-05-10", &[(id, 100, 7)]);

    let summary = commands::linkedin::run(
        &FakeJj::new(),
        &fetcher,
        import_args(tmp.path(), exports.path(), false, true),
    )
    .unwrap();
    // The preview reports the post would be created...
    assert_eq!(summary.created, vec!["the-test-post".to_string()]);
    // ...but nothing is written.
    assert_eq!(repo.list().unwrap().len(), 0);
}

#[test]
fn slug_collision_skips_the_second_post() {
    let (tmp, repo) = workdir();
    let id_a = "7400000000000000050";
    let id_b = "7400000000000000051";
    // Both posts' headlines slugify to the same slug.
    let fetcher = FakeFetcher::new()
        .with(
            linkedin_url(id_a),
            html_post("Same Title", "Body A", "2026-05-10T12:00:00Z", 0, 0, 0),
        )
        .with(
            linkedin_url(id_b),
            html_post("Same Title", "Body B", "2026-05-11T12:00:00Z", 0, 0, 0),
        );
    let exports = TempDir::new().unwrap();
    write_export(exports.path(), "2026-05-10", &[(id_a, 1, 0)]);
    write_export(exports.path(), "2026-05-11", &[(id_b, 1, 0)]);

    let summary = commands::linkedin::run(
        &FakeJj::new(),
        &fetcher,
        import_args(tmp.path(), exports.path(), false, false),
    )
    .unwrap();
    assert_eq!(summary.created, vec!["same-title".to_string()]);
    assert_eq!(summary.unmatched.len(), 1);
    assert_eq!(repo.list().unwrap().len(), 1);
}
