use std::path::PathBuf;

use crate::error::Result;
use crate::stage::Stage;
use crate::storage::{Repository, Workdir};

pub fn run(workdir: PathBuf) -> Result<()> {
    let repo = Repository::open(Workdir::new(&workdir))?;
    let posts = repo.list()?;

    if posts.is_empty() {
        println!("no posts in {}", workdir.display());
        return Ok(());
    }

    let mut current: Option<Stage> = None;
    for handle in posts {
        if Some(handle.stage) != current {
            if current.is_some() {
                println!();
            }
            println!("{}:", handle.stage);
            current = Some(handle.stage);
        }
        println!("  {}\t{}", handle.metadata.slug, handle.metadata.title);
    }
    Ok(())
}
