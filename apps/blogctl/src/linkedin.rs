//! Parser for LinkedIn's daily `Content_<start>_<end>_*.xlsx` analytics
//! exports.
//!
//! Each export's `TOP POSTS` sheet holds two side-by-side tables — one
//! ranked by engagements, one by impressions — that share the columns
//! `Post URL`, `Post publish date`, and the metric. This module reads
//! both, keys every row by the LinkedIn activity URN embedded in the
//! post URL, and merges the two metrics into one [`PostSnapshot`] per
//! distinct post per file. The two tables rank the same posts
//! differently, so the merge is by URN, never by row position.
//!
//! The snapshot date comes from the filename's `<start>_<end>` window,
//! not the sheet, so re-parsing overlapping exports yields stable
//! per-day data points keyed by `(urn, snapshot_date)`.
//!
//! Pure parse: no writes, no network. The batch import (#707) and the
//! metrics refresh (#859) consume these snapshots.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use calamine::{open_workbook_auto, Data, Reader};
use time::macros::format_description;
use time::Date;

use crate::error::{Error, Result};

/// The sheet (third in the workbook) holding the two ranked post tables.
const SHEET: &str = "TOP POSTS";
const URL_HEADER: &str = "Post URL";
const DATE_HEADER: &str = "Post publish date";
const ENGAGEMENTS_HEADER: &str = "Engagements";
const IMPRESSIONS_HEADER: &str = "Impressions";
const URN_PREFIX: &str = "urn:li:activity:";
/// Minimum digit-run length to treat as a LinkedIn activity id. Real ids
/// are 19 digits; the threshold rejects shorter numbers (utm ids,
/// timestamps) that appear elsewhere in a URL.
const MIN_ACTIVITY_ID_LEN: usize = 17;

/// One post's metrics as captured by one daily export.
///
/// `impressions` and `engagements` are independently optional: a given
/// day may populate only one of the two ranked tables (early exports
/// list impressions only), so a post can appear with just one metric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostSnapshot {
    /// Canonical activity URN, e.g. `urn:li:activity:7463470632568008704`.
    pub urn: String,
    /// The post URL the URN was extracted from.
    pub url: String,
    /// The post's publish date, as reported in the export.
    pub publish_date: Date,
    pub impressions: Option<u64>,
    pub engagements: Option<u64>,
    /// The day this export covers, derived from the filename window.
    pub snapshot_date: Date,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Metric {
    Engagements,
    Impressions,
}

/// A table located within the sheet: the columns its rows occupy and
/// which metric its third column carries.
#[derive(Debug, Clone, Copy)]
struct Table {
    url_col: usize,
    date_col: usize,
    metric_col: usize,
    metric: Metric,
}

/// Parse every `Content_*.xlsx` export in `dir`, in filename order,
/// concatenating their per-post snapshots. A post recurs once per day
/// it appears in; de-duplicate by [`PostSnapshot::urn`] to count
/// distinct posts.
pub fn parse_dir(dir: &Path) -> Result<Vec<PostSnapshot>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| Error::io(dir, source))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| is_export(path))
        .collect();
    paths.sort();

    let mut snapshots = Vec::new();
    for path in &paths {
        snapshots.extend(parse_export(path)?);
    }
    Ok(snapshots)
}

/// Parse one daily export into one [`PostSnapshot`] per distinct post.
pub fn parse_export(path: &Path) -> Result<Vec<PostSnapshot>> {
    let snapshot_date = snapshot_date_from_filename(path)?;

    let mut workbook = open_workbook_auto(path).map_err(|source| Error::LinkedinOpen {
        path: path.to_path_buf(),
        source,
    })?;
    if !workbook.sheet_names().iter().any(|name| name == SHEET) {
        return Err(Error::LinkedinSheetMissing {
            path: path.to_path_buf(),
        });
    }
    let range = workbook
        .worksheet_range(SHEET)
        .map_err(|source| Error::LinkedinSheetRead {
            path: path.to_path_buf(),
            source,
        })?;

    let rows: Vec<&[Data]> = range.rows().collect();
    let header_idx = rows
        .iter()
        .position(|row| row.iter().any(|cell| cell_str(cell) == Some(URL_HEADER)))
        .ok_or_else(|| Error::LinkedinHeaderMissing {
            path: path.to_path_buf(),
        })?;
    let tables = locate_tables(rows[header_idx]);

    let mut by_urn: BTreeMap<String, PostSnapshot> = BTreeMap::new();
    for row in &rows[header_idx + 1..] {
        for table in &tables {
            let Some(url) = row
                .get(table.url_col)
                .and_then(cell_str)
                .filter(|url| !url.is_empty())
            else {
                continue;
            };
            let urn = extract_urn(url).ok_or_else(|| Error::LinkedinMissingUrn {
                path: path.to_path_buf(),
                url: url.to_string(),
            })?;
            let date_str = row
                .get(table.date_col)
                .and_then(cell_str)
                .unwrap_or_default();
            let publish_date =
                parse_us_date(date_str).map_err(|source| Error::LinkedinPublishDate {
                    path: path.to_path_buf(),
                    value: date_str.to_string(),
                    source,
                })?;
            let value = row.get(table.metric_col).and_then(cell_u64);

            let entry = by_urn.entry(urn.clone()).or_insert_with(|| PostSnapshot {
                urn: urn.clone(),
                url: url.to_string(),
                publish_date,
                impressions: None,
                engagements: None,
                snapshot_date,
            });
            match table.metric {
                Metric::Engagements => entry.engagements = value,
                Metric::Impressions => entry.impressions = value,
            }
        }
    }
    Ok(by_urn.into_values().collect())
}

/// Locate each table by its `Post URL` header, pairing it with the
/// adjacent `Post publish date` and metric columns. A `Post URL` whose
/// neighbours aren't the expected date + recognized-metric headers is
/// skipped, so an unexpected layout degrades to fewer tables instead
/// of misreading the columns.
fn locate_tables(header: &[Data]) -> Vec<Table> {
    let mut tables = Vec::new();
    for (col, cell) in header.iter().enumerate() {
        if cell_str(cell) != Some(URL_HEADER) {
            continue;
        }
        let date_col = col + 1;
        let metric_col = col + 2;
        if header.get(date_col).and_then(cell_str) != Some(DATE_HEADER) {
            continue;
        }
        let metric = match header.get(metric_col).and_then(cell_str) {
            Some(ENGAGEMENTS_HEADER) => Metric::Engagements,
            Some(IMPRESSIONS_HEADER) => Metric::Impressions,
            _ => continue,
        };
        tables.push(Table {
            url_col: col,
            date_col,
            metric_col,
            metric,
        });
    }
    tables
}

fn cell_str(cell: &Data) -> Option<&str> {
    match cell {
        Data::String(s) => Some(s.as_str()),
        _ => None,
    }
}

/// Coerce a metric cell to a count. LinkedIn stores these as numbers,
/// but tolerate a string-formatted integer (with thousands separators)
/// in case an export varies.
fn cell_u64(cell: &Data) -> Option<u64> {
    match cell {
        Data::Int(i) => u64::try_from(*i).ok(),
        Data::Float(f) if f.is_finite() && *f >= 0.0 => Some(f.round() as u64),
        Data::String(s) => s.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

/// Extract the canonical `urn:li:activity:<id>` from a post URL such as
/// `https://www.linkedin.com/feed/update/urn:li:activity:7463470632568008704`.
/// Any trailing path or query after the id is ignored.
fn extract_urn(url: &str) -> Option<String> {
    let start = url.find(URN_PREFIX)?;
    let digits: String = url[start + URN_PREFIX.len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then(|| format!("{URN_PREFIX}{digits}"))
}

/// Extract the bare LinkedIn activity id (the digit run) from any post
/// URL form: the `urn:li:activity:<id>` of analytics exports, or the
/// `/posts/<slug>-<id>-<code>/?…` of a shared post URL. Returns the
/// longest digit run when it is at least 17 digits (real ids are 19).
///
/// Used to match an export's [`PostSnapshot::urn`] to a post's
/// `targets[].url`, which usually carries the id in the share-URL form
/// rather than the `urn:` form.
pub fn activity_id(url: &str) -> Option<&str> {
    let bytes = url.as_bytes();
    // Track the longest digit run; the activity id is the only long one.
    let mut best: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let len = i - start;
        if best.is_none_or(|(_, best_len)| len > best_len) {
            best = Some((start, len));
        }
    }
    best.filter(|&(_, len)| len >= MIN_ACTIVITY_ID_LEN)
        .map(|(start, len)| &url[start..start + len])
}

/// Parse LinkedIn's `M/D/YYYY` publish dates (month and day unpadded).
fn parse_us_date(value: &str) -> std::result::Result<Date, time::error::Parse> {
    Date::parse(
        value,
        &format_description!("[month padding:none]/[day padding:none]/[year]"),
    )
}

/// Derive the snapshot date from a `Content_<start>_<end>_*.xlsx`
/// filename, using the window end. Dailies have `start == end`; the end
/// is the "as of" date the metrics represent.
fn snapshot_date_from_filename(path: &Path) -> Result<Date> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| Error::LinkedinFilename {
            name: path.display().to_string(),
        })?;
    let window = name
        .strip_prefix("Content_")
        .ok_or_else(|| Error::LinkedinFilename {
            name: name.to_string(),
        })?;
    // window == "<start>_<end>_<rest>.xlsx"; the end date is the 2nd field.
    let end = window
        .split('_')
        .nth(1)
        .ok_or_else(|| Error::LinkedinFilename {
            name: name.to_string(),
        })?;
    Date::parse(end, &format_description!("[year]-[month]-[day]")).map_err(|_| {
        Error::LinkedinFilename {
            name: name.to_string(),
        }
    })
}

fn is_export(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("xlsx")
        && path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("Content_"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn activity_id_from_export_urn() {
        assert_eq!(
            activity_id("urn:li:activity:7440067024082604032"),
            Some("7440067024082604032")
        );
    }

    #[test]
    fn activity_id_from_shared_post_url() {
        let url = "https://www.linkedin.com/posts/kolohelios_the-beaver-community-share-7456442827909005312-8FKm/?utm_source=share";
        assert_eq!(activity_id(url), Some("7456442827909005312"));
    }

    #[test]
    fn activity_id_rejects_urls_without_a_long_id() {
        assert_eq!(activity_id("https://example.com/posts/123"), None);
        assert_eq!(activity_id("https://www.linkedin.com/in/someone"), None);
    }

    #[test]
    fn extract_urn_from_feed_url() {
        assert_eq!(
            extract_urn("https://www.linkedin.com/feed/update/urn:li:activity:7463470632568008704"),
            Some("urn:li:activity:7463470632568008704".to_string())
        );
    }

    #[test]
    fn extract_urn_ignores_trailing_path_and_query() {
        assert_eq!(
            extract_urn("https://www.linkedin.com/feed/update/urn:li:activity:123/?utm=x"),
            Some("urn:li:activity:123".to_string())
        );
    }

    #[test]
    fn extract_urn_rejects_non_activity_urls() {
        assert_eq!(extract_urn("https://www.linkedin.com/in/someone"), None);
        // Prefix present but no trailing digits.
        assert_eq!(extract_urn("urn:li:activity:nope"), None);
    }

    #[test]
    fn parse_us_date_handles_unpadded_month_and_day() {
        assert_eq!(parse_us_date("5/22/2026").unwrap(), date!(2026 - 05 - 22));
        assert_eq!(parse_us_date("1/5/2026").unwrap(), date!(2026 - 01 - 05));
        assert_eq!(parse_us_date("12/9/2026").unwrap(), date!(2026 - 12 - 09));
    }

    #[test]
    fn parse_us_date_rejects_iso_format() {
        assert!(parse_us_date("2026-05-22").is_err());
    }

    #[test]
    fn snapshot_date_uses_window_end() {
        let path = Path::new("/x/Content_2026-04-30_2026-05-01_JonathanEdwards.xlsx");
        assert_eq!(
            snapshot_date_from_filename(path).unwrap(),
            date!(2026 - 05 - 01)
        );
    }

    #[test]
    fn snapshot_date_rejects_unexpected_filenames() {
        for bad in [
            "/x/Shares_2026-04-30_2026-04-30.xlsx", // wrong prefix
            "/x/Content_2026-04-30.xlsx",           // only one date field
            "/x/Content_nope_alsonope_x.xlsx",      // fields aren't dates
        ] {
            assert!(
                matches!(
                    snapshot_date_from_filename(Path::new(bad)),
                    Err(Error::LinkedinFilename { .. })
                ),
                "expected a filename error for {bad}"
            );
        }
    }

    #[test]
    fn cell_u64_coerces_numbers_and_strings() {
        assert_eq!(cell_u64(&Data::Int(5)), Some(5));
        assert_eq!(cell_u64(&Data::Float(74.0)), Some(74));
        assert_eq!(cell_u64(&Data::String("1,234".into())), Some(1234));
        assert_eq!(cell_u64(&Data::Empty), None);
        assert_eq!(cell_u64(&Data::Int(-1)), None);
        assert_eq!(cell_u64(&Data::String("n/a".into())), None);
    }

    #[test]
    fn is_export_matches_only_content_xlsx() {
        assert!(is_export(Path::new(
            "/x/Content_2026-04-30_2026-04-30_X.xlsx"
        )));
        assert!(!is_export(Path::new("/x/Shares_2026-04-30.csv")));
        assert!(!is_export(Path::new("/x/Content_2026-04-30.txt")));
        assert!(!is_export(Path::new("/x/README.md")));
    }
}
