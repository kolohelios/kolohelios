use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::classifications::Classifications;
use crate::error::{Error, Result};
use crate::kind::Kind;
use crate::stage::Stage;
use crate::storage::DEFAULT_THEME;
use crate::target::{TargetEntry, TargetStatus};

/// Frontmatter metadata. Mirrors the YAML block at the top of every post
/// file. Fields stay required so a round-trip preserves shape; clients
/// patch them via accessors rather than partial deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostMetadata {
    pub title: String,
    pub slug: String,
    pub kind: Kind,
    /// Narrative theme. Validated against the workdir config's
    /// `[themes.*]` table at `blogctl new` time; defaults to
    /// `"standard"` on parse so posts predating this field still load.
    #[serde(default = "default_theme")]
    pub theme: String,
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
    /// Distribution targets. Orthogonal to `status` (editorial pipeline):
    /// records *where* this post has been pushed and the per-venue state.
    /// Default `[]` keeps existing files parsing unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<TargetEntry>,
    /// Structured tag dimensions (format, hook, tone, audience,
    /// strategic role, theme). Sits alongside the free-form `tags`
    /// list. Defaults to empty; skipped from serialization when no
    /// dimension has a value so posts predating the field stay
    /// quiet in their frontmatter.
    #[serde(default, skip_serializing_if = "Classifications::is_empty")]
    pub classifications: Classifications,
    /// AI-assisted operations audit trail. Populated by `blogctl
    /// draft` (and future `refine`/`final-edit` commands) so a future
    /// reader can see what prompt + model produced the current body.
    /// `None` for posts that haven't been touched by AI commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiHistory>,
}

/// Per-AI-command audit records. Each field is `Option` so a post that
/// went through `draft` but not `refine` doesn't carry an empty
/// `refine:` block in its frontmatter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AiHistory {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<AiDraftRecord>,
    /// Most-recent `blogctl refine` invocation. Overwrites on each
    /// run — multi-pass history (a `Vec<AiRefineRecord>`) is deferred
    /// per #506's scope. Earlier refines live in `jj` history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refine: Option<AiRefineRecord>,
    /// Most-recent `blogctl final-edit` invocation. Same single-slot
    /// shape as `refine` — earlier polish passes live in `jj` history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_edit: Option<AiFinalEditRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiDraftRecord {
    pub prompt: String,
    pub model: String,
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiRefineRecord {
    pub prompt: String,
    pub model: String,
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiFinalEditRecord {
    pub prompt: String,
    pub model: String,
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
}

fn default_theme() -> String {
    DEFAULT_THEME.to_string()
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
        validate_targets(path, &metadata.targets)?;
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

/// Enforce the cross-field invariants the type system can't carry:
/// each `Target` appears at most once, and a `Published` entry has
/// both `url` and `published_at`.
fn validate_targets(path: &Path, targets: &[TargetEntry]) -> Result<()> {
    let mut seen = HashSet::with_capacity(targets.len());
    for entry in targets {
        if !seen.insert(entry.name) {
            return Err(Error::DuplicateTarget {
                path: path.to_path_buf(),
                name: entry.name,
            });
        }
        if entry.status == TargetStatus::Published {
            if entry.url.is_none() {
                return Err(Error::PublishedTargetMissingUrl {
                    path: path.to_path_buf(),
                    name: entry.name,
                });
            }
            if entry.published_at.is_none() {
                return Err(Error::PublishedTargetMissingPublishedAt {
                    path: path.to_path_buf(),
                    name: entry.name,
                });
            }
        }
    }
    Ok(())
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
            kind: Kind::Post,
            theme: "standard".into(),
            status: Stage::Concept,
            created_at: datetime!(2026-05-03 00:00:00 UTC),
            updated_at: datetime!(2026-05-03 00:00:00 UTC),
            tags: vec!["rust".into(), "tooling".into()],
            todoist_task_id: None,
            history_checked: false,
            targets: vec![],
            classifications: Default::default(),
            ai: None,
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
kind: post
theme: parable
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
        assert_eq!(post.metadata.kind, Kind::Post);
        assert_eq!(post.metadata.theme, "parable");
        assert_eq!(post.metadata.status, Stage::Concept);
        assert!(post.metadata.tags.is_empty());
        assert!(post.metadata.todoist_task_id.is_none());
        assert!(!post.metadata.history_checked);
        assert_eq!(post.body, "\nDraft text here.\n");
    }

    #[test]
    fn parse_defaults_theme_to_standard_when_field_missing() {
        let raw = r#"---
title: "Pre-theme post"
slug: pre-theme-post
kind: post
status: concept
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
---

Body.
"#;
        let post = parse(raw).unwrap();
        assert_eq!(post.metadata.theme, "standard");
    }

    #[test]
    fn parse_round_trips_article_kind() {
        let raw = r#"---
title: "Long-form essay"
slug: long-form-essay
kind: article
status: ideation
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
---

Body.
"#;
        let post = parse(raw).unwrap();
        assert_eq!(post.metadata.kind, Kind::Article);
    }

    #[test]
    fn parse_rejects_unknown_kind() {
        let raw = r#"---
title: "X"
slug: x
kind: essay
status: concept
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
---

Body.
"#;
        assert!(matches!(parse(raw), Err(Error::FrontmatterParse { .. })));
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
        let raw = "---\ntitle: T\nslug: t\nkind: post\nstatus: concept\ncreated_at: 2026-05-03T00:00:00Z\nupdated_at: 2026-05-03T00:00:00Z\ntags: []\n---\n\n--- not a delimiter ---\n";
        let post = parse(raw).unwrap();
        assert!(post.body.contains("--- not a delimiter ---"));
    }

    #[test]
    fn parse_defaults_targets_to_empty_when_field_missing() {
        let raw = r#"---
title: "T"
slug: t
kind: post
status: concept
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
---

Body.
"#;
        let post = parse(raw).unwrap();
        assert!(post.metadata.targets.is_empty());
    }

    #[test]
    fn parse_accepts_post_with_targets() {
        let raw = r#"---
title: "T"
slug: t
kind: post
status: published
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
targets:
  - name: linkedin
    status: published
    url: https://www.linkedin.com/posts/example
    published_at: 2026-05-08T14:32:00Z
  - name: blog
    status: planned
---

Body.
"#;
        let post = parse(raw).unwrap();
        assert_eq!(post.metadata.targets.len(), 2);
        assert_eq!(
            post.metadata.targets[0].name,
            crate::target::Target::Linkedin
        );
        assert_eq!(
            post.metadata.targets[0].status,
            crate::target::TargetStatus::Published
        );
        assert_eq!(post.metadata.targets[1].name, crate::target::Target::Blog);
        assert_eq!(
            post.metadata.targets[1].status,
            crate::target::TargetStatus::Planned
        );
        assert!(post.metadata.targets[1].url.is_none());
    }

    #[test]
    fn parse_rejects_duplicate_targets() {
        let raw = r#"---
title: "T"
slug: t
kind: post
status: concept
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
targets:
  - name: blog
    status: planned
  - name: blog
    status: planned
---

Body.
"#;
        assert!(matches!(
            parse(raw),
            Err(Error::DuplicateTarget { name, .. }) if name == crate::target::Target::Blog
        ));
    }

    #[test]
    fn parse_rejects_published_target_missing_url() {
        let raw = r#"---
title: "T"
slug: t
kind: post
status: published
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
targets:
  - name: linkedin
    status: published
    published_at: 2026-05-08T14:32:00Z
---

Body.
"#;
        assert!(matches!(
            parse(raw),
            Err(Error::PublishedTargetMissingUrl { name, .. })
                if name == crate::target::Target::Linkedin
        ));
    }

    #[test]
    fn parse_rejects_published_target_missing_published_at() {
        let raw = r#"---
title: "T"
slug: t
kind: post
status: published
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
targets:
  - name: linkedin
    status: published
    url: https://www.linkedin.com/posts/example
---

Body.
"#;
        assert!(matches!(
            parse(raw),
            Err(Error::PublishedTargetMissingPublishedAt { name, .. })
                if name == crate::target::Target::Linkedin
        ));
    }

    #[test]
    fn render_round_trips_post_with_targets() {
        use crate::target::{Target, TargetEntry, TargetStatus};
        let mut metadata = fixture_metadata();
        metadata.targets = vec![
            TargetEntry {
                name: Target::Linkedin,
                status: TargetStatus::Published,
                url: Some("https://www.linkedin.com/posts/example".into()),
                published_at: Some(datetime!(2026-05-08 14:32:00 UTC)),
                metrics: None,
            },
            TargetEntry {
                name: Target::Blog,
                status: TargetStatus::Planned,
                url: None,
                published_at: None,
                metrics: None,
            },
        ];
        let original = Post::new(metadata, "Body.\n");
        let rendered = original.render().unwrap();
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(reparsed.metadata, original.metadata);
    }

    #[test]
    fn parse_classifications_from_frontmatter() {
        let raw = r#"---
title: "T"
slug: t
kind: post
theme: standard
status: concept
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
classifications:
  format: thesis
  hook: contradiction
  theme:
    - ambiguity
    - delivery
---

body
"#;
        let post = parse(raw).unwrap();
        assert_eq!(
            post.metadata.classifications.format.as_deref(),
            Some("thesis")
        );
        assert_eq!(
            post.metadata.classifications.hook.as_deref(),
            Some("contradiction")
        );
        assert_eq!(post.metadata.classifications.theme.len(), 2);
        // Dimensions not set in YAML stay at their defaults.
        assert!(post.metadata.classifications.tone.is_none());
        assert!(post.metadata.classifications.audience.is_none());
    }

    #[test]
    fn parse_omitted_classifications_defaults_to_empty() {
        // Pre-#433 frontmatter — no classifications: key at all.
        let raw = r#"---
title: "Pre-classifications"
slug: pre-classifications
kind: post
theme: standard
status: concept
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
---

body
"#;
        let post = parse(raw).unwrap();
        assert!(post.metadata.classifications.is_empty());
    }

    #[test]
    fn render_omits_empty_classifications_from_yaml() {
        let metadata = fixture_metadata();
        assert!(metadata.classifications.is_empty());
        let post = Post::new(metadata, "Body.\n");
        let rendered = post.render().unwrap();
        assert!(
            !rendered.contains("classifications"),
            "empty classifications must be skipped in output: {rendered}"
        );
    }

    #[test]
    fn render_round_trips_post_with_target_metrics() {
        use crate::target::{Target, TargetEntry, TargetMetrics, TargetStatus};
        let mut metadata = fixture_metadata();
        metadata.targets = vec![TargetEntry {
            name: Target::Linkedin,
            status: TargetStatus::Published,
            url: Some("https://www.linkedin.com/posts/example".into()),
            published_at: Some(datetime!(2026-05-08 14:32:00 UTC)),
            metrics: Some(TargetMetrics {
                impressions: 1842,
                reactions: 67,
                comments: 14,
                reposts: 5,
                sampled_at: datetime!(2026-05-14 00:00:00 UTC),
            }),
        }];
        let original = Post::new(metadata, "Body.\n");
        let rendered = original.render().unwrap();
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(reparsed.metadata, original.metadata);
    }

    #[test]
    fn render_round_trips_post_with_classifications() {
        let mut metadata = fixture_metadata();
        metadata.classifications = Classifications {
            format: Some("thesis".into()),
            hook: Some("contradiction".into()),
            tone: Some("sharp".into()),
            audience: Some("engineering".into()),
            theme: vec!["ambiguity".into(), "delivery".into()],
        };
        let original = Post::new(metadata, "Body.\n");
        let rendered = original.render().unwrap();
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(reparsed.metadata, original.metadata);
    }
}
