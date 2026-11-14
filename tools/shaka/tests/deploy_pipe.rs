//! Reproduce the confirmed Nix/Crashpad inherited-pipe hang without Nix,
//! Cloudflare, or 1Password access. Use the real shaka masking/retry command.
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(15);
const SECRET: &str = "fixture-credential-not-a-real-secret";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deploy-pipe")
}

fn collect(stream: impl Read + Send + 'static, lines: Sender<String>) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut output = Vec::new();
        loop {
            let mut line = Vec::new();
            if reader.read_until(b'\n', &mut line).unwrap() == 0 {
                return output;
            }
            let _ = lines.send(String::from_utf8_lossy(&line).into_owned());
            output.extend(line);
        }
    })
}

struct Pipeline {
    root: tempfile::TempDir,
    bash: String,
    group: u32,
    input: Option<ChildStdin>,
    lines: Receiver<String>,
    done: Receiver<Output>,
    waiter: Option<JoinHandle<()>>,
}

impl Pipeline {
    fn start(script: &str, reporting: Option<&str>, exit_code: i32, retries: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let bash = Command::new("bash")
            .args(["-c", "command -v bash"])
            .output()
            .unwrap();
        assert!(bash.status.success());
        let bash = String::from_utf8(bash.stdout).unwrap().trim().to_owned();
        for name in ["nix", "op", "wrangler", "holder"] {
            let source = std::fs::read_to_string(fixtures().join(format!("{name}.sh"))).unwrap();
            // Nix's build sandbox has no /usr/bin/env.
            let path = bin.join(name);
            std::fs::write(&path, source.replacen("/usr/bin/env bash", &bash, 1)).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_shaka"), bin.join("shaka")).unwrap();
        std::fs::copy(
            fixtures().join(".env.example"),
            root.path().join(".env.example"),
        )
        .unwrap();
        let path = std::env::join_paths(
            std::iter::once(bin.clone())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let mut command = Command::new(&bash);
        // GitHub's implicit Bash shell uses -e, not pipefail. This must
        // exercise the workflow's explicit PIPESTATUS propagation.
        command
            .args(["-e", "-c", script])
            .current_dir(root.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", root.path())
            .env("CFDEPLOY_SHAKA", &bin)
            .env("PIPE_TEST_BASH", &bash)
            .env("PIPE_TEST_HOLDER", bin.join("holder"))
            .env("PIPE_TEST_COUNTER", root.path().join("attempts"))
            .env("PIPE_TEST_ARGS", root.path().join("arguments"))
            .env("PIPE_TEST_LOG", root.path().join("output.log"))
            .env("PIPE_TEST_STATUS", exit_code.to_string())
            .env("PIPE_TEST_RETRIES", if retries { "1" } else { "0" })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        if let Some(value) = reporting {
            command.env("NIX_SENTRY_ENDPOINT", value);
        }
        let mut child = command.spawn().unwrap();
        let group = child.id();
        let input = child.stdin.take();
        let (line_tx, lines) = mpsc::channel();
        let stdout = collect(child.stdout.take().unwrap(), line_tx.clone());
        let stderr = collect(child.stderr.take().unwrap(), line_tx);
        let (done_tx, done) = mpsc::channel();
        let waiter = thread::spawn(move || {
            let status = child.wait().unwrap();
            let output = Output {
                status,
                stdout: stdout.join().unwrap(),
                stderr: stderr.join().unwrap(),
            };
            let _ = done_tx.send(output);
        });
        Self {
            root,
            bash,
            group,
            input,
            lines,
            done,
            waiter: Some(waiter),
        }
    }

    fn marker(&self, marker: &str) {
        let deadline = std::time::Instant::now() + DEADLINE;
        let mut seen = String::new();
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let line = self
                .lines
                .recv_timeout(remaining)
                .unwrap_or_else(|error| panic!("waiting for {marker}: {error}; output: {seen}"));
            seen.push_str(&line);
            if line.contains(marker) {
                return;
            }
        }
    }

    fn finish(&self) -> Output {
        self.done
            .recv_timeout(DEADLINE)
            .expect("pipeline must exit and close output pipes promptly")
    }

    fn release_holder(&mut self) {
        self.input.take().unwrap().write_all(b"release\n").unwrap();
    }

    fn assert_output(&self, output: &Output, exit_code: i32) {
        assert_eq!(
            output.status.code(),
            Some(exit_code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("--dry-run: exiting now."));
        assert_eq!(
            std::fs::read(self.root.path().join("output.log")).unwrap(),
            output.stdout,
            "tee must retain the full streaming log"
        );
        // GitHub consumes add-mask commands. Locally they intentionally
        // contain the fake value: assert registration precedes *all*
        // consumer stdout/stderr containing it, not imaginary local masking.
        let directive = format!("::add-mask::{SECRET}");
        let mask = stdout
            .find(&directive)
            .expect("missing GitHub mask registration");
        let after_mask = mask + directive.len();
        for payload in [
            format!("consumer stdout secret={SECRET}"),
            format!("consumer stderr secret={SECRET}"),
        ] {
            assert!(
                stdout.find(&payload).unwrap() > after_mask,
                "consumer output preceded masking: {stdout}"
            );
        }
        for (index, _) in stdout.match_indices(SECRET) {
            assert!(
                index == mask + "::add-mask::".len() || index > after_mask,
                "credential leaked before registration"
            );
        }
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        // Always release stdin and terminate the private process group,
        // including orphaned holders, even if an assertion panics.
        drop(self.input.take());
        let _ = Command::new(&self.bash)
            .args([
                "-c",
                "kill -TERM -- \"-$1\"",
                "test-cleanup",
                &self.group.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Some(waiter) = self.waiter.take() {
            let _ = waiter.join();
        }
    }
}

fn fixture_pipeline() -> String {
    std::fs::read_to_string(fixtures().join("pipeline.sh")).unwrap()
}

#[test]
fn disabled_reporting_closes_pipes_and_preserves_real_exit_status() {
    for status in [0, 23] {
        let pipeline = Pipeline::start(&fixture_pipeline(), Some(""), status, false);
        pipeline.marker("consumer-finished-1"); // Observe streaming before waiting for exit.
        let output = pipeline.finish();
        pipeline.assert_output(&output, status);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("pipe-holder-ready"));
    }
}

#[test]
fn reporting_descendant_holds_pipe_after_consumer_has_exited() {
    for status in [0, 23] {
        let mut pipeline = Pipeline::start(&fixture_pipeline(), None, status, false);
        pipeline.marker(&format!("consumer-process-exited-{status}"));
        assert!(
            matches!(
                pipeline.done.recv_timeout(Duration::from_millis(150)),
                Err(RecvTimeoutError::Timeout)
            ),
            "consumer completion must not be mistaken for pipeline EOF"
        );
        pipeline.release_holder();
        let output = pipeline.finish();
        pipeline.assert_output(&output, status);
        assert!(String::from_utf8_lossy(&output.stdout).contains("pipe-holder-ready"));
    }
}

#[test]
fn retries_keep_streaming_and_preserve_masking_and_final_status() {
    for status in [0, 23] {
        let pipeline = Pipeline::start(&fixture_pipeline(), Some(""), status, true);
        pipeline.marker("consumer-finished-2");
        let output = pipeline.finish();
        pipeline.assert_output(&output, status);
        assert_eq!(
            std::fs::read_to_string(pipeline.root.path().join("attempts"))
                .unwrap()
                .trim(),
            "2"
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("retry:"));
    }
}

#[test]
fn repository_workflow_retains_and_exercises_the_pipe_safeguards() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if !root.join("tools/shaka/project.cue").is_file() {
        // A standalone Nix/FlakeHub package contains only tools/shaka.
        // Fixture-based pipe tests above still run in its build sandbox;
        // this additional repo-artifact check runs in source preflight/CI.
        eprintln!("repository workflow unavailable in standalone package");
        return;
    }
    let source = std::fs::read_to_string(root.join(".github/workflows/cf-deploy.yml")).unwrap();
    let workflow: serde_yaml_ng::Value = serde_yaml_ng::from_str(&source).unwrap();
    let step = workflow["jobs"]["deploy"]["steps"]
        .as_sequence()
        .unwrap()
        .iter()
        .find(|step| step["id"].as_str() == Some("wrangler"))
        .unwrap();
    let reporting = step["env"]["NIX_SENTRY_ENDPOINT"].as_str();
    assert_eq!(
        reporting,
        Some(""),
        "Nix crash reporting must be disabled before nix develop starts"
    );
    let timeout = step["timeout-minutes"].as_u64().unwrap();
    assert!(
        (1..=15).contains(&timeout),
        "retain the bounded timeout backstop"
    );
    for (dry_run, name, environment, expected_args) in [
        ("--dry-run", "", "", vec!["deploy", "--dry-run"]),
        (
            "",
            "--name fixture-pr-1",
            "",
            vec!["deploy", "--name", "fixture-pr-1"],
        ),
        (
            "",
            "",
            "--env production",
            vec!["deploy", "--env", "production"],
        ),
    ] {
        let script = step["run"].as_str().unwrap()
            .replace("${{ inputs.verify_only && '--dry-run' || '' }}", dry_run)
            .replace("${{ inputs.script_name_override && format('--name {0}', inputs.script_name_override) || '' }}", name)
            .replace("${{ inputs.environment && format('--env {0}', inputs.environment) || '' }}", environment)
            .replace("/tmp/wrangler-output.log", "\"$PIPE_TEST_LOG\"");
        assert!(
            !script.contains("${{"),
            "unhandled workflow input in regression test"
        );
        for status in [0, 23] {
            let pipeline = Pipeline::start(&script, reporting, status, false);
            pipeline.marker("consumer-finished-1");
            pipeline.assert_output(&pipeline.finish(), status);
            let arguments =
                std::fs::read_to_string(pipeline.root.path().join("arguments")).unwrap();
            assert_eq!(arguments.lines().collect::<Vec<_>>(), expected_args);
        }
    }
}
