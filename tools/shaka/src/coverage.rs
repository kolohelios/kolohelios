use crate::validate::{discover, write_schema};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[derive(Deserialize)]
struct ProjectMeta {
    name: String,
    kind: String,
}

#[derive(Deserialize)]
struct LlvmCovExport {
    data: Vec<LlvmCovRun>,
}

#[derive(Deserialize)]
struct LlvmCovRun {
    totals: LlvmCovTotals,
}

#[derive(Deserialize)]
struct LlvmCovTotals {
    lines: LlvmCovMetric,
    branches: LlvmCovMetric,
}

#[derive(Deserialize)]
struct LlvmCovMetric {
    count: u64,
    percent: f64,
}

struct CoverageMetrics {
    line_percent: f64,
    branch_percent: Option<f64>,
}

pub fn run(project_filter: Option<String>) {
    let schema_path = match write_schema() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} could not write schema: {e}");
            std::process::exit(1);
        }
    };

    let projects = discover(Path::new("."));
    let mut rust_projects = Vec::new();
    let mut filter_failed = false;

    for project_dir in projects {
        match read_project_meta(&schema_path, &project_dir) {
            Ok(meta) if meta.kind == "rust" => {
                if project_filter
                    .as_deref()
                    .map_or(true, |f| f == meta.name.as_str())
                {
                    rust_projects.push((project_dir, meta));
                }
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!(
                    "{YELLOW}{BOLD}warn:{RESET} could not read {}: {DIM}{e}{RESET}",
                    project_dir.display()
                );
            }
        }
    }

    if rust_projects.is_empty() {
        if let Some(name) = project_filter.as_deref() {
            eprintln!("{RED}{BOLD}error:{RESET} no rust project named {name}");
            filter_failed = true;
        } else {
            println!("{YELLOW}no rust projects found{RESET}");
        }
        if filter_failed {
            std::process::exit(1);
        }
        return;
    }

    println!(
        "{BOLD}coverage:{RESET} {} rust project{}",
        rust_projects.len(),
        if rust_projects.len() == 1 { "" } else { "s" }
    );

    let mut summaries = Vec::new();
    let mut errors = 0;
    for (project_dir, meta) in &rust_projects {
        match run_coverage(project_dir) {
            Ok(metrics) => {
                let branch_str = metrics
                    .branch_percent
                    .map(|b| format!(", branch {b:.1}%"))
                    .unwrap_or_default();
                println!(
                    "  {GREEN}{BOLD}ok{RESET}    {} ({DIM}line {:.1}%{branch_str}{RESET})",
                    meta.name, metrics.line_percent,
                );
                summaries.push((meta.name.clone(), metrics));
            }
            Err(e) => {
                eprintln!(
                    "  {RED}{BOLD}ERROR{RESET} {} ({DIM}{e}{RESET})",
                    meta.name
                );
                errors += 1;
            }
        }
    }

    if !summaries.is_empty() {
        println!();
        let line_avg = summaries.iter().map(|(_, m)| m.line_percent).sum::<f64>()
            / summaries.len() as f64;
        let branch_avg: Vec<f64> =
            summaries.iter().filter_map(|(_, m)| m.branch_percent).collect();
        let branch_str = if branch_avg.is_empty() {
            String::new()
        } else {
            let avg = branch_avg.iter().sum::<f64>() / branch_avg.len() as f64;
            format!(", branch {avg:.1}% across {}", branch_avg.len())
        };
        println!(
            "{BOLD}coverage avg:{RESET} line {line_avg:.1}% across {}{branch_str}",
            summaries.len()
        );
    }

    if errors > 0 {
        std::process::exit(1);
    }
}

fn read_project_meta(
    schema_path: &Path,
    project_dir: &Path,
) -> Result<ProjectMeta, String> {
    let project_file = project_dir.join("project.cue");
    if !project_file.exists() {
        return Err("no project.cue".to_string());
    }
    let out = Command::new("cue")
        .arg("export")
        .arg("--out")
        .arg("json")
        .arg(schema_path)
        .arg(&project_file)
        .output()
        .map_err(|e| format!("failed to spawn cue: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("could not parse cue output: {e}"))
}

fn run_coverage(project_dir: &Path) -> Result<CoverageMetrics, String> {
    let manifest = absolute(project_dir).join("Cargo.toml");
    if !manifest.exists() {
        return Err(format!("missing {}", manifest.display()));
    }

    let out = Command::new("cargo")
        .arg("llvm-cov")
        .arg("--json")
        .arg("--summary-only")
        .arg("--branch")
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .map_err(|e| format!("failed to spawn cargo llvm-cov: {e}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let last = stderr.lines().last().unwrap_or(&stderr).to_string();
        return Err(format!("cargo llvm-cov failed: {last}"));
    }

    parse_llvm_cov_json(&out.stdout)
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn parse_llvm_cov_json(json: &[u8]) -> Result<CoverageMetrics, String> {
    let export: LlvmCovExport = serde_json::from_slice(json)
        .map_err(|e| format!("could not parse cargo llvm-cov json: {e}"))?;
    let totals = &export
        .data
        .first()
        .ok_or_else(|| "cargo llvm-cov json had no data entries".to_string())?
        .totals;
    Ok(CoverageMetrics {
        line_percent: totals.lines.percent,
        branch_percent: if totals.branches.count > 0 {
            Some(totals.branches.percent)
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_export() {
        let json = br#"{"data":[{"totals":{"lines":{"count":100,"percent":80.5},"branches":{"count":0,"percent":0.0}}}]}"#;
        let m = parse_llvm_cov_json(json).unwrap();
        assert!((m.line_percent - 80.5).abs() < 0.001);
        assert_eq!(m.branch_percent, None);
    }

    #[test]
    fn parse_with_branches() {
        let json = br#"{"data":[{"totals":{"lines":{"count":1890,"percent":32.0},"branches":{"count":102,"percent":71.5}}}]}"#;
        let m = parse_llvm_cov_json(json).unwrap();
        assert!((m.line_percent - 32.0).abs() < 0.001);
        assert!(matches!(m.branch_percent, Some(p) if (p - 71.5).abs() < 0.001));
    }

    #[test]
    fn parse_empty_data_is_error() {
        let json = br#"{"data":[]}"#;
        assert!(parse_llvm_cov_json(json).is_err());
    }

    #[test]
    fn parse_invalid_json_is_error() {
        let json = b"not json";
        assert!(parse_llvm_cov_json(json).is_err());
    }
}
