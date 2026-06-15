//! Integration tests for the LinkedIn `xlsx` export parser.
//!
//! LinkedIn's exports are binary (zipped XML), and the real ones carry
//! the author's private per-post impression/engagement counts. Rather
//! than commit opaque blobs of personal analytics to this public repo,
//! each test builds a synthetic corpus on disk with `rust_xlsxwriter`,
//! faithfully replicating the `TOP POSTS` layout — two side-by-side
//! tables, the same headers, numeric metrics, unpadded `M/D/YYYY`
//! dates — then parses it back. The generator code *is* the fixture,
//! fully visible in the diff.
//!
//! The corpus is engineered so its 5 daily files contain exactly 58
//! distinct posts (matching the real export set the issue is written
//! against) with deliberate cross-day overlap, plus an impressions-only
//! day and an engagements-only day.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rust_xlsxwriter::{Workbook, Worksheet};
use tempfile::TempDir;
use time::macros::date;
use time::Date;

use blogctl::linkedin::{self, PostSnapshot};
use blogctl::Error;

/// Synthetic activity ids are offset from this base; clearly distinct
/// from real LinkedIn URNs while staying within `u64`.
const BASE: u64 = 7_400_000_000_000_000_000;

/// The accepted export filename prefixes for these tests — the synthetic
/// corpus uses the `Content_` form.
fn prefixes() -> Vec<String> {
    vec!["Content_".to_string()]
}

struct Row {
    id: u64,
    publish_date: String,
    value: u64,
}

fn url(id: u64) -> String {
    format!("https://www.linkedin.com/feed/update/urn:li:activity:{id}")
}

/// Build the post rows for activity ids `BASE + n` over `ns`. The
/// publish date uses a single-digit month and a day that spans one and
/// two digits, exercising the unpadded `M/D/YYYY` parse.
fn rows(ns: std::ops::RangeInclusive<u64>) -> Vec<Row> {
    ns.map(|n| Row {
        id: BASE + n,
        publish_date: format!("3/{}/2026", ((n - 1) % 28) + 1),
        value: 10 + n,
    })
    .collect()
}

fn reversed(mut v: Vec<Row>) -> Vec<Row> {
    v.reverse();
    v
}

/// Write one `Content_<date>_<date>_Synthetic.xlsx` export with the
/// given engagements-ranked and impressions-ranked tables. Either side
/// may be empty (some days populate only one table).
fn write_export(dir: &Path, date: &str, engagements: &[Row], impressions: &[Row]) {
    let mut workbook = Workbook::new();
    // Real exports have five sheets; a couple of decoys before
    // TOP POSTS prove the parser finds it by name, not position.
    workbook.add_worksheet().set_name("DISCOVERY").unwrap();
    workbook.add_worksheet().set_name("ENGAGEMENT").unwrap();
    let sheet = workbook.add_worksheet();
    sheet.set_name("TOP POSTS").unwrap();

    sheet
        .write_string(
            0,
            0,
            "Maximum of 50 posts available to include in this list",
        )
        .unwrap();
    // Header row (Excel row 3 / 0-based row 2): left table in A:C,
    // right table in E:G, separated by an empty column D.
    sheet.write_string(2, 0, "Post URL").unwrap();
    sheet.write_string(2, 1, "Post publish date").unwrap();
    sheet.write_string(2, 2, "Engagements").unwrap();
    sheet.write_string(2, 4, "Post URL").unwrap();
    sheet.write_string(2, 5, "Post publish date").unwrap();
    sheet.write_string(2, 6, "Impressions").unwrap();

    write_table(sheet, 0, engagements);
    write_table(sheet, 4, impressions);

    let path = dir.join(format!("Content_{date}_{date}_Synthetic.xlsx"));
    workbook.save(&path).unwrap();
}

fn write_table(sheet: &mut Worksheet, base_col: u16, rows: &[Row]) {
    for (i, row) in rows.iter().enumerate() {
        let r = 3 + i as u32;
        sheet.write_string(r, base_col, url(row.id)).unwrap();
        sheet
            .write_string(r, base_col + 1, row.publish_date.as_str())
            .unwrap();
        sheet
            .write_number(r, base_col + 2, row.value as f64)
            .unwrap();
    }
}

/// A single-row export with a caller-chosen URL, date, and metric, used
/// to drive the per-cell error paths.
fn single_post_export(dir: &Path, url_cell: &str, date_cell: &str, value: f64) -> PathBuf {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("TOP POSTS").unwrap();
    sheet.write_string(2, 0, "Post URL").unwrap();
    sheet.write_string(2, 1, "Post publish date").unwrap();
    sheet.write_string(2, 2, "Engagements").unwrap();
    sheet.write_string(3, 0, url_cell).unwrap();
    sheet.write_string(3, 1, date_cell).unwrap();
    sheet.write_number(3, 2, value).unwrap();
    let path = dir.join("Content_2026-02-01_2026-02-01_Synthetic.xlsx");
    workbook.save(&path).unwrap();
    path
}

/// Build the 5-file synthetic corpus (58 distinct posts) and parse it.
/// The temp dir is dropped on return; the parsed snapshots are owned.
fn corpus() -> Vec<PostSnapshot> {
    let dir = TempDir::new().unwrap();
    let d = dir.path();
    // Both tables, same posts ranked differently (impressions reversed).
    write_export(d, "2026-02-01", &rows(1..=10), &reversed(rows(1..=10)));
    // Overlaps 5..=10 with the previous day → cross-file dedup.
    write_export(d, "2026-02-02", &rows(5..=15), &rows(5..=15));
    // Impressions only (engagements side empty).
    write_export(d, "2026-02-03", &[], &rows(16..=30));
    // Engagements only (impressions side empty).
    write_export(d, "2026-02-04", &rows(31..=45), &[]);
    // Unequal-length sides: engagements 46..=58, impressions 50..=58.
    write_export(d, "2026-02-05", &rows(46..=58), &rows(50..=58));
    linkedin::parse_dir(d, &prefixes()).unwrap()
}

fn find(snaps: &[PostSnapshot], id: u64, snapshot: Date) -> &PostSnapshot {
    let urn = format!("urn:li:activity:{id}");
    snaps
        .iter()
        .find(|s| s.urn == urn && s.snapshot_date == snapshot)
        .unwrap_or_else(|| panic!("no snapshot for {urn} @ {snapshot}"))
}

#[test]
fn parse_dir_over_synthetic_exports_yields_58_distinct_urns() {
    let snaps = corpus();
    let distinct: BTreeSet<&str> = snaps.iter().map(|s| s.urn.as_str()).collect();
    assert_eq!(distinct.len(), 58, "distinct URNs across the corpus");
}

#[test]
fn merges_both_metrics_by_urn_within_a_file() {
    let snaps = corpus();
    let s = find(&snaps, BASE + 1, date!(2026 - 02 - 01));
    assert_eq!(s.url, url(BASE + 1));
    assert_eq!(s.publish_date, date!(2026 - 03 - 01));
    assert_eq!(s.engagements, Some(11));
    assert_eq!(s.impressions, Some(11));
}

#[test]
fn tolerates_empty_engagements_side() {
    // 2026-02-03 populates impressions only.
    let snaps = corpus();
    let s = find(&snaps, BASE + 16, date!(2026 - 02 - 03));
    assert!(s.impressions.is_some());
    assert_eq!(s.engagements, None);
}

#[test]
fn tolerates_empty_impressions_side() {
    // 2026-02-04 populates engagements only.
    let snaps = corpus();
    let s = find(&snaps, BASE + 31, date!(2026 - 02 - 04));
    assert!(s.engagements.is_some());
    assert_eq!(s.impressions, None);
}

#[test]
fn tolerates_unequal_table_lengths() {
    // 2026-02-05: engagements 46..=58, impressions 50..=58. Posts
    // 46..=49 have engagements but no impressions that day.
    let snaps = corpus();
    let eng_only = find(&snaps, BASE + 46, date!(2026 - 02 - 05));
    assert!(eng_only.engagements.is_some());
    assert_eq!(eng_only.impressions, None);
    let both = find(&snaps, BASE + 50, date!(2026 - 02 - 05));
    assert!(both.engagements.is_some());
    assert!(both.impressions.is_some());
}

#[test]
fn snapshot_date_comes_from_filename_window() {
    let snaps = corpus();
    // A post unique to the 2026-02-05 export carries that snapshot date.
    let s = find(&snaps, BASE + 58, date!(2026 - 02 - 05));
    assert_eq!(s.snapshot_date, date!(2026 - 02 - 05));
}

#[test]
fn same_post_across_days_is_one_urn_with_per_day_snapshots() {
    // Posts 5..=10 appear in both 2026-02-01 and 2026-02-02.
    let snaps = corpus();
    let urn = format!("urn:li:activity:{}", BASE + 5);
    let dates: BTreeSet<Date> = snaps
        .iter()
        .filter(|s| s.urn == urn)
        .map(|s| s.snapshot_date)
        .collect();
    assert_eq!(
        dates,
        BTreeSet::from([date!(2026 - 02 - 01), date!(2026 - 02 - 02)])
    );
}

#[test]
fn title_case_headers_are_parsed() {
    // The newer `AggregateAnalytics_*` exports title-case the `TOP POSTS`
    // headers (`Post Publish Date`) where `Content_*` used sentence case.
    // The parser matches headers case-insensitively, so both yield posts.
    let dir = TempDir::new().unwrap();
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("TOP POSTS").unwrap();
    sheet.write_string(2, 0, "Post URL").unwrap();
    sheet.write_string(2, 1, "Post Publish Date").unwrap();
    sheet.write_string(2, 2, "Engagements").unwrap();
    sheet.write_string(2, 4, "Post URL").unwrap();
    sheet.write_string(2, 5, "Post Publish Date").unwrap();
    sheet.write_string(2, 6, "Impressions").unwrap();
    write_table(sheet, 0, &rows(1..=3));
    write_table(sheet, 4, &rows(1..=3));
    let path = dir
        .path()
        .join("AggregateAnalytics_Synthetic_2026-02-01_2026-02-01.xlsx");
    workbook.save(&path).unwrap();

    let snaps = linkedin::parse_export(&path).unwrap();
    assert_eq!(snaps.len(), 3, "title-case headers must still yield posts");
    let s = find(&snaps, BASE + 1, date!(2026 - 02 - 01));
    assert_eq!(s.engagements, Some(11));
    assert_eq!(s.impressions, Some(11));
}

#[test]
fn missing_top_posts_sheet_is_an_error() {
    let dir = TempDir::new().unwrap();
    let mut workbook = Workbook::new();
    workbook.add_worksheet().set_name("DISCOVERY").unwrap();
    let path = dir
        .path()
        .join("Content_2026-02-01_2026-02-01_Synthetic.xlsx");
    workbook.save(&path).unwrap();
    assert!(matches!(
        linkedin::parse_export(&path),
        Err(Error::LinkedinSheetMissing { .. })
    ));
}

#[test]
fn header_without_post_url_is_an_error() {
    let dir = TempDir::new().unwrap();
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("TOP POSTS").unwrap();
    sheet.write_string(0, 0, "no usable header").unwrap();
    let path = dir
        .path()
        .join("Content_2026-02-01_2026-02-01_Synthetic.xlsx");
    workbook.save(&path).unwrap();
    assert!(matches!(
        linkedin::parse_export(&path),
        Err(Error::LinkedinHeaderMissing { .. })
    ));
}

#[test]
fn malformed_post_url_is_an_error() {
    let dir = TempDir::new().unwrap();
    let path = single_post_export(dir.path(), "https://example.com/nope", "1/1/2026", 5.0);
    assert!(matches!(
        linkedin::parse_export(&path),
        Err(Error::LinkedinMissingUrn { .. })
    ));
}

#[test]
fn unparsable_publish_date_is_an_error() {
    let dir = TempDir::new().unwrap();
    let path = single_post_export(dir.path(), &url(BASE + 1), "not-a-date", 5.0);
    assert!(matches!(
        linkedin::parse_export(&path),
        Err(Error::LinkedinPublishDate { .. })
    ));
}

#[test]
fn corrupt_file_is_an_open_error() {
    let dir = TempDir::new().unwrap();
    let path = dir
        .path()
        .join("Content_2026-02-01_2026-02-01_Synthetic.xlsx");
    std::fs::write(&path, b"this is not a real xlsx").unwrap();
    assert!(matches!(
        linkedin::parse_export(&path),
        Err(Error::LinkedinOpen { .. })
    ));
}

#[test]
fn filename_without_a_date_is_an_error() {
    let dir = TempDir::new().unwrap();
    let mut workbook = Workbook::new();
    workbook.add_worksheet().set_name("TOP POSTS").unwrap();
    // No parseable date token anywhere in the filename.
    let path = dir.path().join("Content_nope_alsonope.xlsx");
    workbook.save(&path).unwrap();
    assert!(matches!(
        linkedin::parse_export(&path),
        Err(Error::LinkedinFilename { .. })
    ));
}

#[test]
fn parse_dir_ignores_non_export_files() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("README.md"), b"hi").unwrap();
    std::fs::write(dir.path().join("Shares_2026-01-01.csv"), b"x,y").unwrap();
    write_export(dir.path(), "2026-02-01", &rows(1..=3), &[]);
    let snaps = linkedin::parse_dir(dir.path(), &prefixes()).unwrap();
    assert_eq!(snaps.len(), 3);
}
