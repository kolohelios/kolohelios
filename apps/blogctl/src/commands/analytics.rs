//! `blogctl analytics {summary, compare, recommendations}` — read
//! every published post's classifications + metrics and surface
//! aggregates. summary is implemented; compare and recommendations
//! are still stubs (behavior lands in #440/#441).

use std::path::PathBuf;

use time::OffsetDateTime;

use crate::analytics::{self, DimensionSummary, Summary, ValueSummary};
use crate::error::{Error, Result};
use crate::storage::{Repository, Workdir};
use crate::target::Target;

#[derive(Debug)]
pub struct SummaryArgs {
    pub workdir: PathBuf,
    pub target: Option<Target>,
    pub dimension: Option<String>,
    pub json: bool,
}

#[derive(Debug)]
pub struct CompareArgs {
    pub dim_a: String,
    pub dim_b: String,
    pub workdir: PathBuf,
    pub target: Option<Target>,
    pub min_n: usize,
    pub json: bool,
}

#[derive(Debug)]
pub struct RecommendationsArgs {
    pub workdir: PathBuf,
    pub target: Option<Target>,
    pub min_n: usize,
}

pub fn summary(args: SummaryArgs) -> Result<()> {
    let repo = Repository::open(Workdir::new(&args.workdir))?;
    // Walk every post via Repository::list — fail-fast on any
    // taxonomy / stage / frontmatter problem. The analytics view
    // wants a consistent workdir; doctor reports surface issues.
    let handles = repo.list()?;
    // Re-parse each handle's full Post — list() returns handles
    // with metadata only. The analytics layer needs &[Post] but
    // we only act on metadata, so a thin wrapper avoids re-IO:
    // load_raw is keyed by slug and short-circuits the second read.
    let posts: Vec<_> = handles
        .iter()
        .map(|h| repo.load_raw(&h.metadata.slug).map(|(_, post)| post))
        .collect::<Result<_>>()?;
    let summary = analytics::summary(
        &posts,
        args.target,
        args.dimension.as_deref(),
        OffsetDateTime::now_utc(),
    );
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).map_err(Error::SummaryJson)?
        );
    } else {
        print_text(&summary);
    }
    Ok(())
}

fn print_text(s: &Summary) {
    if s.dimensions.is_empty() {
        println!("no classified posts with metrics in this workdir");
        return;
    }
    for (i, dim) in s.dimensions.iter().enumerate() {
        if i > 0 {
            println!();
        }
        let total: usize = dim.values.iter().map(|v| v.n).sum();
        println!("{} (n={total})", dim.name);
        let width = dim
            .values
            .iter()
            .map(|v| v.value.len())
            .max()
            .unwrap_or(0)
            .max(8);
        for v in &dim.values {
            print_value_row(dim, v, width);
        }
    }
}

fn print_value_row(_dim: &DimensionSummary, v: &ValueSummary, value_width: usize) {
    let imp = format_imp(v.impressions.p50);
    let er = match v.engagement_rate {
        None => "eng p50=—".to_string(),
        Some(p) => format!(
            "eng p50={}  [p25={} p75={}]",
            format_er(p.p50),
            format_er(p.p25),
            format_er(p.p75),
        ),
    };
    let low = if v.low_n { "  (low n)" } else { "" };
    println!(
        "  {value:<width$}  n={n}   imp p50={imp}   {er}{low}",
        value = v.value,
        width = value_width,
        n = v.n,
    );
}

/// `1842 → "1.8k"`. Below 1000 we print the integer; at or above
/// we collapse to thousands with one decimal place. Plenty of
/// precision for a glance-readable analytics view.
fn format_imp(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else {
        format!("{:.1}k", (n as f64) / 1000.0)
    }
}

/// `0.042 → "4.2%"`. One decimal of percent; clipped negative
/// values can't occur (engagement_rate is ratio of counts).
fn format_er(rate: f64) -> String {
    format!("{:.1}%", rate * 100.0)
}

pub fn compare(_args: CompareArgs) -> Result<()> {
    Err(Error::Unimplemented("analytics compare"))
}

pub fn recommendations(_args: RecommendationsArgs) -> Result<()> {
    Err(Error::Unimplemented("analytics recommendations"))
}
