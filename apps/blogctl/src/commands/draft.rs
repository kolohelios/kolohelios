//! `blogctl draft <slug> --prompt <text>` — generate the post body via
//! OpenRouter, seeded by the post's title + existing body + a single
//! user prompt.
//!
//! Operates on `Ideation`-stage posts only. Other stages error; the
//! intent is that `draft` produces the "first real body" pass, while
//! later passes go through `refine` / `final-edit` (tracked in #483).
//!
//! Writes the model's reply directly into the post body (replacing
//! existing content) and records an audit trail in `metadata.ai.draft`
//! so a future reader can see what prompt + model produced this output.

use std::fs;
use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::openrouter;
use crate::post::{AiDraftRecord, AiHistory};
use crate::stage::Stage;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};

#[derive(Debug)]
pub struct DraftArgs {
    pub slug: String,
    pub workdir: PathBuf,
    pub prompt: String,
    pub model: Option<String>,
    pub no_sync: bool,
}

pub fn run(jj: &dyn Jj, args: DraftArgs) -> Result<()> {
    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let (handle, mut post) = repo.load_raw(&args.slug)?;

    if handle.stage != Stage::Ideation {
        return Err(Error::DraftWrongStage {
            slug: args.slug,
            stage: handle.stage,
        });
    }

    let config = repo.read_config()?;
    let model = openrouter::resolve_model(args.model.as_deref(), config.defaults.model.as_deref());

    let llm_prompt = build_prompt(&post.metadata.title, &post.body, &args.prompt);
    let reply = openrouter::chat(&llm_prompt, &model)?;

    post.body = reply;
    post.metadata.ai = Some(merge_draft_record(
        post.metadata.ai.take(),
        AiDraftRecord {
            prompt: args.prompt.clone(),
            model: model.clone(),
            generated_at: OffsetDateTime::now_utc(),
        },
    ));

    let opts = SyncOptions::from_config(&config.sync, args.no_sync);
    let message = format!("post({}): draft via {}", args.slug, model);
    let path = handle.path.clone();
    let slug = args.slug.clone();

    sync::commit_and_push(jj, &args.workdir, &opts, &message, || {
        post.metadata.updated_at = OffsetDateTime::now_utc();
        let rendered = post.render()?;
        fs::write(&path, rendered).map_err(|e| Error::io(&path, e))?;
        Ok(())
    })?;
    println!("{slug}: drafted via {model}");
    Ok(())
}

/// Compose the LLM prompt: small preamble giving the model role +
/// markdown expectation, then the post's title and existing body as
/// context, then the user's instruction. Kept as a separate fn so the
/// shape is unit-testable without spinning up a mock server.
fn build_prompt(title: &str, existing_body: &str, user_prompt: &str) -> String {
    let mut s = String::with_capacity(
        // Rough size estimate; the +256 covers the boilerplate.
        title.len() + existing_body.len() + user_prompt.len() + 256,
    );
    s.push_str(
        "You are drafting a blog post. \
         Produce the post body in Markdown. \
         Do not include a title, frontmatter, or commentary about your output — \
         only the body text the reader will see.\n\n",
    );
    s.push_str("Title: ");
    s.push_str(title);
    s.push_str("\n\n");
    if !existing_body.trim().is_empty() {
        s.push_str("Existing seed/notes:\n");
        s.push_str(existing_body.trim());
        s.push_str("\n\n");
    }
    s.push_str("Author instruction: ");
    s.push_str(user_prompt);
    s
}

/// Fold a new `AiDraftRecord` into the post's optional `AiHistory`,
/// preserving any other AI-history fields a future `refine`/
/// `final-edit` command might have written.
fn merge_draft_record(existing: Option<AiHistory>, new_draft: AiDraftRecord) -> AiHistory {
    let mut history = existing.unwrap_or_default();
    history.draft = Some(new_draft);
    history
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_title_and_user_instruction() {
        let p = build_prompt("The Title", "", "write three paragraphs");
        assert!(p.contains("Title: The Title"));
        assert!(p.contains("Author instruction: write three paragraphs"));
    }

    #[test]
    fn prompt_includes_existing_body_when_non_empty() {
        let p = build_prompt("T", "some seed notes", "expand");
        assert!(p.contains("Existing seed/notes:"));
        assert!(p.contains("some seed notes"));
    }

    #[test]
    fn prompt_skips_seed_block_when_body_is_empty() {
        let p = build_prompt("T", "", "go");
        assert!(!p.contains("Existing seed/notes:"));
    }

    #[test]
    fn prompt_skips_seed_block_when_body_is_whitespace() {
        // Trimmed-empty body should be treated as no seed; emitting
        // an "Existing seed/notes:\n\n" block confuses the model
        // about whether there's hidden content.
        let p = build_prompt("T", "   \n  \t\n", "go");
        assert!(!p.contains("Existing seed/notes:"));
    }

    #[test]
    fn prompt_specifies_markdown_and_excludes_frontmatter() {
        let p = build_prompt("T", "", "go");
        assert!(p.to_lowercase().contains("markdown"));
        assert!(p.to_lowercase().contains("frontmatter"));
    }

    #[test]
    fn merge_creates_history_when_none_existed() {
        let h = merge_draft_record(
            None,
            AiDraftRecord {
                prompt: "p".into(),
                model: "m".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            },
        );
        assert!(h.draft.is_some());
    }

    #[test]
    fn merge_overwrites_prior_draft_record() {
        let prior = AiHistory {
            draft: Some(AiDraftRecord {
                prompt: "old".into(),
                model: "old-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            }),
            refine: None,
            final_edit: None,
        };
        let h = merge_draft_record(
            Some(prior),
            AiDraftRecord {
                prompt: "new".into(),
                model: "new-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            },
        );
        assert_eq!(h.draft.unwrap().prompt, "new");
    }
}
