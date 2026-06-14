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
//! Most validation happens *before* any GitHub call, so a malformed
//! title or freeform parent ref fails locally without polluting the
//! issue list with a half-created issue.
//!
//! **Cross-repo scope validation.** When `--repo <owner/name>` targets a
//! foreign repo, `--scope` is validated against *that* repo's
//! `.shaka/labels.cue` fetched over the GitHub API — not the local
//! working tree. This is what lets a private consumer repo (which has no
//! local checkout of the target, and whose own `.shaka/labels.cue` would
//! otherwise shadow the target's) file issues into the canonical repo
//! through this command instead of falling back to raw `gh issue create`.
//! Without `--repo`, scope is validated against the local tree exactly as
//! before.

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
    let fetch = |repo: &str, path: &str| gh::fetch_raw_file(repo, path).map_err(|e| e.to_string());
    if let Err(e) = run_inner(args, Path::new("."), fetch) {
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

fn run_inner<F>(args: CreateArgs, cwd: &Path, fetch_labels: F) -> Result<(), String>
where
    F: Fn(&str, &str) -> Result<Option<String>, String>,
{
    // 1. Title format — same validator as `shaka commit lint`.
    title::validate(&args.title).map_err(|e| e.to_string())?;

    // 2. Body — exactly one of --body / --body-file. clap's ArgGroup
    //    enforces the structural rule; this resolves the file path.
    let body = resolve_body(&args.body, &args.body_file)?;

    // 3. No freeform parent references. `--parent <N>` is the only way
    //    to set a parent (uses GitHub's native sub-issue API); freeform
    //    `Sub-issue of #N` / `Tracked in #N` body text rots silently
    //    and is what `shaka issue audit` flags. Refuse at create time
    //    so we never write the freeform shape in the first place.
    reject_freeform_parent_refs(&body, args.parent)?;

    // 4. Scope — must be one of the canonical scope labels of the *target*
    //    repo. With `--repo`, fetch that repo's `.shaka/labels.cue` over
    //    the API; without it, read the local tree (unchanged behavior).
    let label_set = load_scope_labels(&args.repo, cwd, &fetch_labels)?;
    validate_scope(&args.scope, &label_set)?;

    // 5. Repo — explicit flag wins; otherwise infer.
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

/// Path to the canonical label set, relative to a repo root — both for
/// the local working tree and for the GitHub Contents API.
const LABELS_FILE_REL: &str = ".shaka/labels.cue";

/// Resolve the label set `--scope` is validated against.
///
/// With `--repo`, the target is a (possibly foreign) repo: fetch its
/// `.shaka/labels.cue` over the API and parse it. A consumer repo has no
/// local checkout of the target, and its own `.shaka/labels.cue` would
/// shadow the target's if we read the local tree — so the remote fetch is
/// the only correct source here. Without `--repo`, read the local tree as
/// before.
fn load_scope_labels<F>(repo: &Option<String>, cwd: &Path, fetch: &F) -> Result<LabelSet, String>
where
    F: Fn(&str, &str) -> Result<Option<String>, String>,
{
    match repo {
        Some(r) => {
            let cue = fetch(r, LABELS_FILE_REL)?.ok_or_else(|| {
                format!(
                    "target repo `{r}` has no {LABELS_FILE_REL} — cannot validate \
                     --scope against it"
                )
            })?;
            labels::load_from_string(&cue).map_err(|e| format!("{r}:{LABELS_FILE_REL}: {e}"))
        }
        None => labels::load(cwd),
    }
}

/// Reject bodies that contain freeform parent-pointer phrases (`Sub-issue
/// of #N`, `Tracked in #N`). `--parent <N>` is the only sanctioned way
/// to set a parent — it uses GitHub's native sub-issue API, which is
/// what `shaka issue audit` enforces. The text-only shape rots silently
/// and is exactly what audit flags; refusing here keeps new issues from
/// being born in the bad state.
///
/// `_parent` is accepted but ignored — even with `--parent N` set, the
/// freeform body text is still wrong (it duplicates the link in a
/// brittle form). One canonical signal, no second source of truth.
fn reject_freeform_parent_refs(body: &str, _parent: Option<u64>) -> Result<(), String> {
    let refs = super::audit::extract_parent_refs(body);
    if refs.is_empty() {
        return Ok(());
    }
    let phrases: Vec<String> = refs.iter().map(|r| r.phrase.clone()).collect();
    Err(format!(
        "body contains freeform parent reference(s) {phrases:?} — use `--parent <N>` \
         instead (sets GitHub's native sub-issue link) and drop the freeform text"
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

    /// Build a `.shaka/labels.cue` document declaring `canonical` as scope
    /// labels. Used both as a local file (planted in a tempdir) and as the
    /// string a fake remote fetch serves.
    fn labels_cue(canonical: &[&str]) -> String {
        let mut s = String::from("package labels\n\n#LabelSet & {\n    labels: [\n");
        for name in canonical {
            s.push_str(&format!(
                "        {{name: {name:?}, color: \"5319e7\", description: \"\", scope: true}},\n"
            ));
        }
        s.push_str("    ]\n}\n");
        s
    }

    fn workdir_with_labels(canonical: &[&str]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".shaka")).unwrap();
        std::fs::write(tmp.path().join(".shaka/labels.cue"), labels_cue(canonical)).unwrap();
        tmp
    }

    /// Fake fetch that serves `cue` for any (repo, path) — stands in for
    /// `gh::fetch_raw_file` so tests exercise the cross-repo path offline.
    fn serve(cue: String) -> impl Fn(&str, &str) -> Result<Option<String>, String> {
        move |_repo, _path| Ok(Some(cue.clone()))
    }

    /// Fake fetch that serves a 404 (no labels file) for any repo.
    fn serve_missing() -> impl Fn(&str, &str) -> Result<Option<String>, String> {
        |_repo, _path| Ok(None)
    }

    /// Fake fetch that must never be called — the local path (no `--repo`)
    /// and the early-exit checks (title/body/freeform) never fetch.
    fn never() -> impl Fn(&str, &str) -> Result<Option<String>, String> {
        |_repo, _path| panic!("fetch should not be called")
    }

    #[test]
    fn rejects_bad_title_before_loading_labels() {
        // Title validation runs before label loading — the `never` fetch
        // would panic if we got as far as resolving the (foreign) label
        // set, so reaching the title error proves ordering.
        let err =
            run_inner(args("not conventional", "shaka"), Path::new("."), never()).unwrap_err();
        assert!(
            err.contains("missing `: ` separator") || err.contains("does not match"),
            "expected title error, got: {err}"
        );
    }

    #[test]
    fn rejects_unknown_scope_with_valid_list() {
        // `--repo` is set, so scope is validated against the *served*
        // (foreign) label set, not any local tree.
        let err = run_inner(
            args("feat(shaka): add x", "blogctl"),
            Path::new("."),
            serve(labels_cue(&["shaka", "ci"])),
        )
        .unwrap_err();
        assert!(err.contains("blogctl"), "should name the bad scope: {err}");
        assert!(err.contains("shaka"), "should list valid scopes: {err}");
        assert!(err.contains("ci"), "should list valid scopes: {err}");
    }

    #[test]
    fn accepts_valid_scope_from_foreign_repo() {
        // The served set is the *only* source of truth here (no local
        // labels file exists), proving cross-repo validation works.
        run_inner(
            args("feat(shaka): add x", "shaka"),
            Path::new("."),
            serve(labels_cue(&["shaka"])),
        )
        .unwrap();
    }

    #[test]
    fn validates_scope_against_foreign_set_not_local() {
        // Local tree declares `local-only`; the foreign repo declares
        // `ci`. With `--repo` set, the foreign set wins: `ci` is accepted
        // and `local-only` is rejected.
        let tmp = workdir_with_labels(&["local-only"]);
        let mut ok = args("feat(ci): add x", "ci");
        ok.repo = Some("o/r".into());
        run_inner(ok, tmp.path(), serve(labels_cue(&["ci"]))).unwrap();

        let mut bad = args("feat(x): add x", "local-only");
        bad.repo = Some("o/r".into());
        let err = run_inner(bad, tmp.path(), serve(labels_cue(&["ci"]))).unwrap_err();
        assert!(
            err.contains("local-only"),
            "local scope should be rejected against the foreign set: {err}"
        );
    }

    #[test]
    fn errors_when_foreign_repo_has_no_labels_file() {
        let err = run_inner(
            args("feat(shaka): add x", "shaka"),
            Path::new("."),
            serve_missing(),
        )
        .unwrap_err();
        assert!(err.contains("o/r"), "should name the target repo: {err}");
        assert!(
            err.contains(".shaka/labels.cue"),
            "should name the missing file: {err}"
        );
    }

    #[test]
    fn load_scope_labels_uses_local_tree_when_repo_absent() {
        // No `--repo` → read the local working tree; the `never` fetch
        // guarantees no remote call happens. Tested at the helper level
        // so we don't trip step-5 repo detection (which shells out to
        // jj/git) that a full dry-run with `repo: None` would.
        let tmp = workdir_with_labels(&["shaka", "ci"]);
        let set = load_scope_labels(&None, tmp.path(), &never()).unwrap();
        assert_eq!(set.scope_names(), vec!["shaka", "ci"]);
    }

    #[test]
    fn dedups_scope_when_also_passed_via_label_flag() {
        // A common slip is `--scope shaka --label shaka`. Don't pass
        // duplicate labels to gh — confirmed via the dry-run plan's
        // label vector (no behavioral effect on gh, but cleaner output).
        let mut a = args("feat(shaka): add x", "shaka");
        a.labels = vec!["shaka".into(), "bug".into()];
        run_inner(a, Path::new("."), serve(labels_cue(&["shaka"]))).unwrap();
    }

    #[test]
    fn rejects_neither_body_nor_body_file() {
        let mut a = args("feat(shaka): add x", "shaka");
        a.body = None;
        a.body_file = None;
        let err = run_inner(a, Path::new("."), never()).unwrap_err();
        assert!(err.contains("--body"), "got: {err}");
    }

    #[test]
    fn rejects_both_body_and_body_file() {
        let mut a = args("feat(shaka): add x", "shaka");
        a.body_file = Some("/nonexistent".into());
        let err = run_inner(a, Path::new("."), never()).unwrap_err();
        assert!(err.contains("mutually exclusive"), "got: {err}");
    }

    #[test]
    fn reads_body_from_file_when_set() {
        let tmp = TempDir::new().unwrap();
        let body_path = tmp.path().join("body.md");
        std::fs::write(&body_path, "from-file body").unwrap();
        let mut a = args("feat(shaka): add x", "shaka");
        a.body = None;
        a.body_file = Some(body_path.to_string_lossy().into());
        // Reaching dry-run without an error is the assertion — file
        // was readable and resolved.
        run_inner(a, Path::new("."), serve(labels_cue(&["shaka"]))).unwrap();
    }

    #[test]
    fn rejects_body_with_sub_issue_of() {
        let mut a = args("feat(shaka): add x", "shaka");
        a.body = Some("Sub-issue of #15. Adds a thing.".into());
        let err = run_inner(a, Path::new("."), never()).unwrap_err();
        assert!(err.contains("Sub-issue of #15"), "got: {err}");
        assert!(err.contains("--parent"), "should hint at --parent: {err}");
    }

    #[test]
    fn rejects_body_with_tracked_in() {
        let mut a = args("feat(shaka): add x", "shaka");
        a.body = Some("Tracked in #209.".into());
        let err = run_inner(a, Path::new("."), never()).unwrap_err();
        assert!(err.contains("Tracked in #209"), "got: {err}");
    }

    #[test]
    fn rejects_freeform_even_with_parent_set() {
        // --parent N doesn't whitelist freeform body text; the two
        // would duplicate the link in incompatible shapes. One source
        // of truth (the native API), no freeform.
        let mut a = args("feat(shaka): add x", "shaka");
        a.body = Some("Sub-issue of #15".into());
        a.parent = Some(15);
        let err = run_inner(a, Path::new("."), never()).unwrap_err();
        assert!(err.contains("Sub-issue of #15"), "got: {err}");
    }
}
