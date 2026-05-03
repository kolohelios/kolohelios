use std::path::PathBuf;

use crate::error::Result;
use crate::storage::{Repository, Workdir};

pub fn run(workdir: PathBuf) -> Result<()> {
    let repo = Repository::unchecked(Workdir::new(&workdir));
    repo.init()?;
    println!("initialized blogctl workdir at {}", workdir.display());
    Ok(())
}
