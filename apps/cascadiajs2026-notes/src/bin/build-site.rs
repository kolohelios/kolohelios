//! Static site generator for the CascadiaJS 2026 notes site.
//!
//! Exports `data/talks.cue` to JSON (the cue invocation also validates
//! the manifest), renders each talk's Markdown body under
//! `content/talks/<slug>.md` via `pulldown-cmark`, fills the askama
//! templates under `templates/`, invokes the system `tailwindcss` CLI
//! against the rendered HTML, and writes the result to `dist/`. The
//! resulting `dist/` is what Workers Static Assets uploads to
//! Cloudflare.
//!
//! Two modes:
//!
//!   * Default: write the build into `dist/`. Local iteration runs this
//!     after editing content/data/templates/styles and commits the
//!     result.
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
struct Project {
    name: String,
    url: String,
}

#[derive(Deserialize, Clone)]
struct Talk {
    slug: String,
    speaker: String,
    company: String,
    title: String,
    day: u32,
    order: u32,
    #[serde(default)]
    slides: Option<String>,
    #[serde(default)]
    projects: Vec<Project>,
    #[serde(default)]
    sources: Vec<String>,
}

#[derive(Clone)]
struct NavLink {
    slug: String,
    title: String,
}

struct TalkLink {
    slug: String,
    speaker: String,
    company: String,
    title: String,
}

struct DayGroup {
    day: u32,
    talks: Vec<TalkLink>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    days: Vec<DayGroup>,
}

#[derive(Template)]
#[template(path = "talk.html")]
struct TalkTemplate {
    title: String,
    speaker: String,
    company: String,
    day: u32,
    body: String,
    slides: Option<String>,
    projects: Vec<Project>,
    sources: Vec<String>,
    prev: Option<NavLink>,
    next: Option<NavLink>,
}

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
    let mut talks = load_talks()?;
    talks.sort_by_key(|t| (t.day, t.order));
    let pages = render_pages(&talks)?;

    let committed = PathBuf::from("dist");
    if check {
        let tmp = tempfile::TempDir::new().map_err(|e| format!("tempdir: {e}"))?;
        write_pages(tmp.path(), &pages)?;
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
        // Wipe dist/ before each build so renames in content/, data/,
        // or templates/ don't leave orphaned files behind that the
        // drift check would later catch.
        if committed.exists() {
            std::fs::remove_dir_all(&committed)
                .map_err(|e| format!("remove_dir_all dist/: {e}"))?;
        }
        std::fs::create_dir_all(&committed).map_err(|e| format!("create_dir_all dist/: {e}"))?;
        write_pages(&committed, &pages)?;
        run_tailwindcss(&committed)?;
        println!(
            "build-site: wrote {} page(s) + style.css to dist/",
            pages.len()
        );
    }
    Ok(())
}

/// Export `data/talks.cue` to JSON via the system `cue` binary (the
/// export also validates the manifest against its inline `#Talk`
/// constraint — a malformed entry fails here) and deserialize the
/// `talks` list.
fn load_talks() -> Result<Vec<Talk>, String> {
    let output = Command::new("cue")
        .args(["export", "data/talks.cue", "-e", "talks", "--out", "json"])
        .output()
        .map_err(|e| {
            format!("spawn cue: {e} (is it on PATH? enter the project's nix devshell first)")
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cue export data/talks.cue failed:\n{stderr}"));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("parse talks JSON: {e}"))
}

/// Render one talk's Markdown body (GFM tables/strikethrough/footnotes
/// enabled) to an HTML fragment.
fn md_to_html(md: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(md, opts);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// (relative path, rendered HTML). Each talk's path includes the
/// `index.html` suffix so the dist tree mirrors Workers Static Assets'
/// clean-URL expectations: `/talks/<slug>` → `dist/talks/<slug>/index.html`.
fn render_pages(talks: &[Talk]) -> Result<Vec<(PathBuf, String)>, String> {
    let mut out = Vec::new();

    // Index, grouped by day in sorted order.
    let mut days: Vec<DayGroup> = Vec::new();
    for t in talks {
        let link = TalkLink {
            slug: t.slug.clone(),
            speaker: t.speaker.clone(),
            company: t.company.clone(),
            title: t.title.clone(),
        };
        match days.last_mut() {
            Some(g) if g.day == t.day => g.talks.push(link),
            _ => days.push(DayGroup {
                day: t.day,
                talks: vec![link],
            }),
        }
    }
    out.push((
        PathBuf::from("index.html"),
        IndexTemplate { days }
            .render()
            .map_err(|e| format!("render index: {e}"))?,
    ));

    // One page per talk, with prev/next drawn from the sorted order.
    for (i, t) in talks.iter().enumerate() {
        let body_md = read_body(&t.slug)?;
        let prev = i
            .checked_sub(1)
            .and_then(|j| talks.get(j))
            .map(|p| NavLink {
                slug: p.slug.clone(),
                title: p.title.clone(),
            });
        let next = talks.get(i + 1).map(|n| NavLink {
            slug: n.slug.clone(),
            title: n.title.clone(),
        });
        let page = TalkTemplate {
            title: t.title.clone(),
            speaker: t.speaker.clone(),
            company: t.company.clone(),
            day: t.day,
            body: md_to_html(&body_md),
            slides: t.slides.clone(),
            projects: t.projects.clone(),
            sources: t.sources.clone(),
            prev,
            next,
        }
        .render()
        .map_err(|e| format!("render talk {}: {e}", t.slug))?;
        out.push((PathBuf::from(format!("talks/{}/index.html", t.slug)), page));
    }

    Ok(out)
}

fn read_body(slug: &str) -> Result<String, String> {
    let path = PathBuf::from(format!("content/talks/{slug}.md"));
    std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))
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

/// Walks both trees and returns the set of paths that differ. Compares
/// files by raw bytes — no normalization, so a generator change that
/// reflows whitespace shows up as drift and forces a regenerate.
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
