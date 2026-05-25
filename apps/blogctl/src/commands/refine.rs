//! `blogctl refine <slug> --prompt <text>` — iterate on a post body via
//! OpenRouter, seeded by the post's title + current body + a single
//! user instruction.
//!
//! Operates on `Editing`-stage posts only. Other stages error; the
//! intent is that `refine` iterates on the draft that `draft` produced.
//! Polish-pass belongs to a future `final-edit` command (#483).
//!
//! Replaces the post body with the model's reply and records an audit
//! trail in `metadata.ai.refine`. Prior `ai.draft` (and any future
//! `ai.<other>`) records are preserved.
//!
//! Only the most-recent `refine` is stored; earlier passes live in `jj`
//! history. Promoting `ai.refine` to a `Vec<AiRefineRecord>` is
//! deliberately deferred until the audit-trail use case demands it.

use std::fs;
use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::openrouter;
use crate::post::{AiHistory, AiRefineRecord};
use crate::stage::Stage;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};

#[derive(Debug)]
pub struct RefineArgs {
    pub slug: String,
    pub workdir: PathBuf,
    pub prompt: String,
    pub model: String,
    pub no_sync: bool,
}

pub fn run(jj: &dyn Jj, args: RefineArgs) -> Result<()> {
    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let (handle, mut post) = repo.load_raw(&args.slug)?;

    if handle.stage != Stage::Editing {
        return Err(Error::RefineWrongStage {
            slug: args.slug,
            stage: handle.stage,
        });
    }

    let llm_prompt = build_prompt(&post.metadata.title, &post.body, &args.prompt);
    let reply = openrouter::chat(&llm_prompt, &args.model)?;

    post.body = reply;
    post.metadata.ai = Some(merge_refine_record(
        post.metadata.ai.take(),
        AiRefineRecord {
            prompt: args.prompt.clone(),
            model: args.model.clone(),
            generated_at: OffsetDateTime::now_utc(),
        },
    ));

    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, args.no_sync);
    let message = format!("post({}): refine via {}", args.slug, args.model);
    let path = handle.path.clone();
    let model = args.model.clone();
    let slug = args.slug.clone();

    sync::commit_and_push(jj, &args.workdir, &opts, &message, || {
        post.metadata.updated_at = OffsetDateTime::now_utc();
        let rendered = post.render()?;
        fs::write(&path, rendered).map_err(|e| Error::io(&path, e))?;
        Ok(())
    })?;
    println!("{slug}: refined via {model}");
    Ok(())
}

/// Compose the LLM prompt: small preamble framing the task as
/// iteration, then the post's title and current body as context, then
/// the user's refinement instruction. Kept as a separate fn so the
/// shape is unit-testable without spinning up a mock server.
fn build_prompt(title: &str, current_body: &str, user_prompt: &str) -> String {
    let mut s = String::with_capacity(
        // Rough size estimate; the +256 covers the boilerplate.
        title.len() + current_body.len() + user_prompt.len() + 256,
    );
    s.push_str(
        "You are iterating on an existing blog post draft. \
         Produce the revised post body in Markdown. \
         Do not include a title, frontmatter, or commentary about your output — \
         only the body text the reader will see.\n\n",
    );
    s.push_str("Title: ");
    s.push_str(title);
    s.push_str("\n\n");
    if !current_body.trim().is_empty() {
        s.push_str("Current body:\n");
        s.push_str(current_body.trim());
        s.push_str("\n\n");
    }
    s.push_str("Author instruction: ");
    s.push_str(user_prompt);
    s
}

/// Fold a new `AiRefineRecord` into the post's optional `AiHistory`,
/// preserving any other AI-history fields (notably `ai.draft` from an
/// earlier `draft` invocation).
fn merge_refine_record(existing: Option<AiHistory>, new_refine: AiRefineRecord) -> AiHistory {
    let mut history = existing.unwrap_or_default();
    history.refine = Some(new_refine);
    history
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::post::AiDraftRecord;

    #[test]
    fn prompt_includes_title_and_user_instruction() {
        let p = build_prompt("The Title", "draft body", "tighten paragraph two");
        assert!(p.contains("Title: The Title"));
        assert!(p.contains("Author instruction: tighten paragraph two"));
    }

    #[test]
    fn prompt_includes_current_body_when_non_empty() {
        let p = build_prompt("T", "existing draft text", "trim");
        assert!(p.contains("Current body:"));
        assert!(p.contains("existing draft text"));
    }

    #[test]
    fn prompt_skips_body_block_when_empty() {
        // Refine on an empty body is silly (use draft), but the
        // helper handles it gracefully rather than emitting a
        // misleading "Current body:" header over nothing.
        let p = build_prompt("T", "", "go");
        assert!(!p.contains("Current body:"));
    }

    #[test]
    fn prompt_skips_body_block_when_whitespace() {
        let p = build_prompt("T", "   \n  \t\n", "go");
        assert!(!p.contains("Current body:"));
    }

    #[test]
    fn prompt_specifies_markdown_and_excludes_frontmatter() {
        let p = build_prompt("T", "draft", "go");
        assert!(p.to_lowercase().contains("markdown"));
        assert!(p.to_lowercase().contains("frontmatter"));
    }

    #[test]
    fn prompt_frames_task_as_iteration_not_drafting() {
        // Different preamble from `draft`'s — the model should know
        // it's editing existing content, not generating from a seed.
        let p = build_prompt("T", "draft", "polish");
        assert!(p.to_lowercase().contains("iterating"));
    }

    #[test]
    fn merge_creates_history_when_none_existed() {
        let h = merge_refine_record(
            None,
            AiRefineRecord {
                prompt: "p".into(),
                model: "m".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            },
        );
        assert!(h.refine.is_some());
        assert!(h.draft.is_none());
    }

    #[test]
    fn merge_preserves_prior_draft_record() {
        // A post that went through `draft` then `refine` should keep
        // its `ai.draft` block as audit trail for the original
        // generation — never overwrite it.
        let prior = AiHistory {
            draft: Some(AiDraftRecord {
                prompt: "draft-prompt".into(),
                model: "draft-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            }),
            refine: None,
        };
        let h = merge_refine_record(
            Some(prior),
            AiRefineRecord {
                prompt: "refine-prompt".into(),
                model: "refine-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            },
        );
        assert_eq!(h.draft.unwrap().prompt, "draft-prompt");
        assert_eq!(h.refine.unwrap().prompt, "refine-prompt");
    }

    #[test]
    fn merge_overwrites_prior_refine_record() {
        let prior = AiHistory {
            draft: None,
            refine: Some(AiRefineRecord {
                prompt: "old".into(),
                model: "old-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            }),
        };
        let h = merge_refine_record(
            Some(prior),
            AiRefineRecord {
                prompt: "new".into(),
                model: "new-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            },
        );
        assert_eq!(h.refine.unwrap().prompt, "new");
    }
}
