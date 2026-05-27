//! `blogctl import` — backfill a post that was already published on
//! another surface (LinkedIn, an external blog) before the workdir
//! existed.
//!
//! The editorial pipeline (`new` → `promote` × 4) is the wrong shape
//! for this case: there's no drafting to do, the body is already
//! written, and the post already has a URL and a publish date. This
//! command writes straight to `published/` and seeds a `targets:`
//! entry recording the venue, URL, and publish time — the same shape
//! `add-target` would produce if it accepted `--status published` /
//! `--url` / `--published-at` (deferred from #527).
//!
//! Out of scope: classifications and metrics (covered by
//! `blogctl backfill`, #435), multi-target single invocation (call
//! `import` once per surface), body cleanup or LinkedIn-format
//! munging (body is written verbatim from `--body-file`).

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::error::Result;
use crate::kind::Kind;
use crate::post::{Post, PostMetadata};
use crate::stage::Stage;
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};
use crate::target::{Target, TargetEntry, TargetStatus};
use crate::{slug, Error};

#[derive(Debug)]
pub struct ImportArgs {
    pub title: String,
    pub workdir: PathBuf,
    pub slug: Option<String>,
    pub kind: Kind,
    pub theme: Option<String>,
    pub target: Target,
    pub url: String,
    pub published_at: String,
    pub body_file: PathBuf,
    pub tags: Vec<String>,
    pub no_sync: bool,
}

pub fn run(jj: &dyn Jj, args: ImportArgs) -> Result<()> {
    if args.title.trim().is_empty() {
        return Err(Error::EmptyTitle(args.title));
    }
    let resolved_slug = match args.slug {
        Some(s) => slug::validate(&s)?.to_string(),
        None => slug::slugify(&args.title)?,
    };

    let published_at = OffsetDateTime::parse(&args.published_at, &Rfc3339).map_err(|source| {
        Error::InvalidPublishedAt {
            value: args.published_at.clone(),
            source,
        }
    })?;

    let body = read_body(&args.body_file)?;

    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let config = repo.read_config()?;
    let resolved_theme = args.theme.unwrap_or_else(|| config.defaults.theme.clone());
    if !config.themes.contains_key(&resolved_theme) {
        let known: Vec<String> = config.themes.keys().cloned().collect();
        return Err(Error::UnknownTheme {
            theme: resolved_theme,
            known,
        });
    }

    let message = format!("post({}): import from {}", resolved_slug, args.target);
    let opts = SyncOptions::from_config(&config.sync, args.no_sync);
    let target_name = args.target;

    let path = sync::commit_and_push(jj, &args.workdir, &opts, &message, || {
        let metadata = PostMetadata {
            title: args.title,
            slug: resolved_slug,
            kind: args.kind,
            theme: resolved_theme,
            status: Stage::Published,
            created_at: published_at,
            updated_at: published_at,
            tags: args.tags,
            todoist_task_id: None,
            history_checked: false,
            targets: vec![TargetEntry {
                name: target_name,
                status: TargetStatus::Published,
                url: Some(args.url),
                published_at: Some(published_at),
                metrics: None,
            }],
            classifications: Default::default(),
            ai: None,
        };
        let post = Post::new(metadata, body);
        repo.create_post(&post)
    })?;
    println!("imported {} (target: {})", path.display(), target_name);
    Ok(())
}

fn read_body(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| Error::io(PathBuf::from("<stdin>"), e))?;
        Ok(buf)
    } else {
        fs::read_to_string(path).map_err(|e| Error::io(path, e))
    }
}
