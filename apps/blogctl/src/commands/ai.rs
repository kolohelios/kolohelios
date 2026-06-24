use std::path::PathBuf;

use crate::error::Result;
use crate::openrouter;
use crate::storage::{ConfigStatus, Repository, Workdir};

/// One-shot OpenRouter prompt. Unlike `draft`/`refine`/`final-edit`,
/// `ping` doesn't operate on a post and works outside an initialized
/// workdir — so it reads `[defaults].model` best-effort: if `workdir`
/// holds a readable `.blog-os.toml`, its `defaults.model` participates
/// in resolution; a missing or unparsable config simply falls through
/// to the built-in default. `--model` always wins when passed.
pub fn ping(prompt: String, model: Option<String>, workdir: PathBuf) -> Result<()> {
    let config_default = match Repository::unchecked(Workdir::new(&workdir)).audit_config() {
        ConfigStatus::Ok(config) => config.defaults.model,
        ConfigStatus::Missing | ConfigStatus::Unparsable(_) => None,
    };
    let model = openrouter::resolve_model(model.as_deref(), config_default.as_deref());
    let reply = openrouter::chat(&prompt, &model)?;
    println!("{reply}");
    Ok(())
}
