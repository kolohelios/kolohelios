//! `blogctl final-edit <slug> [--prompt <text>]` — last-pass polish on
//! a `FinalEditing`-stage post via OpenRouter.
//!
//! Operates on `FinalEditing`-stage posts only — the polish pass runs
//! after `draft` and `refine` have produced something the author is
//! happy with structurally.
//!
//! Without `--prompt`, the polish guidance is baked in and aimed at
//! LinkedIn-format posts: strip LLMisms, drop em-dashes and staccato
//! sequences, cap at 500 words, no Markdown (LinkedIn doesn't render
//! it), only words and blank lines for structure. Override with
//! `--prompt` for non-LinkedIn targets (articles, other surfaces).
//!
//! Replaces the post body with the model's reply and records the audit
//! block in `metadata.ai.final_edit`. Prior `ai.draft` and `ai.refine`
//! records are preserved.

use std::fs;
use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::openrouter;
use crate::post::{AiFinalEditRecord, AiHistory};
use crate::stage::Stage;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};

/// Default polish instruction used when `--prompt` is omitted. Aimed at
/// LinkedIn feed posts (`Kind::Post`); pass `--prompt` to polish an
/// article or other surface with different rules.
const DEFAULT_POLISH_PROMPT: &str = "Remove the LLMisms, staccato sequences, em-dashes. \
The post should be for LinkedIn, 500 words or less. \
We should use words and whitespace (new double line breaks) as our only means of expression. \
There is no markdown support.";

#[derive(Debug)]
pub struct FinalEditArgs {
    pub slug: String,
    pub workdir: PathBuf,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub no_sync: bool,
}

pub fn run(jj: &dyn Jj, args: FinalEditArgs) -> Result<()> {
    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let (handle, mut post) = repo.load_raw(&args.slug)?;

    if handle.stage != Stage::FinalEditing {
        return Err(Error::FinalEditWrongStage {
            slug: args.slug,
            stage: handle.stage,
        });
    }

    let config = repo.read_config()?;
    let model = openrouter::resolve_model(args.model.as_deref(), config.defaults.model.as_deref());

    let instruction = args
        .prompt
        .clone()
        .unwrap_or_else(|| DEFAULT_POLISH_PROMPT.to_string());
    let llm_prompt = build_prompt(&post.metadata.title, &post.body, &instruction);
    let reply = openrouter::chat(&llm_prompt, &model)?;

    post.body = reply;
    post.metadata.ai = Some(merge_final_edit_record(
        post.metadata.ai.take(),
        AiFinalEditRecord {
            prompt: instruction,
            model: model.clone(),
            generated_at: OffsetDateTime::now_utc(),
        },
    ));

    let opts = SyncOptions::from_config(&config.sync, args.no_sync);
    let message = format!("post({}): final-edit via {}", args.slug, model);
    let path = handle.path.clone();
    let slug = args.slug.clone();

    sync::commit_and_push(jj, &args.workdir, &opts, &message, || {
        post.metadata.updated_at = OffsetDateTime::now_utc();
        let rendered = post.render()?;
        fs::write(&path, rendered).map_err(|e| Error::io(&path, e))?;
        Ok(())
    })?;
    println!("{slug}: final-edited via {model}");
    Ok(())
}

/// Compose the LLM prompt. The preamble frames the task as a polish
/// pass and tells the model to emit only the body text — no title, no
/// frontmatter, no meta-commentary. Crucially, it does NOT instruct
/// the model to use Markdown; the default polish prompt explicitly
/// targets LinkedIn, which doesn't render Markdown, and a `--prompt`
/// override can re-introduce Markdown if needed.
fn build_prompt(title: &str, current_body: &str, instruction: &str) -> String {
    let mut s = String::with_capacity(title.len() + current_body.len() + instruction.len() + 256);
    s.push_str(
        "You are performing a final polish pass on a blog post draft. \
         Produce the revised post body only — no title, no frontmatter, \
         no commentary about your output.\n\n",
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
    s.push_str(instruction);
    s
}

/// Fold a new `AiFinalEditRecord` into the post's optional `AiHistory`,
/// preserving any prior `ai.draft` and `ai.refine` records.
fn merge_final_edit_record(
    existing: Option<AiHistory>,
    new_record: AiFinalEditRecord,
) -> AiHistory {
    let mut history = existing.unwrap_or_default();
    history.final_edit = Some(new_record);
    history
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::post::{AiDraftRecord, AiRefineRecord};

    #[test]
    fn prompt_includes_title_and_instruction() {
        let p = build_prompt("The Title", "polished body", "tighten");
        assert!(p.contains("Title: The Title"));
        assert!(p.contains("Author instruction: tighten"));
    }

    #[test]
    fn prompt_includes_current_body_when_non_empty() {
        let p = build_prompt("T", "some body text", "polish");
        assert!(p.contains("Current body:"));
        assert!(p.contains("some body text"));
    }

    #[test]
    fn prompt_skips_body_block_when_empty() {
        let p = build_prompt("T", "", "go");
        assert!(!p.contains("Current body:"));
    }

    #[test]
    fn prompt_skips_body_block_when_whitespace() {
        let p = build_prompt("T", "   \n\t\n", "go");
        assert!(!p.contains("Current body:"));
    }

    #[test]
    fn prompt_does_not_instruct_markdown_output() {
        // The default polish target is LinkedIn, which doesn't render
        // Markdown. The preamble must stay format-agnostic — the user's
        // (or default) instruction decides the output format.
        let p = build_prompt("T", "body", DEFAULT_POLISH_PROMPT);
        assert!(
            !p.to_lowercase().contains("in markdown"),
            "preamble should not impose Markdown on the model: {p}"
        );
    }

    #[test]
    fn prompt_frames_task_as_polish_not_drafting() {
        let p = build_prompt("T", "body", "polish");
        assert!(p.to_lowercase().contains("polish"));
    }

    #[test]
    fn default_polish_prompt_targets_linkedin_constraints() {
        // Smoke-test the baked-in default so a future edit that loses
        // the LinkedIn focus is caught — this is the primary use case
        // and the prompt is load-bearing.
        assert!(DEFAULT_POLISH_PROMPT.to_lowercase().contains("linkedin"));
        assert!(DEFAULT_POLISH_PROMPT.contains("500"));
        assert!(DEFAULT_POLISH_PROMPT.to_lowercase().contains("markdown"));
    }

    #[test]
    fn merge_creates_history_when_none_existed() {
        let h = merge_final_edit_record(
            None,
            AiFinalEditRecord {
                prompt: "p".into(),
                model: "m".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            },
        );
        assert!(h.final_edit.is_some());
        assert!(h.draft.is_none());
        assert!(h.refine.is_none());
    }

    #[test]
    fn merge_preserves_prior_draft_and_refine_records() {
        // A post that went through draft → refine → final-edit should
        // keep all three audit blocks — final-edit must never clobber
        // its predecessors.
        let prior = AiHistory {
            draft: Some(AiDraftRecord {
                prompt: "draft-prompt".into(),
                model: "draft-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            }),
            refine: Some(AiRefineRecord {
                prompt: "refine-prompt".into(),
                model: "refine-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            }),
            final_edit: None,
        };
        let h = merge_final_edit_record(
            Some(prior),
            AiFinalEditRecord {
                prompt: "polish-prompt".into(),
                model: "polish-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            },
        );
        assert_eq!(h.draft.unwrap().prompt, "draft-prompt");
        assert_eq!(h.refine.unwrap().prompt, "refine-prompt");
        assert_eq!(h.final_edit.unwrap().prompt, "polish-prompt");
    }

    #[test]
    fn merge_overwrites_prior_final_edit_record() {
        let prior = AiHistory {
            draft: None,
            refine: None,
            final_edit: Some(AiFinalEditRecord {
                prompt: "old".into(),
                model: "old-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            }),
        };
        let h = merge_final_edit_record(
            Some(prior),
            AiFinalEditRecord {
                prompt: "new".into(),
                model: "new-model".into(),
                generated_at: OffsetDateTime::UNIX_EPOCH,
            },
        );
        assert_eq!(h.final_edit.unwrap().prompt, "new");
    }
}
