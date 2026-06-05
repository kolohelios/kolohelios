//! Canonical-profile generation.
//!
//! `tools/resume/profile.cue` is the single source for the profile prose
//! shared by the rendered résumé and the portfolio's about page (#785).
//! `shaka profile generate` renders it into two committed artifacts:
//!
//!   * `tools/resume/resume.md` — the profile sections (identity/contact,
//!     `## Summary`, `## Skills`, `## Education`) inside HTML-comment
//!     managed regions; the hand-authored work experience between them is
//!     left untouched.
//!   * `apps/kolohelios-portfolio/data/profile.json` — a flat export the
//!     portfolio's `build-site` reads (via serde, like `work-history.json`)
//!     to render `about.html`.
//!
//! `--check` regenerates both and exits non-zero on drift; it's wired into
//! `shaka preflight` as a repo-level check so a `profile.cue` edit that
//! isn't propagated fails CI regardless of which project a PR touches.

use std::process::Command;

use clap::Subcommand;
use serde::{Deserialize, Serialize};

use crate::term::{BOLD, GREEN, RED, RESET};

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// Render `tools/resume/profile.cue` into resume.md's managed regions
    /// and the portfolio's `data/profile.json`. `--check` verifies the
    /// committed outputs match and exits non-zero on drift.
    Generate {
        /// Verify committed outputs match what would be generated; exit non-zero on drift
        #[arg(long)]
        check: bool,
    },
}

pub fn run(cmd: ProfileCommand) {
    match cmd {
        ProfileCommand::Generate { check } => {
            if let Err(e) = generate(check) {
                eprintln!("{RED}{BOLD}error:{RESET} {e}");
                std::process::exit(1);
            }
        }
    }
}

// Repo-root-relative paths; preflight and the shaka wrapper both run from
// the repo root.
const SCHEMA: &str = "tools/resume/schema/profile.cue";
const DATA: &str = "tools/resume/profile.cue";
const RESUME_MD: &str = "tools/resume/resume.md";
const PROFILE_JSON: &str = "apps/kolohelios-portfolio/data/profile.json";

const PROFILE_BEGIN: &str = "<!-- BEGIN generated profile (shaka profile generate) -->";
const PROFILE_END: &str = "<!-- END generated profile -->";
const EDU_BEGIN: &str = "<!-- BEGIN generated education (shaka profile generate) -->";
const EDU_END: &str = "<!-- END generated education -->";

#[derive(Deserialize, Serialize)]
struct Profile {
    name: String,
    title: String,
    contact: Contact,
    summary: String,
    skills: Vec<SkillGroup>,
    education: Education,
}

#[derive(Deserialize, Serialize)]
struct Contact {
    phone: String,
    email: String,
    citizenship: String,
    location: String,
    linkedin: String,
    github: String,
}

#[derive(Deserialize, Serialize)]
struct SkillGroup {
    category: String,
    items: String,
}

#[derive(Deserialize, Serialize)]
struct Education {
    institution: String,
    degree: String,
    gpa: String,
    honors: String,
}

fn generate(check: bool) -> Result<(), String> {
    let profile = load_profile()?;
    let json = render_json(&profile)?;
    let resume = render_resume_md(&profile)?;

    if check {
        let mut drift = Vec::new();
        check_file(PROFILE_JSON, &json, &mut drift);
        check_file(RESUME_MD, &resume, &mut drift);
        if drift.is_empty() {
            println!("{GREEN}{BOLD}profile generate --check:{RESET} outputs match {DATA}");
            Ok(())
        } else {
            Err(format!(
                "profile outputs drifted from {DATA}:\n{}\nregenerate with: shaka profile generate",
                drift
                    .iter()
                    .map(|p| format!("  {p}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
        }
    } else {
        std::fs::write(PROFILE_JSON, &json).map_err(|e| format!("write {PROFILE_JSON}: {e}"))?;
        std::fs::write(RESUME_MD, &resume).map_err(|e| format!("write {RESUME_MD}: {e}"))?;
        println!("{GREEN}{BOLD}profile generate:{RESET} wrote {PROFILE_JSON} and {RESUME_MD}");
        Ok(())
    }
}

fn check_file(path: &str, expected: &str, drift: &mut Vec<String>) {
    match std::fs::read_to_string(path) {
        Ok(actual) if actual == expected => println!("  {GREEN}ok{RESET}      {path}"),
        Ok(_) => {
            println!("  {RED}DRIFT{RESET}   {path}");
            drift.push(path.to_string());
        }
        Err(_) => {
            println!("  {RED}MISSING{RESET} {path}");
            drift.push(path.to_string());
        }
    }
}

fn load_profile() -> Result<Profile, String> {
    let output = Command::new("cue")
        .args(["export", "-e", "profile", SCHEMA, DATA])
        .output()
        .map_err(|e| format!("spawn cue: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("cue export {DATA}: {stderr}"));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("parse cue export output: {e}"))
}

/// Pretty JSON the portfolio reads via serde, with a trailing newline so
/// the file passes shaka's whitespace check.
fn render_json(profile: &Profile) -> Result<String, String> {
    let mut s =
        serde_json::to_string_pretty(profile).map_err(|e| format!("serialize json: {e}"))?;
    s.push('\n');
    Ok(s)
}

/// Splice the generated profile + education blocks into the committed
/// resume.md, leaving everything outside the managed regions (frontmatter,
/// work experience, innovations) untouched.
fn render_resume_md(profile: &Profile) -> Result<String, String> {
    let current =
        std::fs::read_to_string(RESUME_MD).map_err(|e| format!("read {RESUME_MD}: {e}"))?;
    let with_profile = replace_region(
        &current,
        PROFILE_BEGIN,
        PROFILE_END,
        &render_profile_block(profile),
    )?;
    replace_region(
        &with_profile,
        EDU_BEGIN,
        EDU_END,
        &render_education_block(&profile.education),
    )
}

/// Replace the text between (exclusive) the `begin` and `end` marker lines
/// with `block`, surrounded by blank lines. Keeps the markers in place.
fn replace_region(text: &str, begin: &str, end: &str, block: &str) -> Result<String, String> {
    let bpos = text
        .find(begin)
        .ok_or_else(|| format!("missing marker `{begin}` in {RESUME_MD}"))?;
    let epos = text
        .find(end)
        .ok_or_else(|| format!("missing marker `{end}` in {RESUME_MD}"))?;
    let after_begin = bpos + begin.len();
    if epos < after_begin {
        return Err(format!("marker `{end}` precedes `{begin}` in {RESUME_MD}"));
    }
    let mut out = String::with_capacity(text.len() + block.len());
    out.push_str(&text[..after_begin]);
    out.push_str("\n\n");
    out.push_str(block);
    out.push_str("\n\n");
    out.push_str(&text[epos..]);
    Ok(out)
}

fn render_profile_block(p: &Profile) -> String {
    let c = &p.contact;
    let mut s = String::new();
    s.push_str(&format!("# {}\n\n", p.name));
    s.push_str(&format!("**{}**\n\n", p.title));
    s.push_str(&format!(
        "{} • {} • {} • {} • [LinkedIn]({}) • [GitHub]({})\n\n",
        c.phone, c.email, c.citizenship, c.location, c.linkedin, c.github
    ));
    s.push_str("## Summary\n\n");
    s.push_str(&format!("{}\n\n", p.summary));
    s.push_str("## Skills\n\n");
    for (i, g) in p.skills.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(&format!("- **{}:** {}", g.category, g.items));
    }
    s
}

fn render_education_block(e: &Education) -> String {
    format!(
        "## Education\n\n### {}\n\n{} — {} GPA / {}",
        e.institution, e.degree, e.gpa, e.honors
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Profile {
        Profile {
            name: "Ada L".to_string(),
            title: "Engineer".to_string(),
            contact: Contact {
                phone: "(555) 010-0000".to_string(),
                email: "a@example.com".to_string(),
                citizenship: "US Person".to_string(),
                location: "Town, ST".to_string(),
                linkedin: "https://example.com/in/a".to_string(),
                github: "https://github.com/a".to_string(),
            },
            summary: "Builds things.".to_string(),
            skills: vec![
                SkillGroup {
                    category: "Languages".to_string(),
                    items: "Rust, CUE".to_string(),
                },
                SkillGroup {
                    category: "Cloud".to_string(),
                    items: "AWS".to_string(),
                },
            ],
            education: Education {
                institution: "Some College".to_string(),
                degree: "A.A.S., General Studies".to_string(),
                gpa: "3.75".to_string(),
                honors: "Phi Theta Kappa".to_string(),
            },
        }
    }

    #[test]
    fn profile_block_renders_contact_summary_and_skill_bullets() {
        let s = render_profile_block(&sample());
        assert!(s.starts_with("# Ada L\n\n**Engineer**\n\n"));
        assert!(s.contains("(555) 010-0000 • a@example.com • US Person • Town, ST • [LinkedIn](https://example.com/in/a) • [GitHub](https://github.com/a)"));
        assert!(s.contains("## Summary\n\nBuilds things.\n\n## Skills\n\n"));
        assert!(s.contains("- **Languages:** Rust, CUE\n- **Cloud:** AWS"));
        // No trailing newline — replace_region owns the surrounding blanks.
        assert!(!s.ends_with('\n'));
    }

    #[test]
    fn education_block_uses_em_dash_and_gpa_honors() {
        assert_eq!(
            render_education_block(&sample().education),
            "## Education\n\n### Some College\n\nA.A.S., General Studies — 3.75 GPA / Phi Theta Kappa"
        );
    }

    #[test]
    fn replace_region_swaps_only_between_markers() {
        let text = "head\n<!--B-->\nold\nstuff\n<!--E-->\ntail\n";
        let out = replace_region(text, "<!--B-->", "<!--E-->", "NEW").unwrap();
        assert_eq!(out, "head\n<!--B-->\n\nNEW\n\n<!--E-->\ntail\n");
    }

    #[test]
    fn replace_region_errors_on_missing_marker() {
        assert!(replace_region("no markers here", "<!--B-->", "<!--E-->", "x").is_err());
    }
}
