use crate::error::Result;
use crate::openrouter;

pub fn ping(prompt: String, model: String) -> Result<()> {
    let reply = openrouter::chat(&prompt, &model)?;
    println!("{reply}");
    Ok(())
}
