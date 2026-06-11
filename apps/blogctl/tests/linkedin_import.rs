//! Integration tests for `blogctl linkedin import`.
//!
//! Seeds a workdir with published posts whose LinkedIn target URLs carry
//! activity ids in the `/posts/…-<id>-<code>` share form, writes
//! synthetic `Content_*.xlsx` exports (carrying the `urn:li:activity:<id>`
//! form), and drives the import end-to-end against a `FakeJj` so the
//! sync path runs without a real `jj` binary.

use std::path::Path;

use rust_xlsxwriter::Workbook;
use tempfile::TempDir;
use time::macros::{date, datetime};

use blogctl::commands;
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

/// Write one `Content_<date>_<date>_Synthetic.xlsx` export. Each row is
/// `(activity_id, impressions, engagements)` and is written to both the
/// engagements-ranked (A:C) and impressions-ranked (E:G) tables.
fn write_export(dir: &Path, date: &str, rows: &[(&str, u64, u64)]) {
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
    let path = dir.join(format!("Content_{date}_{date}_Synthetic.xlsx"));
    workbook.save(&path).unwrap();
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

fn import_args(workdir: &Path, xlsx: &Path, dry_run: bool) -> commands::linkedin::ImportArgs {
    commands::linkedin::ImportArgs {
        workdir: workdir.to_path_buf(),
        xlsx_dir: Some(xlsx.to_path_buf()),
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
        import_args(tmp.path(), exports.path(), false),
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
        import_args(tmp.path(), exports.path(), false),
    )
    .unwrap();
    assert_eq!(first.added, 1);

    let second = commands::linkedin::run(
        &FakeJj::new(),
        import_args(tmp.path(), exports.path(), false),
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
        import_args(tmp.path(), exports.path(), false),
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
        import_args(tmp.path(), exports.path(), true),
    )
    .unwrap();
    // The preview reports what would be added...
    assert_eq!(summary.added, 1);
    // ...but nothing is persisted.
    assert!(samples_of(&repo, "alpha").is_empty());
}
