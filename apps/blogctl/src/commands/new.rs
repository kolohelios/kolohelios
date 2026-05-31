use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::Result;
use crate::kind::Kind;
use crate::post::{Post, PostMetadata};
use crate::stage::Stage;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};
use crate::{slug, Error};

pub fn run(
    jj: &dyn Jj,
    title: String,
    workdir: PathBuf,
    slug_override: Option<String>,
    kind: Kind,
    theme: Option<String>,
    no_sync: bool,
) -> Result<()> {
    if title.trim().is_empty() {
        return Err(Error::EmptyTitle { value: title });
    }
    let resolved_slug = match slug_override {
        Some(s) => slug::validate(&s)?.to_string(),
        None => slug::slugify(&title)?,
    };

    let repo = Repository::open(Workdir::new(&workdir))?;
    let config = repo.read_config()?;
    let resolved_theme = theme.unwrap_or_else(|| config.defaults.theme.clone());
    if !config.themes.contains_key(&resolved_theme) {
        let known: Vec<String> = config.themes.keys().cloned().collect();
        return Err(Error::UnknownTheme {
            theme: resolved_theme,
            known,
        });
    }

    let message = format!(
        "post({}): draft \"{}\"",
        resolved_slug,
        title.replace('"', "\\\"")
    );
    let opts = SyncOptions::from_config(&config.sync, no_sync);

    let path = sync::commit_and_push(jj, &workdir, &opts, &message, || {
        let now = OffsetDateTime::now_utc();
        let metadata = PostMetadata {
            title,
            slug: resolved_slug.clone(),
            kind,
            theme: resolved_theme,
            status: Stage::Concept,
            created_at: now,
            updated_at: now,
            tags: vec![],
            todoist_task_id: None,
            history_checked: false,
            targets: vec![],
            classifications: Default::default(),
            ai: None,
        };
        let post = Post::new(metadata, "");
        repo.create_post(&post)
    })?;
    println!("created {}", path.display());
    Ok(())
}
