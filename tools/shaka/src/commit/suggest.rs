//! `shaka commit suggest` — draft a conventional-commit message from a
//! diff's changed paths. Deterministic heuristics only: no LLM, no
//! network beyond a best-effort `gh issue view` for the fallback.
//!
//! Three components, each resolved independently with path heuristics
//! primary and the tied issue's title (already conventional-commit
//! shaped, since `issue create` enforces it) as the fallback:
//!
//! - **type** — unambiguous path categories win (`docs`-only → `docs`,
//!   tests-only → `test`, workflows-only → `ci`). A mixed/source diff
//!   can't be told apart (`feat` vs `fix` vs `refactor`), so it falls
//!   back to the issue's type, then to `feat`.
//! - **scope** — the single project the diff touches (`tools/shaka` →
//!   `shaka`, `infra/home` → `infra/home`), reusing `commit lint`'s
//!   `project_of`. Root-only or cross-project diffs fall back to the
//!   issue's scope; cross-project also warns, per the atomicity rule.
//! - **subject** — left as a placeholder unless the tied issue supplies
//!   one (no LLM here, by design).
//!
//! The result is emitted as a ready-to-paste message (and, with
//! `--apply`, written to the working-copy description when it's empty).

use std::collections::BTreeSet;

use serde::Serialize;

use super::title;
use super::{gather_tied_issues, jj_files, project_of, BODY_LINE_MAX, TITLE_MAX};
use crate::gh;
use crate::jj;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

const SUBJECT_PLACEHOLDER: &str = "<TODO: subject>";
const BODY_PLACEHOLDER: &str =
    "<TODO: why — explain the motivation behind this change, not the mechanics>";

/// Where a drafted component came from. Surfaced in `--json` and the
/// human note so the draft never looks more certain than it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Source {
    /// Inferred from the changed paths.
    Path,
    /// Carried over from the tied issue's title.
    Issue,
    /// Neither heuristic resolved it; a default/placeholder was used.
    Default,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::Path => "path",
            Source::Issue => "issue",
            Source::Default => "default",
        }
    }
}

/// The drafted message plus the provenance of each component. Serialized
/// verbatim for `--json`.
#[derive(Debug, Serialize)]
struct Draft {
    #[serde(rename = "type")]
    ty: String,
    type_source: Source,
    scope: Option<String>,
    scope_source: Source,
    subject: Option<String>,
    subject_source: Source,
    /// The tied issue, if any, that fed fallbacks and `Closes #N`.
    issue: Option<u64>,
    title: String,
    body: String,
    message: String,
    warnings: Vec<String>,
}

/// Conventional-commit components extracted from a tied issue's title.
struct IssueParts {
    ty: String,
    scope: Option<String>,
    subject: String,
}

pub fn run(revset: &str, json: bool, apply: bool) {
    let draft = match build_draft(revset) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    crate::output::emit(json, &draft, print_human);

    if apply {
        apply_draft(&draft.message);
    }
}

fn build_draft(revset: &str) -> Result<Draft, String> {
    let paths = jj_files(revset)?;
    let mut warnings = Vec::new();
    if paths.is_empty() {
        warnings.push(format!(
            "no changes in revset {revset:?} — draft is a bare template"
        ));
    }

    // Best-effort tied-issue lookup. A missing link or a `gh` failure
    // simply leaves the fallbacks unfilled — never an error.
    let issue = gather_tied_issues(revset).into_iter().next();
    let issue_parts = issue.and_then(issue_parts);

    let (ty, type_source) = infer_type(&paths, issue_parts.as_ref());
    let (scope, scope_source) = infer_scope(&paths, issue_parts.as_ref(), &mut warnings);
    let (subject, subject_source) = match issue_parts.as_ref() {
        Some(p) => (Some(p.subject.clone()), Source::Issue),
        None => (None, Source::Default),
    };

    let title = render_title(&ty, scope.as_deref(), subject.as_deref());
    let title_len = title.chars().count();
    if title_len > TITLE_MAX {
        warnings.push(format!(
            "drafted title is {title_len} chars (max {TITLE_MAX}) — shorten the subject"
        ));
    }
    let body = render_body(issue);
    let message = format!("{title}\n\n{body}");

    Ok(Draft {
        ty,
        type_source,
        scope,
        scope_source,
        subject,
        subject_source,
        issue,
        title,
        body,
        message,
        warnings,
    })
}

/// Fetch and parse a tied issue's title into commit components. Returns
/// `None` on any failure (no such issue, `gh` unavailable, or a title
/// that somehow isn't conventional-commit shaped).
fn issue_parts(n: u64) -> Option<IssueParts> {
    let title = gh::issue_title(n).ok()?;
    let parsed = title::parse(&title).ok()?;
    Some(IssueParts {
        ty: parsed.ty.to_string(),
        scope: parsed.scope.map(str::to_string),
        subject: parsed.subject.to_string(),
    })
}

/// Resolve the commit type. Unambiguous path categories win; otherwise
/// fall back to the issue's type, then to `feat`.
fn infer_type(paths: &[String], issue: Option<&IssueParts>) -> (String, Source) {
    if let Some(ty) = type_from_paths(paths) {
        return (ty.to_string(), Source::Path);
    }
    if let Some(p) = issue {
        return (p.ty.clone(), Source::Issue);
    }
    ("feat".to_string(), Source::Default)
}

/// The unambiguous path-only type categories. Returns `None` for an
/// empty diff or any mix that can't be classified from paths alone
/// (including a source change, which could be `feat`/`fix`/`refactor`).
fn type_from_paths(paths: &[String]) -> Option<&'static str> {
    if paths.is_empty() {
        return None;
    }
    if paths.iter().all(|p| is_ci(p)) {
        return Some("ci");
    }
    if paths.iter().all(|p| is_test(p)) {
        return Some("test");
    }
    if paths.iter().all(|p| is_docs(p)) {
        return Some("docs");
    }
    None
}

fn is_ci(path: &str) -> bool {
    path.starts_with(".github/")
}

fn is_test(path: &str) -> bool {
    path.contains("/tests/") || path.ends_with("_test.rs")
}

fn is_docs(path: &str) -> bool {
    path.ends_with(".md") || path.starts_with("docs/") || path.contains("/docs/")
}

/// Resolve the scope from the single project the diff touches. Pushes a
/// cross-project warning when the diff spans more than one. Falls back
/// to the issue's scope when paths don't pin a single project.
fn infer_scope(
    paths: &[String],
    issue: Option<&IssueParts>,
    warnings: &mut Vec<String>,
) -> (Option<String>, Source) {
    let projects: BTreeSet<String> = paths.iter().filter_map(|p| project_of(p)).collect();

    if projects.len() > 1 {
        let names: Vec<String> = projects.iter().cloned().collect();
        warnings.push(format!(
            "diff spans multiple projects ({}); pick one scope or split the commit",
            names.join(", ")
        ));
    } else if let Some(slot_project) = projects.iter().next() {
        return (Some(scope_of(slot_project)), Source::Path);
    }

    // Root-only or cross-project: defer to the issue's scope.
    if let Some(p) = issue {
        if let Some(scope) = &p.scope {
            return (Some(scope.clone()), Source::Issue);
        }
    }
    (None, Source::Default)
}

/// Map a `slot/project` pair to its commit scope. `infra` keeps the
/// `infra/<name>` form (matching repo history); every other slot uses
/// the bare project name (`tools/shaka` → `shaka`).
fn scope_of(slot_project: &str) -> String {
    match slot_project.split_once('/') {
        Some(("infra", _)) => slot_project.to_string(),
        Some((_, project)) => project.to_string(),
        None => slot_project.to_string(),
    }
}

fn render_title(ty: &str, scope: Option<&str>, subject: Option<&str>) -> String {
    let scope_part = scope.map(|s| format!("({s})")).unwrap_or_default();
    let subject = subject.unwrap_or(SUBJECT_PLACEHOLDER);
    format!("{ty}{scope_part}: {subject}")
}

/// The body: a "why not what" placeholder, plus a `Closes #N` trailer
/// when the branch is tied to an issue (so the draft already satisfies
/// `commit lint`'s issue-link rule). Every line stays within the body
/// width budget.
fn render_body(issue: Option<u64>) -> String {
    debug_assert!(BODY_PLACEHOLDER.chars().count() <= BODY_LINE_MAX);
    match issue {
        Some(n) => format!("{BODY_PLACEHOLDER}\n\nCloses #{n}"),
        None => BODY_PLACEHOLDER.to_string(),
    }
}

fn print_human(draft: &Draft) {
    for w in &draft.warnings {
        eprintln!("{YELLOW}{BOLD}warn{RESET} {w}");
    }
    println!("{}", draft.message);
    let scope_note = draft.scope.as_deref().unwrap_or("(none)");
    eprintln!(
        "{DIM}draft: type={} ({}) · scope={} ({}) · subject ({}){RESET}",
        draft.ty,
        draft.type_source.label(),
        scope_note,
        draft.scope_source.label(),
        draft.subject_source.label(),
    );
}

/// Apply the draft to `@`'s description, but only when it's currently
/// empty — refusing to clobber in-progress work is the safe default.
fn apply_draft(message: &str) {
    let current = match jj::current_description() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} reading current description: {e}");
            std::process::exit(1);
        }
    };
    let trimmed = current.trim();
    if !trimmed.is_empty() && trimmed != "(no description set)" {
        eprintln!(
            "{YELLOW}{BOLD}warn{RESET} working copy already has a description; \
             not overwriting. Copy the draft above manually if you meant to replace it."
        );
        return;
    }
    match jj::describe(message) {
        Ok(()) => eprintln!("{GREEN}{BOLD}applied{RESET} draft to @ description"),
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} applying description: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(ty: &str, scope: Option<&str>, subject: &str) -> IssueParts {
        IssueParts {
            ty: ty.to_string(),
            scope: scope.map(str::to_string),
            subject: subject.to_string(),
        }
    }

    fn paths(ps: &[&str]) -> Vec<String> {
        ps.iter().map(|s| s.to_string()).collect()
    }

    // -- type inference ------------------------------------------------

    #[test]
    fn type_from_workflows_only_is_ci() {
        assert_eq!(
            type_from_paths(&paths(&[".github/workflows/main.yaml"])),
            Some("ci")
        );
    }

    #[test]
    fn type_from_tests_only_is_test() {
        assert_eq!(
            type_from_paths(&paths(&[
                "tools/shaka/tests/schema.rs",
                "tools/shaka/src/foo_test.rs",
            ])),
            Some("test")
        );
    }

    #[test]
    fn type_from_docs_only_is_docs() {
        assert_eq!(
            type_from_paths(&paths(&["README.md", "docs/intro.md"])),
            Some("docs")
        );
    }

    #[test]
    fn type_from_source_change_is_ambiguous() {
        // feat/fix/refactor can't be told apart from paths.
        assert_eq!(
            type_from_paths(&paths(&["tools/shaka/src/commit.rs"])),
            None
        );
    }

    #[test]
    fn type_from_mixed_categories_is_ambiguous() {
        assert_eq!(
            type_from_paths(&paths(&["README.md", "tools/shaka/tests/x.rs"])),
            None
        );
    }

    #[test]
    fn type_falls_back_to_issue_then_default() {
        let src = paths(&["tools/shaka/src/commit.rs"]);
        let issue = parts("fix", Some("shaka"), "thing");
        assert_eq!(
            infer_type(&src, Some(&issue)),
            ("fix".into(), Source::Issue)
        );
        assert_eq!(infer_type(&src, None), ("feat".into(), Source::Default));
    }

    #[test]
    fn type_path_category_beats_issue() {
        // Even tied to a `feat` issue, a docs-only diff is `docs`.
        let issue = parts("feat", Some("shaka"), "thing");
        assert_eq!(
            infer_type(&paths(&["README.md"]), Some(&issue)),
            ("docs".into(), Source::Path)
        );
    }

    // -- scope inference -----------------------------------------------

    #[test]
    fn scope_of_strips_slot_except_infra() {
        assert_eq!(scope_of("tools/shaka"), "shaka");
        assert_eq!(scope_of("apps/blogctl"), "blogctl");
        assert_eq!(scope_of("infra/home"), "infra/home");
    }

    #[test]
    fn scope_from_single_project() {
        let mut warnings = Vec::new();
        let (scope, source) =
            infer_scope(&paths(&["tools/shaka/src/commit.rs"]), None, &mut warnings);
        assert_eq!(scope, Some("shaka".into()));
        assert_eq!(source, Source::Path);
        assert!(warnings.is_empty());
    }

    #[test]
    fn scope_cross_project_warns_and_falls_back_to_issue() {
        let mut warnings = Vec::new();
        let issue = parts("feat", Some("shaka"), "thing");
        let (scope, source) = infer_scope(
            &paths(&["tools/shaka/src/main.rs", "infra/home/flake.nix"]),
            Some(&issue),
            &mut warnings,
        );
        assert_eq!(scope, Some("shaka".into()));
        assert_eq!(source, Source::Issue);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("multiple projects"));
    }

    #[test]
    fn scope_root_only_falls_back_to_issue() {
        let mut warnings = Vec::new();
        let issue = parts("ci", Some("ci"), "thing");
        let (scope, source) = infer_scope(&paths(&["flake.nix"]), Some(&issue), &mut warnings);
        assert_eq!(scope, Some("ci".into()));
        assert_eq!(source, Source::Issue);
        assert!(warnings.is_empty());
    }

    #[test]
    fn scope_none_when_no_project_and_no_issue() {
        let mut warnings = Vec::new();
        let (scope, source) = infer_scope(&paths(&["flake.nix"]), None, &mut warnings);
        assert_eq!(scope, None);
        assert_eq!(source, Source::Default);
    }

    // -- rendering -----------------------------------------------------

    #[test]
    fn render_title_with_scope_and_subject() {
        assert_eq!(
            render_title("feat", Some("shaka"), Some("add suggest")),
            "feat(shaka): add suggest"
        );
    }

    #[test]
    fn render_title_uses_placeholders() {
        assert_eq!(
            render_title("feat", None, None),
            format!("feat: {SUBJECT_PLACEHOLDER}")
        );
    }

    #[test]
    fn render_body_appends_closes_when_tied() {
        let body = render_body(Some(69));
        assert!(body.starts_with(BODY_PLACEHOLDER));
        assert!(body.ends_with("Closes #69"));
    }

    #[test]
    fn render_body_omits_closes_when_untied() {
        assert_eq!(render_body(None), BODY_PLACEHOLDER);
    }
}
