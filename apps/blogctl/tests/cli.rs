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
