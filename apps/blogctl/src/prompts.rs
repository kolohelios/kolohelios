//! Prompt template rendering for the config-driven stage pipeline.
//!
//! Templates are minijinja files (Jinja-style `{{ var }}` and `{% if %}`),
//! resolved relative to the workdir root. Each render gets two top-level
//! variables: `body` (the post's Markdown body, verbatim) and
//! `frontmatter` (the full `PostMetadata` as a structured value).

use std::fs;
use std::path::Path;

use minijinja::{context, Environment, Value};

use crate::error::{Error, Result};
use crate::post::Post;

/// Render `template_path` against `post`. The path is taken verbatim —
/// callers resolve workdir-relative paths themselves before calling in.
pub fn render(template_path: &Path, post: &Post) -> Result<String> {
    let source = fs::read_to_string(template_path).map_err(|e| Error::io(template_path, e))?;
    render_str(template_path, &source, post)
}

/// Render an in-memory template. Useful for tests and for any future
/// command that wants to render a string the user typed on the CLI
/// rather than a file.
pub fn render_str(template_path: &Path, source: &str, post: &Post) -> Result<String> {
    let mut env = Environment::new();
    env.add_template("prompt", source)
        .map_err(|source| Error::PromptRender {
            path: template_path.to_path_buf(),
            source,
        })?;
    let tmpl = env.get_template("prompt").expect("template was just added");
    let frontmatter = Value::from_serialize(&post.metadata);
    tmpl.render(context! { body => &post.body, frontmatter => frontmatter })
        .map_err(|source| Error::PromptRender {
            path: template_path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use time::macros::datetime;

    use crate::classifications::Classifications;
    use crate::kind::Kind;
    use crate::post::{Post, PostMetadata};
    use crate::stage::Stage;

    fn fixture_post(body: &str) -> Post {
        Post::new(
            PostMetadata {
                title: "Example Title".into(),
                slug: "example".into(),
                kind: Kind::Post,
                theme: "standard".into(),
                status: Stage::Ideation,
                created_at: datetime!(2026-05-03 00:00:00 UTC),
                updated_at: datetime!(2026-05-03 00:00:00 UTC),
                tags: vec!["rust".into(), "tooling".into()],
                todoist_task_id: None,
                history_checked: false,
                targets: vec![],
                classifications: Classifications::default(),
            },
            body,
        )
    }

    #[test]
    fn renders_body_variable() {
        let post = fixture_post("seed notes");
        let path = PathBuf::from("inline.md");
        let out = render_str(&path, "Body: {{ body }}", &post).unwrap();
        assert_eq!(out, "Body: seed notes");
    }

    #[test]
    fn renders_frontmatter_scalar() {
        let post = fixture_post("");
        let out = render_str(
            &PathBuf::from("inline.md"),
            "Title: {{ frontmatter.title }}",
            &post,
        )
        .unwrap();
        assert_eq!(out, "Title: Example Title");
    }

    #[test]
    fn renders_frontmatter_array_field() {
        let post = fixture_post("");
        let out = render_str(
            &PathBuf::from("inline.md"),
            "{% for tag in frontmatter.tags %}- {{ tag }}\n{% endfor %}",
            &post,
        )
        .unwrap();
        assert_eq!(out, "- rust\n- tooling\n");
    }

    #[test]
    fn render_reads_template_from_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("ideation-post.md");
        fs::write(&path, "Topic: {{ frontmatter.title }}\n\n{{ body }}").unwrap();
        let post = fixture_post("seed line");
        let out = render(&path, &post).unwrap();
        assert_eq!(out, "Topic: Example Title\n\nseed line");
    }

    #[test]
    fn missing_template_file_surfaces_as_io_error() {
        let post = fixture_post("");
        let err = render(&PathBuf::from("/no/such/template.md"), &post).unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "got: {err:?}");
    }

    #[test]
    fn malformed_template_surfaces_as_prompt_render_error() {
        let post = fixture_post("");
        // Unclosed `{% if %}` block.
        let err = render_str(
            &PathBuf::from("inline.md"),
            "{% if frontmatter.title %}orphan",
            &post,
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::PromptRender { ref path, .. } if path == &PathBuf::from("inline.md")),
            "got: {err:?}"
        );
    }
}
