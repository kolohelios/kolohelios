use std::fs;
use std::path::PathBuf;

use crate::error::Result;
use crate::storage::{Repository, Workdir};
use crate::Error;

pub fn run(slug: String, workdir: PathBuf) -> Result<()> {
    let repo = Repository::open(Workdir::new(&workdir))?;
    let (handle, _post) = repo.load(&slug)?;
    let raw = fs::read_to_string(&handle.path).map_err(|e| Error::io(&handle.path, e))?;
    print!("{raw}");
    Ok(())
}
