//! Static site generator for the kolohelios portfolio.
//!
//! Reads `data/work-history.json` and `data/profile.json`, renders the
//! askama templates under `templates/`, invokes the system `tailwindcss`
//! CLI against the rendered HTML, and writes the resulting files to
//! `dist/`. The resulting `dist/` is what Workers Static Assets uploads
//! to Cloudflare.
//!
//! `data/profile.json` is generated from `tools/resume/profile.cue` by
//! `shaka profile generate` — the same canonical source the résumé's
//! profile sections render from — so the about page never hand-duplicates
//! profile prose (#785).
//!
//! The `/resume` page is rendered from `tools/resume/resume.md` (the
//! same source that project renders to the downloadable PDF/DOCX), and
//! those PDF/DOCX artifacts are copied into `dist/` for download — see
//! `copy_resume`.
//!
//! Two modes:
//!
//!   * Default: write the build into `dist/`. Local iteration runs this
//!     after editing templates/data/styles and commits the result.
//!   * `--check`: build into a temp directory, diff against the
//!     committed `dist/`, exit non-zero on drift. Wired into the
//!     generated `build-check` justfile recipe so `shaka preflight`
//!     catches drift in CI.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use askama::Template;
use pulldown_cmark::{html, Options, Parser};
use serde::Deserialize;

#[derive(Deserialize, Clone)]
struct WorkEntry {
    company: String,
    role: String,
    start_date: String,
    #[serde(default)]
    end_date: Option<String>,
    #[serde(default)]
    context: Option<String>,
    achievements: Vec<String>,
    #[serde(default)]
    technology: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct WorkHistory {
    entries: Vec<WorkEntry>,
}

/// Profile prose shared with the résumé, generated from
/// `tools/resume/profile.cue` into `data/profile.json` by
/// `shaka profile generate`. Only the fields the about page renders are
/// modelled; serde ignores the rest (name/title/contact, used by the
/// résumé). See #785.
#[derive(Deserialize)]
struct Profile {
    summary: String,
    skills: Vec<SkillGroup>,
    education: Education,
}

#[derive(Deserialize)]
struct SkillGroup {
    category: String,
    items: String,
}

#[derive(Deserialize)]
struct Education {
    institution: String,
    degree: String,
    gpa: String,
    honors: String,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate;

#[derive(Template)]
#[template(path = "about.html")]
struct AboutTemplate {
    summary: String,
    skills: Vec<SkillGroup>,
    education: Education,
}

#[derive(Template)]
#[template(path = "work.html")]
struct WorkTemplate {
    entries: Vec<WorkEntry>,
}

#[derive(Template)]
#[template(path = "projects.html")]
struct ProjectsTemplate;

#[derive(Template)]
#[template(path = "resume.html")]
struct ResumeTemplate {
    /// The résumé body, already rendered from `tools/resume/resume.md`
    /// to HTML. Emitted with the `safe` filter in the template since
    /// it's trusted markup we generated, not user input.
    body: String,
}

#[derive(Template)]
#[template(path = "contact.html")]
struct ContactTemplate;

fn main() {
    let check = std::env::args().any(|a| a == "--check");
    match run(check) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn run(check: bool) -> Result<(), String> {
    let history = load_work_history()?;
    let pages = render_pages(&history.entries)?;

    let committed = PathBuf::from("dist");
    if check {
        let tmp = tempfile::TempDir::new().map_err(|e| format!("tempdir: {e}"))?;
        write_pages(tmp.path(), &pages)?;
        copy_static_assets(tmp.path())?;
        run_tailwindcss(tmp.path())?;
        let drift = diff_dirs(tmp.path(), &committed)?;
        if !drift.is_empty() {
            eprintln!("dist/ drift detected against generated build:");
            for path in &drift {
                eprintln!("  {}", path.display());
            }
            eprintln!();
            eprintln!("regenerate with: cargo run --features build-site --bin build-site");
            return Err("drift".into());
        }
        println!("build-site --check: dist/ matches generated build");
    } else {
        // Wipe dist/ before each build so renames in templates/,
        // chart/, or data/ don't leave orphaned files behind that
        // the drift check would later catch.
        if committed.exists() {
            std::fs::remove_dir_all(&committed)
                .map_err(|e| format!("remove_dir_all dist/: {e}"))?;
        }
        std::fs::create_dir_all(&committed).map_err(|e| format!("create_dir_all dist/: {e}"))?;
        write_pages(&committed, &pages)?;
        copy_static_assets(&committed)?;
        run_tailwindcss(&committed)?;
        println!(
            "build-site: wrote {} page(s) + style.css + static assets to dist/",
            pages.len()
        );
    }
    Ok(())
}

/// Copy `chart/` (D3 vendor bundle + the timeline JS) and every
/// JSON under `data/` into the dist tree so the served paths
/// `/chart/*` and `/data/*` resolve, plus the rendered résumé
/// artifacts from `tools/resume`. Plain `cp -r` semantics — content
/// drives drift, not mtimes.
fn copy_static_assets(root: &Path) -> Result<(), String> {
    copy_tree(Path::new("chart"), &root.join("chart"))?;
    copy_tree(Path::new("data"), &root.join("data"))?;
    copy_resume(root)?;
    Ok(())
}

/// The sibling `tools/resume` project renders `resume.md` to committed,
/// reproducible `resume.{pdf,docx}` artifacts. We copy them into the
/// dist root so they're served at `/resume.pdf` and `/resume.docx`.
///
/// This is a cross-project source dependency: the path reaches out of
/// the project tree to `tools/resume`. A repo-level `shaka preflight`
/// check byte-compares those source artifacts against the copies
/// committed here, so a résumé re-render that isn't mirrored into this
/// project's `dist/` fails CI regardless of which project a PR touches.
const RESUME_SRC_DIR: &str = "../../tools/resume";
const RESUME_FILES: &[&str] = &["resume.pdf", "resume.docx"];

fn copy_resume(root: &Path) -> Result<(), String> {
    for name in RESUME_FILES {
        let from = Path::new(RESUME_SRC_DIR).join(name);
        let to = root.join(name);
        std::fs::copy(&from, &to)
            .map_err(|e| format!("copy {} -> {}: {e}", from.display(), to.display()))?;
    }
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.exists() {
        return Err(format!("source does not exist: {}", src.display()));
    }
    std::fs::create_dir_all(dst).map_err(|e| format!("create_dir_all {}: {e}", dst.display()))?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| format!("read_dir {}: {e}", src.display()))?
        .flatten()
    {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("copy {} -> {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn load_work_history() -> Result<WorkHistory, String> {
    let path = Path::new("data/work-history.json");
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Load the generated profile prose for the about page. `profile.json`
/// is produced from `tools/resume/profile.cue` by `shaka profile
/// generate` and committed; `shaka preflight` gates it against the
/// canonical CUE.
fn load_profile() -> Result<Profile, String> {
    let path = Path::new("data/profile.json");
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// (relative path, rendered HTML). Each entry's relative path
/// includes the `index.html` suffix so the dist tree mirrors
/// Workers Static Assets' clean-URL expectations:
/// `/about` → `dist/about/index.html`.
fn render_pages(entries: &[WorkEntry]) -> Result<Vec<(PathBuf, String)>, String> {
    let mut out = Vec::new();
    out.push((
        PathBuf::from("index.html"),
        IndexTemplate
            .render()
            .map_err(|e| format!("render index: {e}"))?,
    ));
    let profile = load_profile()?;
    out.push((
        PathBuf::from("about/index.html"),
        AboutTemplate {
            summary: profile.summary,
            skills: profile.skills,
            education: profile.education,
        }
        .render()
        .map_err(|e| format!("render about: {e}"))?,
    ));
    out.push((
        PathBuf::from("work/index.html"),
        WorkTemplate {
            entries: entries.to_vec(),
        }
        .render()
        .map_err(|e| format!("render work: {e}"))?,
    ));
    out.push((
        PathBuf::from("projects/index.html"),
        ProjectsTemplate
            .render()
            .map_err(|e| format!("render projects: {e}"))?,
    ));
    out.push((
        PathBuf::from("resume/index.html"),
        ResumeTemplate {
            body: load_resume_html()?,
        }
        .render()
        .map_err(|e| format!("render resume: {e}"))?,
    ));
    out.push((
        PathBuf::from("contact/index.html"),
        ContactTemplate
            .render()
            .map_err(|e| format!("render contact: {e}"))?,
    ));
    Ok(out)
}

/// Read `tools/resume/resume.md`, strip the leading pandoc YAML
/// frontmatter (LaTeX/geometry directives that only apply to the
/// PDF/DOCX render), and convert the markdown body to HTML. This is
/// the same source `tools/resume` renders to the downloadable
/// PDF/DOCX, so the page never duplicates résumé content by hand.
fn load_resume_html() -> Result<String, String> {
    let path = Path::new(RESUME_SRC_DIR).join("resume.md");
    let md = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(render_markdown(strip_frontmatter(&md)))
}

/// Strip a leading `---`-delimited YAML frontmatter block, if present.
/// Returns the input unchanged when there's no frontmatter.
fn strip_frontmatter(md: &str) -> &str {
    md.strip_prefix("---\n")
        .and_then(|rest| rest.find("\n---\n").map(|end| &rest[end + 5..]))
        .unwrap_or(md)
}

fn render_markdown(md: &str) -> String {
    let parser = Parser::new_ext(md, Options::empty());
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

fn write_pages(root: &Path, pages: &[(PathBuf, String)]) -> Result<(), String> {
    for (rel, html) in pages {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create_dir_all {}: {e}", parent.display()))?;
        }
        let cleaned = normalize_whitespace(html);
        std::fs::write(&path, cleaned).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Trim trailing whitespace from every line and end with exactly one
/// newline. Keeps shaka's repo-wide whitespace check happy without
/// having to sprinkle askama `{%-` `-%}` directives through every
/// control-flow tag (which obscure templates for marginal value).
fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.lines() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    // s.lines() drops a trailing empty line, so we already end with
    // a single '\n'. If the input was empty, force a single newline
    // so the file isn't zero-byte.
    if out.is_empty() {
        out.push('\n');
    }
    out
}

/// Shells out to the system `tailwindcss` binary (provided by the
/// project's nix devshell). Scans every `.html` under `root` for
/// Tailwind class names and writes a minimized `style.css` next to
/// them, then normalizes whitespace so the file passes shaka's
/// repo-wide checks (`--minify` strips the trailing newline).
fn run_tailwindcss(root: &Path) -> Result<(), String> {
    let css_path = root.join("style.css");
    let output = Command::new("tailwindcss")
        .arg("-i")
        .arg("styles/input.css")
        .arg("-o")
        .arg(&css_path)
        .arg("--content")
        .arg(format!("{}/**/*.html", root.display()))
        .arg("--minify")
        .output()
        .map_err(|e| {
            format!(
                "spawn tailwindcss: {e} (is it on PATH? enter the project's nix devshell first)"
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tailwindcss failed:\n{stderr}"));
    }
    let css = std::fs::read_to_string(&css_path)
        .map_err(|e| format!("read {}: {e}", css_path.display()))?;
    std::fs::write(&css_path, normalize_whitespace(&css))
        .map_err(|e| format!("write {}: {e}", css_path.display()))?;
    Ok(())
}

/// Walks both trees and returns the set of paths that differ
/// (missing on either side, or differing contents). Compares files
/// by raw bytes — no normalization, no whitespace tolerance, so a
/// generator change that reflows whitespace shows up as drift and
/// forces a regenerate-and-commit.
fn diff_dirs(generated: &Path, committed: &Path) -> Result<Vec<PathBuf>, String> {
    let mut left = collect_files(generated, generated)?;
    let mut right = collect_files(committed, committed)?;
    let mut drift = Vec::new();

    let keys: std::collections::BTreeSet<_> = left.keys().chain(right.keys()).cloned().collect();
    for key in keys {
        match (left.remove(&key), right.remove(&key)) {
            (Some(l), Some(r)) if l == r => {}
            _ => drift.push(key),
        }
    }
    Ok(drift)
}

fn collect_files(root: &Path, cursor: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let mut out = BTreeMap::new();
    if !cursor.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(cursor)
        .map_err(|e| format!("read_dir {}: {e}", cursor.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            let sub = collect_files(root, &path)?;
            out.extend(sub);
        } else {
            let bytes =
                std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
            let rel = path
                .strip_prefix(root)
                .map_err(|_| format!("strip_prefix {} from {}", root.display(), path.display()))?
                .to_path_buf();
            out.insert(rel, bytes);
        }
    }
    Ok(out)
}
