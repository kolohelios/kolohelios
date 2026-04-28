use clap::Subcommand;
use serde_json::Value;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[derive(Subcommand)]
pub enum CiCommand {
    /// Assert every upstream job in a workflow's needs context succeeded or was skipped
    Gate {
        /// JSON object from `${{ toJson(needs) }}` in the workflow file
        #[arg(long)]
        needs: String,
    },
}

pub fn run(cmd: CiCommand) {
    match cmd {
        CiCommand::Gate { needs } => match gate(&needs) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} {e}");
                std::process::exit(1);
            }
        },
    }
}

#[derive(Debug, PartialEq)]
enum JobOutcome {
    Pass,
    Skip,
    Block,
}

fn classify(result: &str) -> JobOutcome {
    match result {
        "success" => JobOutcome::Pass,
        "skipped" => JobOutcome::Skip,
        _ => JobOutcome::Block,
    }
}

fn gate(needs_json: &str) -> Result<(), String> {
    let parsed: Value =
        serde_json::from_str(needs_json).map_err(|e| format!("could not parse --needs as JSON: {e}"))?;

    let jobs = parsed
        .as_object()
        .ok_or_else(|| "--needs must be a JSON object".to_string())?;

    if jobs.is_empty() {
        return Err("--needs object is empty (no upstream jobs)".to_string());
    }

    let mut blocking: Vec<(String, String)> = Vec::new();

    println!("{BOLD}Gate: checking upstream jobs{RESET}");
    for (name, job) in jobs {
        let result = job
            .get("result")
            .and_then(|r| r.as_str())
            .unwrap_or("missing");
        let outcome = classify(result);
        let (label, color) = match outcome {
            JobOutcome::Pass => ("PASS", GREEN),
            JobOutcome::Skip => ("SKIP", YELLOW),
            JobOutcome::Block => ("FAIL", RED),
        };
        println!("  {color}{BOLD}[{label}]{RESET} {name}: {result}");
        if outcome == JobOutcome::Block {
            blocking.push((name.clone(), result.to_string()));
        }
    }

    println!();
    if blocking.is_empty() {
        println!("{GREEN}{BOLD}Gate passed{RESET} ({} job(s))", jobs.len());
        Ok(())
    } else {
        eprintln!(
            "{RED}{BOLD}Gate failed{RESET}: {} of {} job(s) blocking",
            blocking.len(),
            jobs.len()
        );
        for (name, result) in &blocking {
            eprintln!("  - {name}: {result}");
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_success_passes() {
        assert_eq!(classify("success"), JobOutcome::Pass);
    }

    #[test]
    fn classify_skipped_passes() {
        assert_eq!(classify("skipped"), JobOutcome::Skip);
    }

    #[test]
    fn classify_failure_blocks() {
        assert_eq!(classify("failure"), JobOutcome::Block);
    }

    #[test]
    fn classify_cancelled_blocks() {
        assert_eq!(classify("cancelled"), JobOutcome::Block);
    }

    #[test]
    fn classify_missing_or_unknown_blocks() {
        assert_eq!(classify("missing"), JobOutcome::Block);
        assert_eq!(classify(""), JobOutcome::Block);
    }

    #[test]
    fn gate_rejects_invalid_json() {
        assert!(gate("not json").is_err());
    }

    #[test]
    fn gate_rejects_non_object() {
        assert!(gate("[]").is_err());
    }

    #[test]
    fn gate_rejects_empty_object() {
        assert!(gate("{}").is_err());
    }
}
