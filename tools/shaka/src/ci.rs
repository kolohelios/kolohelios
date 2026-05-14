use std::collections::HashMap;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use clap::Subcommand;
use serde_json::Value;

use crate::term::{BOLD, GREEN, RED, RESET, YELLOW};

#[derive(Subcommand)]
pub enum CiCommand {
    /// Assert every upstream job in a workflow's needs context succeeded or was skipped
    Gate {
        /// JSON object from `${{ toJson(needs) }}` in the workflow file
        #[arg(long)]
        needs: String,
    },
    /// Run a command under `op run`, registering each resolved-secret value
    /// with GitHub Actions log masking first (`::add-mask::<value>`).
    ///
    /// GH auto-masks the literal `secrets.*` values it injects, but not
    /// values `op run` resolves from them (CLOUDFLARE_API_TOKEN,
    /// AWS_SECRET_ACCESS_KEY, etc.). This wrapper enumerates the resolved
    /// env first, emits a mask command for each non-empty new/changed
    /// value, then execs `op run --env-file=<file> -- <args...>`.
    #[command(name = "mask-and-run")]
    MaskAndRun {
        /// Path passed through to `op run --env-file=<path>`.
        #[arg(long)]
        env_file: PathBuf,
        /// Command and args to run under `op run` (separated by `--`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        args: Vec<String>,
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
        CiCommand::MaskAndRun { env_file, args } => match mask_and_run(&env_file, &args) {
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
    let parsed: Value = serde_json::from_str(needs_json)
        .map_err(|e| format!("could not parse --needs as JSON: {e}"))?;

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

fn mask_and_run(env_file: &PathBuf, args: &[String]) -> Result<(), String> {
    // Phase 1: enumerate the resolved env without running the consumer.
    // `env -0` separates entries with NUL so values containing newlines or
    // `=` survive parsing.
    let output = Command::new("op")
        .arg("run")
        .arg("--env-file")
        .arg(env_file)
        .arg("--")
        .arg("env")
        .arg("-0")
        .output()
        .map_err(|e| format!("could not spawn `op run -- env -0`: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "`op run --env-file={} -- env -0` exited with status {}: {}",
            env_file.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let child_env = parse_env_z(&output.stdout)?;
    let parent_env: HashMap<String, String> = std::env::vars().collect();
    let secrets = resolved_secrets(&parent_env, &child_env);

    // Phase 2: register each resolved value with GH Actions log masking.
    // The worker reads workflow commands from any stdout line, so plain
    // `println!` is sufficient. Flush before exec so the masks land before
    // the child writes a single byte.
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for value in &secrets {
        writeln!(out, "::add-mask::{value}").map_err(|e| format!("write add-mask: {e}"))?;
    }
    out.flush().map_err(|e| format!("flush stdout: {e}"))?;
    drop(out);

    // Phase 3: replace this process with the real `op run`. On failure,
    // `exec` returns; on success, it does not.
    let mut cmd = Command::new("op");
    cmd.arg("run").arg("--env-file").arg(env_file).arg("--");
    cmd.args(args);
    Err(format!("exec `op run`: {}", cmd.exec()))
}

/// Parse the NUL-separated output of `env -0` into a map of `KEY -> VALUE`.
/// Entries without an `=` are dropped (env never produces them, but be
/// defensive). The trailing empty entry produced by the final NUL is
/// dropped naturally by the `is_empty` filter.
fn parse_env_z(bytes: &[u8]) -> Result<HashMap<String, String>, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|e| format!("`env -0` output is not utf-8: {e}"))?;
    let mut map = HashMap::new();
    for entry in text.split('\0') {
        if entry.is_empty() {
            continue;
        }
        if let Some((k, v)) = entry.split_once('=') {
            map.insert(k.to_string(), v.to_string());
        }
    }
    Ok(map)
}

/// Return the values present in `child` that are non-empty and either
/// absent from `parent` or differ from the parent value. Sorted and
/// deduplicated for deterministic output.
fn resolved_secrets(
    parent: &HashMap<String, String>,
    child: &HashMap<String, String>,
) -> Vec<String> {
    let mut values: Vec<String> = child
        .iter()
        .filter(|(k, v)| !v.is_empty() && parent.get(*k).map(|p| p != *v).unwrap_or(true))
        .map(|(_, v)| v.clone())
        .collect();
    values.sort();
    values.dedup();
    values
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

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn parse_env_z_handles_trailing_nul_and_multiline_values() {
        let bytes = b"A=1\0B=two\nlines\0C=\0";
        let got = parse_env_z(bytes).unwrap();
        assert_eq!(got.get("A"), Some(&"1".to_string()));
        assert_eq!(got.get("B"), Some(&"two\nlines".to_string()));
        assert_eq!(got.get("C"), Some(&"".to_string()));
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn resolved_secrets_picks_new_vars() {
        let parent = map(&[("PATH", "/usr/bin")]);
        let child = map(&[("PATH", "/usr/bin"), ("SECRET", "abc")]);
        assert_eq!(resolved_secrets(&parent, &child), vec!["abc".to_string()]);
    }

    #[test]
    fn resolved_secrets_skips_parent_equal_values() {
        let parent = map(&[("FOO", "same")]);
        let child = map(&[("FOO", "same")]);
        assert!(resolved_secrets(&parent, &child).is_empty());
    }

    #[test]
    fn resolved_secrets_skips_empty_values() {
        let parent = map(&[]);
        let child = map(&[("EMPTY", ""), ("SET", "value")]);
        assert_eq!(resolved_secrets(&parent, &child), vec!["value".to_string()]);
    }

    #[test]
    fn resolved_secrets_includes_overridden_values() {
        let parent = map(&[("TOKEN", "placeholder")]);
        let child = map(&[("TOKEN", "real")]);
        assert_eq!(resolved_secrets(&parent, &child), vec!["real".to_string()]);
    }

    #[test]
    fn resolved_secrets_deduplicates_repeated_values() {
        let parent = map(&[]);
        let child = map(&[("A", "shared"), ("B", "shared"), ("C", "unique")]);
        assert_eq!(
            resolved_secrets(&parent, &child),
            vec!["shared".to_string(), "unique".to_string()]
        );
    }
}
