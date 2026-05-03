use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::stage::Stage;

/// Frontmatter metadata. Mirrors the YAML block at the top of every post
/// file. Fields stay required so a round-trip preserves shape; clients
/// patch them via accessors rather than partial deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostMetadata {
    pub title: String,
    pub slug: String,
    pub status: Stage,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub todoist_task_id: Option<String>,
    #[serde(default)]
    pub history_checked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Post {
    pub metadata: PostMetadata,
    pub body: String,
}

const DELIM: &str = "---";

impl Post {
    pub fn new(metadata: PostMetadata, body: impl Into<String>) -> Self {
        Self {
            metadata,
            body: body.into(),
        }
    }

    /// Parse a frontmatter-prefixed Markdown document. The file must open
    /// with a `---` line; the YAML block ends at the next `---` line. The
    /// remaining text is preserved verbatim as the body.
    pub fn parse(path: &Path, contents: &str) -> Result<Self> {
        let (yaml, body) = split_frontmatter(path, contents)?;
        let metadata: PostMetadata =
            serde_yaml_ng::from_str(yaml).map_err(|source| Error::FrontmatterParse {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(Self {
            metadata,
            body: body.to_string(),
        })
    }

    /// Render the post back to a frontmatter-prefixed Markdown string.
    pub fn render(&self) -> Result<String> {
        let yaml = serde_yaml_ng::to_string(&self.metadata).map_err(Error::FrontmatterSerialize)?;
        let mut out = String::with_capacity(yaml.len() + self.body.len() + 16);
        out.push_str(DELIM);
        out.push('\n');
        out.push_str(yaml.trim_end());
        out.push('\n');
        out.push_str(DELIM);
        out.push('\n');
        if !self.body.is_empty() {
            if !self.body.starts_with('\n') {
                out.push('\n');
            }
            out.push_str(&self.body);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        Ok(out)
    }
}

fn split_frontmatter<'a>(path: &Path, contents: &'a str) -> Result<(&'a str, &'a str)> {
    let after_opening = strip_opening_delim(contents)
        .ok_or_else(|| Error::FrontmatterMissingOpen(path.to_path_buf()))?;

    // Locate the closing `---` line. We look for `\n---` followed by either
    // end-of-input or another `\n` so a body line that happens to start with
    // `---` doesn't get confused with the delimiter.
    let mut search_from = 0;
    while let Some(pos) = after_opening[search_from..].find("\n---") {
        let abs = search_from + pos;
        let after_delim = abs + 4; // past the `\n---`
        let next = after_opening.as_bytes().get(after_delim).copied();
        let is_line = next.is_none() || next == Some(b'\n') || next == Some(b'\r');
        if is_line {
            let yaml = &after_opening[..abs];
            let body = if next == Some(b'\n') {
                &after_opening[after_delim + 1..]
            } else if next == Some(b'\r') {
                let after_cr = after_delim + 1;
                if after_opening.as_bytes().get(after_cr).copied() == Some(b'\n') {
                    &after_opening[after_cr + 1..]
                } else {
                    &after_opening[after_cr..]
                }
            } else {
                ""
            };
            return Ok((yaml, body));
        }
        search_from = abs + 1;
    }

    Err(Error::FrontmatterMissingClose(path.to_path_buf()))
}

fn strip_opening_delim(contents: &str) -> Option<&str> {
    if let Some(rest) = contents.strip_prefix("---\n") {
        Some(rest)
    } else if let Some(rest) = contents.strip_prefix("---\r\n") {
        Some(rest)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use time::macros::datetime;

    fn fixture_metadata() -> PostMetadata {
        PostMetadata {
            title: "Example Title".into(),
            slug: "example-title".into(),
            status: Stage::Concept,
            created_at: datetime!(2026-05-03 00:00:00 UTC),
            updated_at: datetime!(2026-05-03 00:00:00 UTC),
            tags: vec!["rust".into(), "tooling".into()],
            todoist_task_id: None,
            history_checked: false,
        }
    }

    fn parse(s: &str) -> Result<Post> {
        Post::parse(&PathBuf::from("test.md"), s)
    }

    #[test]
    fn parse_extracts_metadata_and_body() {
        let raw = r#"---
title: "Example Title"
slug: example-title
status: concept
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
todoist_task_id: null
history_checked: false
---

Draft text here.
"#;
        let post = parse(raw).unwrap();
        assert_eq!(post.metadata.title, "Example Title");
        assert_eq!(post.metadata.slug, "example-title");
        assert_eq!(post.metadata.status, Stage::Concept);
        assert!(post.metadata.tags.is_empty());
        assert!(post.metadata.todoist_task_id.is_none());
        assert!(!post.metadata.history_checked);
        assert_eq!(post.body, "\nDraft text here.\n");
    }

    #[test]
    fn parse_rejects_missing_opening_delim() {
        let raw = "title: Example\n---\n\nbody\n";
        assert!(matches!(parse(raw), Err(Error::FrontmatterMissingOpen(_))));
    }

    #[test]
    fn parse_rejects_missing_closing_delim() {
        let raw = "---\ntitle: Example\nslug: example\n";
        assert!(matches!(parse(raw), Err(Error::FrontmatterMissingClose(_))));
    }

    #[test]
    fn parse_rejects_invalid_yaml() {
        let raw = "---\nthis is: : not yaml\n---\n\nbody\n";
        assert!(matches!(parse(raw), Err(Error::FrontmatterParse { .. })));
    }

    #[test]
    fn parse_rejects_yaml_missing_required_fields() {
        let raw = "---\ntitle: Example\n---\n\nbody\n";
        assert!(matches!(parse(raw), Err(Error::FrontmatterParse { .. })));
    }

    #[test]
    fn render_round_trips_through_parse() {
        let original = Post::new(fixture_metadata(), "Hello body.\n");
        let rendered = original.render().unwrap();
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(reparsed.metadata, original.metadata);
        assert_eq!(reparsed.body.trim(), "Hello body.");
    }

    #[test]
    fn render_emits_well_known_rfc3339_timestamps() {
        let post = Post::new(fixture_metadata(), "");
        let rendered = post.render().unwrap();
        assert!(
            rendered.contains("created_at: 2026-05-03T00:00:00Z"),
            "got: {rendered}"
        );
    }

    #[test]
    fn render_handles_empty_body() {
        let post = Post::new(fixture_metadata(), "");
        let rendered = post.render().unwrap();
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(reparsed.body, "");
    }

    #[test]
    fn parse_tolerates_body_lines_starting_with_dashes() {
        let raw = "---\ntitle: T\nslug: t\nstatus: concept\ncreated_at: 2026-05-03T00:00:00Z\nupdated_at: 2026-05-03T00:00:00Z\ntags: []\n---\n\n--- not a delimiter ---\n";
        let post = parse(raw).unwrap();
        assert!(post.body.contains("--- not a delimiter ---"));
    }
}
