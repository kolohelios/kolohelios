use std::path::PathBuf;

use time::OffsetDateTime;

use crate::error::Result;
use crate::kind::Kind;
use crate::post::{Post, PostMetadata};
use crate::stage::Stage;
use crate::storage::{Repository, Workdir};
use crate::{slug, Error};

pub fn run(
    title: String,
    workdir: PathBuf,
    slug_override: Option<String>,
    kind: Kind,
) -> Result<()> {
    if title.trim().is_empty() {
        return Err(Error::EmptyTitle(title));
    }
    let resolved_slug = match slug_override {
        Some(s) => slug::validate(&s)?.to_string(),
        None => slug::slugify(&title)?,
    };

    let repo = Repository::open(Workdir::new(&workdir))?;

    let now = OffsetDateTime::now_utc();
    let metadata = PostMetadata {
        title,
        slug: resolved_slug.clone(),
        kind,
        status: Stage::Concept,
        created_at: now,
        updated_at: now,
        tags: vec![],
        todoist_task_id: None,
        history_checked: false,
    };
    let post = Post::new(metadata, "");
    let path = repo.create_post(&post)?;
    println!("created {}", path.display());
    Ok(())
}
