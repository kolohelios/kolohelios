use std::fs;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::openrouter;
use crate::prompts;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};

/// Generate a draft for `slug` by rendering the configured prompt
/// template and calling OpenRouter. The reply lands as
/// `<workdir>/drafts/<slug>.draft-N.md` (next-free integer) — sidecar
/// to the post so the LLM output is never silently merged into the body.
pub fn run(
    jj: &dyn Jj,
    slug: String,
    workdir: PathBuf,
    model_override: Option<String>,
    no_sync: bool,
) -> Result<()> {
    let repo = Repository::open(Workdir::new(&workdir))?;
    let (handle, post) = repo.load(&slug)?;
    let config = repo.read_config()?;

    let stage_cfg = config
        .stage_config(post.metadata.kind, handle.stage)
        .ok_or(Error::DraftStageUnconfigured {
            kind: post.metadata.kind,
            stage: handle.stage,
        })?;

    let prompt_path = repo.workdir().root().join(&stage_cfg.prompt);
    let rendered_prompt = prompts::render(&prompt_path, &post)?;

    let model = match model_override {
        Some(m) => m,
        None => config
            .model_for(post.metadata.kind, handle.stage)
            .ok_or(Error::DraftModelUnconfigured {
                kind: post.metadata.kind,
                stage: handle.stage,
            })?
            .to_string(),
    };

    let reply = openrouter::chat(&rendered_prompt, &model)?;

    let drafts_dir = repo.workdir().drafts_dir();
    fs::create_dir_all(&drafts_dir).map_err(|e| Error::io(&drafts_dir, e))?;
    let draft_path = next_draft_path(&repo, &slug)?;
    let draft_name = draft_path
        .file_name()
        .expect("draft_path always has a file name")
        .to_string_lossy()
        .into_owned();

    let message = format!("post({slug}): draft {draft_name} via {model}");
    let opts = SyncOptions::from_config(&config.sync, no_sync);

    let written = sync::commit_and_push(jj, &workdir, &opts, &message, || {
        fs::write(&draft_path, &reply).map_err(|e| Error::io(&draft_path, e))?;
        Ok(draft_path.clone())
    })?;

    println!("wrote {}", written.display());
    Ok(())
}

/// First free `drafts/<slug>.draft-N.md`, starting at N=1. The loop's
/// upper bound exists so a bug can't spin the disk forever; a user
/// iterating by hand will give up long before the cap fires.
fn next_draft_path(repo: &Repository, slug: &str) -> Result<PathBuf> {
    for n in 1..=999u32 {
        let p = repo.workdir().draft_path(slug, n);
        if !p.exists() {
            return Ok(p);
        }
    }
    Err(Error::DraftFloodLimit(slug.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn repo_with_drafts_dir() -> (TempDir, Repository) {
        let tmp = TempDir::new().unwrap();
        let repo = Repository::unchecked(Workdir::new(tmp.path()));
        repo.init().unwrap();
        fs::create_dir_all(repo.workdir().drafts_dir()).unwrap();
        (tmp, repo)
    }

    #[test]
    fn next_draft_path_starts_at_one() {
        let (_tmp, repo) = repo_with_drafts_dir();
        let p = next_draft_path(&repo, "hello").unwrap();
        assert!(p.ends_with("hello.draft-1.md"));
    }

    #[test]
    fn next_draft_path_skips_existing_drafts() {
        let (_tmp, repo) = repo_with_drafts_dir();
        for n in 1..=3 {
            fs::write(repo.workdir().draft_path("hello", n), "stub").unwrap();
        }
        let p = next_draft_path(&repo, "hello").unwrap();
        assert!(p.ends_with("hello.draft-4.md"));
    }

    #[test]
    fn next_draft_path_per_slug_numbering_is_independent() {
        let (_tmp, repo) = repo_with_drafts_dir();
        fs::write(repo.workdir().draft_path("alpha", 1), "stub").unwrap();
        let p = next_draft_path(&repo, "beta").unwrap();
        assert!(p.ends_with("beta.draft-1.md"));
    }
}
