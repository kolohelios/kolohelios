use std::collections::HashMap;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use crate::term::{BOLD, RED, RESET};

pub fn run(env_file: PathBuf, args: Vec<String>) {
    if let Err(e) = mask_and_run(&env_file, &args) {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
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
