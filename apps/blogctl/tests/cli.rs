//! End-to-end integration tests against the compiled `blogctl` binary.
//!
//! Each test runs the binary as a subprocess against a fresh `TempDir`
//! workdir, so the assertions exercise the same code path users hit.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_blogctl");

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("failed to spawn blogctl")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_success(out: &Output, label: &str) {
    assert!(
        out.status.success(),
        "{label} failed: stdout={} stderr={}",
        stdout(out),
        stderr(out)
    );
}

fn workdir_arg(workdir: &Path) -> String {
    workdir.to_str().expect("workdir path utf-8").to_string()
}

/// Initialize the tempdir as a colocated `jj` repo so `doctor` and
/// `fix` see `Status::Ok` from the sync layer. blogctl treats `.jj`
/// as a hard precondition for those commands.
fn jj_init(workdir: &Path) {
    let out = Command::new("jj")
        .args(["git", "init", "--colocate"])
        .current_dir(workdir)
        .output()
        .expect("failed to spawn jj");
    assert!(
        out.status.success(),
        "jj git init --colocate failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn init_creates_expected_layout() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    let out = run(&["init", "--workdir", &wd]);
    assert_success(&out, "init");

    for d in [
        "concepts",
        "ideation",
        "editing",
        "final-editing",
        "published",
        "abandoned",
    ] {
        assert!(tmp.path().join(d).is_dir(), "missing dir after init: {d}");
    }
    for absent in ["history", "prompts"] {
        assert!(
            !tmp.path().join(absent).exists(),
            "should not create {absent}/"
        );
    }
    assert!(tmp.path().join(".blog-os.toml").is_file());
}

#[test]
fn vertical_slice_init_new_list_promote_show_demote() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");
    assert_success(&run(&["new", "Hello World", "--workdir", &wd]), "new");
    assert!(tmp.path().join("concepts/hello-world.md").is_file());

    let list_concept = run(&["list", "--workdir", &wd]);
    assert_success(&list_concept, "list (concept)");
    assert!(stdout(&list_concept).contains("hello-world"));
    assert!(stdout(&list_concept).contains("concept"));

    assert_success(
        &run(&["promote", "hello-world", "--workdir", &wd]),
        "promote",
    );
    assert!(tmp.path().join("ideation/hello-world.md").is_file());
    assert!(!tmp.path().join("concepts/hello-world.md").exists());

    let show = run(&["show", "hello-world", "--workdir", &wd]);
    assert_success(&show, "show");
    assert!(stdout(&show).contains("status: ideation"));
    assert!(stdout(&show).contains("title: Hello World"));

    assert_success(&run(&["demote", "hello-world", "--workdir", &wd]), "demote");
    assert!(tmp.path().join("concepts/hello-world.md").is_file());
}

#[test]
fn promote_walks_to_published_then_refuses() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");
    assert_success(&run(&["new", "Pipe", "--workdir", &wd]), "new");

    for stage in ["ideation", "editing", "final-editing", "published"] {
        let out = run(&["promote", "pipe", "--workdir", &wd]);
        assert_success(&out, &format!("promote -> {stage}"));
    }
    let blocked = run(&["promote", "pipe", "--workdir", &wd]);
    assert!(!blocked.status.success());
    assert!(stderr(&blocked).contains("final workflow stage"));
}

#[test]
fn demote_from_concept_fails() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");
    assert_success(&run(&["new", "Stuck", "--workdir", &wd]), "new");
    let out = run(&["demote", "stuck", "--workdir", &wd]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("first workflow stage"));
}

#[test]
fn demote_from_published_is_blocked() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");
    assert_success(&run(&["new", "Walk", "--workdir", &wd]), "new");
    for _ in 0..4 {
        assert_success(&run(&["promote", "walk", "--workdir", &wd]), "promote");
    }
    let out = run(&["demote", "walk", "--workdir", &wd]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("published"));
}

#[test]
fn commands_fail_when_workdir_uninitialized() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    for args in [
        vec!["new", "X", "--workdir", &wd],
        vec!["list", "--workdir", &wd],
        vec!["show", "x", "--workdir", &wd],
    ] {
        let out = run(&args);
        assert!(
            !out.status.success(),
            "expected failure: {args:?}; stdout={}",
            stdout(&out)
        );
        assert!(
            stderr(&out).contains("not initialized"),
            "stderr was: {}",
            stderr(&out)
        );
    }
}

#[test]
fn new_rejects_duplicate_slug() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");
    assert_success(&run(&["new", "Same", "--workdir", &wd]), "new");
    let dup = run(&["new", "Same", "--workdir", &wd]);
    assert!(!dup.status.success());
    assert!(
        stderr(&dup).contains("multiple posts share slug") || stderr(&dup).contains("share slug")
    );
}

#[test]
fn new_accepts_explicit_slug_override() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");
    assert_success(
        &run(&["new", "A Long Title", "--workdir", &wd, "--slug", "short"]),
        "new with --slug",
    );
    assert!(tmp.path().join("concepts/short.md").is_file());
}

#[test]
fn init_writes_readme_template() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");
    let readme =
        std::fs::read_to_string(tmp.path().join("README.md")).expect("README.md after init");
    for needle in [
        "jj git init --colocate",
        "--allow-new",
        "--allow-backwards",
        "kind: post",
    ] {
        assert!(readme.contains(needle), "README missing {needle:?}");
    }
}

#[test]
fn readme_regenerate_overwrites_user_edits() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");
    std::fs::write(tmp.path().join("README.md"), "stale\n").unwrap();
    assert_success(
        &run(&["readme", "regenerate", "--workdir", &wd]),
        "readme regenerate",
    );
    let readme = std::fs::read_to_string(tmp.path().join("README.md")).unwrap();
    assert!(readme.contains("jj git init --colocate"));
}

#[test]
fn new_writes_kind_into_frontmatter() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");
    assert_success(
        &run(&["new", "Long Form", "--workdir", &wd, "--kind", "article"]),
        "new --kind article",
    );
    let body = std::fs::read_to_string(tmp.path().join("concepts/long-form.md")).unwrap();
    assert!(body.contains("kind: article"));

    assert_success(&run(&["new", "Short", "--workdir", &wd]), "new (default)");
    let body = std::fs::read_to_string(tmp.path().join("concepts/short.md")).unwrap();
    assert!(body.contains("kind: post"));
}

#[test]
fn new_writes_theme_into_frontmatter() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");

    assert_success(&run(&["new", "Default", "--workdir", &wd]), "new (default)");
    let body = std::fs::read_to_string(tmp.path().join("concepts/default.md")).unwrap();
    assert!(body.contains("theme: standard"));

    assert_success(
        &run(&["new", "Allegory", "--workdir", &wd, "--theme", "parable"]),
        "new --theme parable",
    );
    let body = std::fs::read_to_string(tmp.path().join("concepts/allegory.md")).unwrap();
    assert!(body.contains("theme: parable"));
}

#[test]
fn new_rejects_unknown_theme_with_known_list() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd]), "init");
    let out = run(&["new", "Bogus", "--workdir", &wd, "--theme", "noir"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("unknown theme"), "stderr was: {err}");
    assert!(err.contains("standard"), "stderr was: {err}");
    assert!(err.contains("parable"), "stderr was: {err}");
}

#[test]
fn doctor_reports_clean_workdir_with_zero_exit() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());
    jj_init(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd, "--no-sync"]), "init");
    assert_success(
        &run(&["new", "Hello", "--workdir", &wd, "--no-sync"]),
        "new",
    );

    let out = run(&["doctor", "--workdir", &wd]);
    assert_success(&out, "doctor (clean)");
    assert!(stdout(&out).contains("workdir healthy"));
}

#[test]
fn fix_repairs_stage_mismatch_and_leaves_workdir_clean() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());
    jj_init(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd, "--no-sync"]), "init");
    assert_success(
        &run(&["new", "Hello", "--workdir", &wd, "--no-sync"]),
        "new",
    );
    // Move the file from concepts/ to editing/ without rewriting
    // frontmatter — that's a stage mismatch fix can repair.
    let from = tmp.path().join("concepts/hello.md");
    let to = tmp.path().join("editing/hello.md");
    std::fs::rename(&from, &to).unwrap();

    let fix_out = run(&["fix", "--workdir", &wd, "--no-sync"]);
    assert_success(&fix_out, "fix");
    assert!(
        stdout(&fix_out).contains("fixed"),
        "got: {}",
        stdout(&fix_out)
    );

    // Doctor exits zero now.
    assert_success(&run(&["doctor", "--workdir", &wd]), "doctor after fix");
    let body = std::fs::read_to_string(&to).unwrap();
    assert!(
        body.contains("status: editing"),
        "frontmatter was not rewritten: {body}"
    );
}

#[test]
fn fix_dry_run_makes_no_changes() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());
    jj_init(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd, "--no-sync"]), "init");
    assert_success(
        &run(&["new", "Hello", "--workdir", &wd, "--no-sync"]),
        "new",
    );
    let from = tmp.path().join("concepts/hello.md");
    let to = tmp.path().join("editing/hello.md");
    std::fs::rename(&from, &to).unwrap();
    let raw_before = std::fs::read_to_string(&to).unwrap();

    let out = run(&["fix", "--workdir", &wd, "--dry-run"]);
    assert_success(&out, "fix --dry-run");
    let raw_after = std::fs::read_to_string(&to).unwrap();
    assert_eq!(raw_before, raw_after, "dry-run modified the file");
}

#[test]
fn fix_exits_nonzero_when_skipped_findings_remain() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());
    jj_init(tmp.path());
    assert_success(&run(&["init", "--workdir", &wd, "--no-sync"]), "init");
    std::fs::write(tmp.path().join("concepts/.DS_Store"), "x").unwrap();

    let out = run(&["fix", "--workdir", &wd, "--no-sync"]);
    assert!(!out.status.success(), "expected non-zero exit");
    assert!(stdout(&out).contains("skipped"), "got: {}", stdout(&out));
}

#[test]
fn doctor_reports_findings_with_nonzero_exit() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());
    jj_init(tmp.path());

    assert_success(&run(&["init", "--workdir", &wd, "--no-sync"]), "init");
    // Plant several distinct findings: stray file, removed stage dir.
    std::fs::write(tmp.path().join("concepts/.DS_Store"), "x").unwrap();
    std::fs::remove_dir_all(tmp.path().join("editing")).unwrap();

    let out = run(&["doctor", "--workdir", &wd]);
    assert!(!out.status.success(), "doctor should exit non-zero");
    let combined = format!("{}{}", stdout(&out), stderr(&out));
    assert!(combined.contains("stray entry"), "got: {combined}");
    assert!(
        combined.contains("stage directory missing"),
        "got: {combined}"
    );
    assert!(combined.contains("workdir unhealthy"), "got: {combined}");
}

#[test]
fn doctor_flags_missing_jj_repo() {
    // No `jj_init` here — the workdir is intentionally not a jj repo.
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());
    assert_success(&run(&["init", "--workdir", &wd]), "init");

    let out = run(&["doctor", "--workdir", &wd]);
    assert!(!out.status.success(), "doctor should exit non-zero");
    let combined = format!("{}{}", stdout(&out), stderr(&out));
    assert!(combined.contains("not a jj repo"), "got: {combined}");
}

#[test]
fn fix_refuses_to_write_without_jj_repo() {
    let tmp = TempDir::new().unwrap();
    let wd = workdir_arg(tmp.path());
    assert_success(&run(&["init", "--workdir", &wd]), "init");
    assert_success(&run(&["new", "Hello", "--workdir", &wd]), "new");
    // Plant a repair-able stage mismatch — fix would normally land it.
    let from = tmp.path().join("concepts/hello.md");
    let to = tmp.path().join("editing/hello.md");
    let raw_before = std::fs::read_to_string(&from).unwrap();
    std::fs::rename(&from, &to).unwrap();

    let out = run(&["fix", "--workdir", &wd]);
    assert!(!out.status.success(), "fix should refuse without jj");
    let combined = format!("{}{}", stdout(&out), stderr(&out));
    assert!(combined.contains("not a jj repo"), "got: {combined}");
    // The repairable file must be untouched.
    let raw_after = std::fs::read_to_string(&to).unwrap();
    assert_eq!(raw_before, raw_after, "fix wrote without commit tracking");
}
