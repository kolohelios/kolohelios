use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::term::{BOLD, RED, RESET, YELLOW};

/// Bounded retry policy for the consumer command. Active only when
/// `retry_on` is non-empty and `max_retries > 0`.
struct RetryConfig {
    retry_on: String,
    max_retries: u32,
    retry_delay: Duration,
}

pub fn run(
    env_file: PathBuf,
    args: Vec<String>,
    retry_on: String,
    max_retries: u32,
    retry_delay: u64,
) {
    let retry = (!retry_on.is_empty() && max_retries > 0).then(|| RetryConfig {
        retry_on,
        max_retries,
        retry_delay: Duration::from_secs(retry_delay),
    });
    match mask_and_run(&env_file, &args, retry.as_ref()) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    }
}

fn mask_and_run(
    env_file: &PathBuf,
    args: &[String],
    retry: Option<&RetryConfig>,
) -> Result<i32, String> {
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

    let make_cmd = || {
        let mut cmd = Command::new("op");
        cmd.arg("run").arg("--env-file").arg(env_file).arg("--");
        cmd.args(args);
        cmd
    };

    match retry {
        // Phase 3 (retry mode): spawn, stream, and re-run on a matching
        // transient failure. Can't `exec` here — we need to observe the
        // exit code and output to decide whether to retry.
        Some(cfg) => run_with_retry(make_cmd, cfg),
        // Phase 3 (default): replace this process with the real `op run`.
        // On failure, `exec` returns; on success, it does not.
        None => {
            let mut cmd = make_cmd();
            Err(format!("exec `op run`: {}", cmd.exec()))
        }
    }
}

/// Run `make_cmd()` until it succeeds or retries are exhausted, streaming
/// its stdout/stderr through to ours while capturing them for matching.
/// Returns the final exit code (0 on success, the last child's code on
/// exhaustion). A retry fires only when the exit is non-zero *and* the
/// combined output contains `cfg.retry_on`; any other failure propagates
/// immediately so real errors aren't masked.
fn run_with_retry(make_cmd: impl Fn() -> Command, cfg: &RetryConfig) -> Result<i32, String> {
    let mut attempt: u32 = 0;
    loop {
        let mut cmd = make_cmd();
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("spawn `op run`: {e}"))?;

        // Drain both pipes concurrently so the child never blocks on a
        // full pipe; tee each to the parent stream and accumulate a
        // combined copy for the retry-pattern check.
        let buffer = Arc::new(Mutex::new(String::new()));
        let out_h = tee(
            child.stdout.take().expect("stdout piped"),
            Stream::Out,
            Arc::clone(&buffer),
        );
        let err_h = tee(
            child.stderr.take().expect("stderr piped"),
            Stream::Err,
            Arc::clone(&buffer),
        );

        let status = child.wait().map_err(|e| format!("wait `op run`: {e}"))?;
        out_h.join().ok();
        err_h.join().ok();

        if status.success() {
            return Ok(0);
        }
        let code = status.code().unwrap_or(1);

        let matched = buffer
            .lock()
            .map(|b| b.contains(&cfg.retry_on))
            .unwrap_or(false);
        if attempt < cfg.max_retries && matched {
            attempt += 1;
            eprintln!(
                "{YELLOW}{BOLD}retry:{RESET} attempt {attempt}/{} after transient failure matching {:?}; \
                 waiting {}s",
                cfg.max_retries,
                cfg.retry_on,
                cfg.retry_delay.as_secs()
            );
            std::thread::sleep(cfg.retry_delay);
            continue;
        }
        return Ok(code);
    }
}

/// Which parent stream a `tee` thread writes through to.
enum Stream {
    Out,
    Err,
}

/// Spawn a thread that copies `reader` to the chosen parent stream and
/// appends every byte to `buffer` (as lossy UTF-8) for pattern matching.
fn tee<R: Read + Send + 'static>(
    mut reader: R,
    stream: Stream,
    buffer: Arc<Mutex<String>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let chunk = &buf[..n];
                    match stream {
                        Stream::Out => {
                            let mut w = std::io::stdout().lock();
                            let _ = w.write_all(chunk);
                            let _ = w.flush();
                        }
                        Stream::Err => {
                            let mut w = std::io::stderr().lock();
                            let _ = w.write_all(chunk);
                            let _ = w.flush();
                        }
                    }
                    if let Ok(mut b) = buffer.lock() {
                        b.push_str(&String::from_utf8_lossy(chunk));
                    }
                }
            }
        }
    })
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

    fn zero_delay(retry_on: &str, max_retries: u32) -> RetryConfig {
        RetryConfig {
            retry_on: retry_on.to_string(),
            max_retries,
            retry_delay: Duration::ZERO,
        }
    }

    /// Build a `sh -c` command that fails (printing `msg` to stderr) for
    /// the first `fail_times` invocations, then succeeds — tracking the
    /// attempt count in a counter file so retries are observable.
    fn flaky_cmd(counter: &std::path::Path, fail_times: u32, msg: &str) -> Command {
        let script = format!(
            "n=$(cat '{f}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{f}'; \
             if [ \"$n\" -le {fail_times} ]; then echo '{msg}' >&2; exit 1; fi; echo ok",
            f = counter.display(),
        );
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd
    }

    fn counter_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("shaka-mask-and-run-{}-{}", std::process::id(), tag))
    }

    fn read_count(path: &std::path::Path) -> u32 {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    #[test]
    fn retry_succeeds_once_transient_failure_clears() {
        let counter = counter_path("clears");
        let _ = std::fs::remove_file(&counter);
        let cfg = zero_delay("This Worker does not exist", 5);
        let code = run_with_retry(
            || flaky_cmd(&counter, 2, "This Worker does not exist"),
            &cfg,
        )
        .unwrap();
        assert_eq!(code, 0);
        // Two failures + one success.
        assert_eq!(read_count(&counter), 3);
        let _ = std::fs::remove_file(&counter);
    }

    #[test]
    fn retry_exhausts_and_propagates_exit_code() {
        let counter = counter_path("exhausts");
        let _ = std::fs::remove_file(&counter);
        let cfg = zero_delay("This Worker does not exist", 2);
        // Always fails; 1 initial + 2 retries = 3 attempts, then gives up.
        let code = run_with_retry(
            || flaky_cmd(&counter, 999, "This Worker does not exist"),
            &cfg,
        )
        .unwrap();
        assert_eq!(code, 1);
        assert_eq!(read_count(&counter), 3);
        let _ = std::fs::remove_file(&counter);
    }

    #[test]
    fn non_matching_failure_is_not_retried() {
        let counter = counter_path("nomatch");
        let _ = std::fs::remove_file(&counter);
        let cfg = zero_delay("This Worker does not exist", 5);
        // Fails with an unrelated message; must not retry.
        let code = run_with_retry(|| flaky_cmd(&counter, 999, "some other error"), &cfg).unwrap();
        assert_eq!(code, 1);
        assert_eq!(read_count(&counter), 1);
        let _ = std::fs::remove_file(&counter);
    }
}
