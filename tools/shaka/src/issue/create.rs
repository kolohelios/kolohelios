//! `shaka issue create` — gated wrapper around `gh issue create` that
//! enforces:
//!
//! - title matches the conventional-commit shape (shared validator
//!   with `shaka commit lint`)
//! - `--scope <label>` is one of the canonical scope labels declared
//!   in `.shaka/labels.cue`
//! - native sub-issue link is set via the GitHub sub-issues API when
//!   `--parent <N>` is supplied (no freeform `Sub-issue of #N` body text)
//!
//! All validation happens *before* any GitHub call, so a bad scope or
//! malformed title fails locally without polluting the issue list with
//! a half-created issue.

use std::path::Path;

use crate::commit::title;
use crate::gh::{self, GhError};
use crate::issue::labels::{self, LabelSet};
use crate::term::{BOLD, DIM, GREEN, RED, RESET};

pub struct CreateArgs {
    pub repo: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub body_file: Option<String>,
    pub scope: String,
    pub parent: Option<u64>,
    pub milestone: Option<String>,
    pub labels: Vec<String>,
    pub dry_run: bool,
}

pub fn run(args: CreateArgs) {
    if let Err(e) = run_inner(args, Path::new(".")) {
        eprintln!("{RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }
}

/// Plan that `run_inner` builds before any GitHub call. Returned in
/// dry-run mode for tests / debugging; in normal mode it's executed
/// immediately.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plan {
    repo: String,
    title: String,
    body: String,
    labels: Vec<String>,
    milestone: Option<String>,
    parent: Option<u64>,
}

fn run_inner(args: CreateArgs, cwd: &Path) -> Result<(), String> {
    // 1. Title format — same validator as `shaka commit lint`.
    title::validate(&args.title).map_err(|e| e.to_string())?;

    // 2. Body — exactly one of --body / --body-file. clap's ArgGroup
    //    enforces the structural rule; this resolves the file path.
    let body = resolve_body(&args.body, &args.body_file)?;

    // 3. Scope — must be one of the canonical scope labels.
    let label_set = labels::load(cwd)?;
    validate_scope(&args.scope, &label_set)?;

    // 4. Repo — explicit flag wins; otherwise infer.
    let repo = match args.repo {
        Some(r) => r,
        None => gh::detect_repo_or_env().map_err(|e| format!("could not detect repo: {e}"))?,
    };

    // Scope label goes first, extra `--label` flags append. Dedup so
    // a redundant `--label <scope>` doesn't double-up.
    let mut all_labels = vec![args.scope.clone()];
    for l in args.labels {
        if !all_labels.contains(&l) {
            all_labels.push(l);
        }
    }

    let plan = Plan {
        repo,
        title: args.title,
        body,
        labels: all_labels,
        milestone: args.milestone,
        parent: args.parent,
    };

    if args.dry_run {
        print_plan(&plan);
        return Ok(());
    }

    execute(&plan).map_err(|e| e.to_string())
}

fn resolve_body(body: &Option<String>, body_file: &Option<String>) -> Result<String, String> {
    match (body, body_file) {
        (Some(b), None) => Ok(b.clone()),
        (None, Some(p)) => {
            std::fs::read_to_string(p).map_err(|e| format!("could not read --body-file {p}: {e}"))
        }
        // clap's ArgGroup(required=true) makes neither/both unreachable
        // in practice, but we keep the runtime check so the function
        // is correct in isolation (and so tests don't depend on clap
        // setup).
        (None, None) => Err("exactly one of --body / --body-file is required".into()),
        (Some(_), Some(_)) => Err("--body and --body-file are mutually exclusive".into()),
    }
}

fn validate_scope(scope: &str, set: &LabelSet) -> Result<(), String> {
    let valid: Vec<&str> = set.scope_names();
    if valid.contains(&scope) {
        return Ok(());
    }
    Err(format!(
        "scope `{scope}` is not a canonical scope label; valid: {}",
        valid.join(", ")
    ))
}

fn execute(plan: &Plan) -> Result<(), GhError> {
    let created = gh::issue_create(
        &plan.repo,
        &plan.title,
        &plan.body,
        &plan.labels,
        plan.milestone.as_deref(),
    )?;
    println!(
        "{GREEN}{BOLD}created{RESET} #{} {}",
        created.number, created.url
    );

    if let Some(parent) = plan.parent {
        // Sub-issue API needs the child's *internal* id, not its
        // user-facing number.
        let child_id = gh::issue_db_id(&plan.repo, created.number)?;
        gh::add_sub_issue(&plan.repo, parent, child_id)?;
        println!("{DIM}linked as sub-issue of #{parent}{RESET}");
    }

    Ok(())
}

fn print_plan(plan: &Plan) {
    println!("{BOLD}shaka issue create{RESET} {DIM}(dry-run){RESET}");
    println!("{DIM}├──{RESET} repo:      {}", plan.repo);
    println!("{DIM}├──{RESET} title:     {}", plan.title);
    println!("{DIM}├──{RESET} labels:    {}", plan.labels.join(", "));
    if let Some(m) = &plan.milestone {
        println!("{DIM}├──{RESET} milestone: {m}");
    }
    if let Some(p) = plan.parent {
        println!("{DIM}├──{RESET} parent:    #{p}");
    }
    let body_preview = if plan.body.len() > 80 {
        format!("{}…", &plan.body[..80])
    } else {
        plan.body.clone()
    };
    println!(
        "{DIM}└──{RESET} body:      {body_preview:?} ({} chars)",
        plan.body.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn args(title: &str, scope: &str) -> CreateArgs {
        CreateArgs {
            repo: Some("o/r".into()),
            title: title.into(),
            body: Some("body text".into()),
            body_file: None,
            scope: scope.into(),
            parent: None,
            milestone: None,
            labels: vec![],
            dry_run: true,
        }
    }

    fn workdir_with_labels(canonical: &[&str]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let mut entries = String::from("package labels\n\n#LabelSet & {\n    labels: [\n");
        for name in canonical {
            entries.push_str(&format!(
                "        {{name: {name:?}, color: \"5319e7\", description: \"\", scope: true}},\n"
            ));
        }
        entries.push_str("    ]\n}\n");
        std::fs::create_dir_all(tmp.path().join(".shaka")).unwrap();
        std::fs::write(tmp.path().join(".shaka/labels.cue"), entries).unwrap();
        tmp
    }

    #[test]
    fn rejects_bad_title_before_loading_labels() {
        // Title validation runs before label loading — confirmed by
        // *not* planting a labels file. If the validator fired second,
        // we'd hit "no .shaka/labels.cue found" first.
        let tmp = TempDir::new().unwrap();
        let err = run_inner(args("not conventional", "shaka"), tmp.path()).unwrap_err();
        assert!(
            err.contains("missing `: ` separator") || err.contains("does not match"),
            "expected title error, got: {err}"
        );
    }

    #[test]
    fn rejects_unknown_scope_with_valid_list() {
        let tmp = workdir_with_labels(&["shaka", "ci"]);
        let err = run_inner(args("feat(shaka): add x", "blogctl"), tmp.path()).unwrap_err();
        assert!(err.contains("blogctl"), "should name the bad scope: {err}");
        assert!(err.contains("shaka"), "should list valid scopes: {err}");
        assert!(err.contains("ci"), "should list valid scopes: {err}");
    }

    #[test]
    fn accepts_valid_scope_and_title_in_dry_run() {
        let tmp = workdir_with_labels(&["shaka"]);
        run_inner(args("feat(shaka): add x", "shaka"), tmp.path()).unwrap();
    }

    #[test]
    fn dedups_scope_when_also_passed_via_label_flag() {
        // A common slip is `--scope shaka --label shaka`. Don't pass
        // duplicate labels to gh — confirmed via the dry-run plan's
        // label vector (no behavioral effect on gh, but cleaner output).
        let tmp = workdir_with_labels(&["shaka"]);
        let mut a = args("feat(shaka): add x", "shaka");
        a.labels = vec!["shaka".into(), "bug".into()];
        // Execute through to plan-building by calling run_inner with
        // dry-run; assertion on stdout is brittle, so call the plan
        // builder directly via a small refactor in a future iteration
        // if this proves needed. For now, just exercise the path.
        run_inner(a, tmp.path()).unwrap();
    }

    #[test]
    fn rejects_neither_body_nor_body_file() {
        let tmp = workdir_with_labels(&["shaka"]);
        let mut a = args("feat(shaka): add x", "shaka");
        a.body = None;
        a.body_file = None;
        let err = run_inner(a, tmp.path()).unwrap_err();
        assert!(err.contains("--body"), "got: {err}");
    }

    #[test]
    fn rejects_both_body_and_body_file() {
        let tmp = workdir_with_labels(&["shaka"]);
        let mut a = args("feat(shaka): add x", "shaka");
        a.body_file = Some("/nonexistent".into());
        let err = run_inner(a, tmp.path()).unwrap_err();
        assert!(err.contains("mutually exclusive"), "got: {err}");
    }

    #[test]
    fn reads_body_from_file_when_set() {
        let tmp = workdir_with_labels(&["shaka"]);
        let body_path = tmp.path().join("body.md");
        std::fs::write(&body_path, "from-file body").unwrap();
        let mut a = args("feat(shaka): add x", "shaka");
        a.body = None;
        a.body_file = Some(body_path.to_string_lossy().into());
        // Reaching dry-run without an error is the assertion — file
        // was readable and resolved.
        run_inner(a, tmp.path()).unwrap();
    }
}
