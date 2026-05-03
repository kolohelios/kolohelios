use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::commands;
use crate::error::Result;

#[derive(Parser, Debug)]
#[command(
    name = "blogctl",
    about = "Manage Markdown blog post drafts across workflow stages"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create the workdir layout and write `.blog-os.toml`.
    Init {
        #[arg(long, value_name = "PATH")]
        workdir: PathBuf,
    },
    /// Create a new post in the `concept` stage.
    New {
        /// Post title — slug is derived from this unless `--slug` is given.
        title: String,
        #[arg(long, value_name = "PATH")]
        workdir: PathBuf,
        /// Override the auto-derived slug.
        #[arg(long)]
        slug: Option<String>,
    },
    /// List every post in the workdir, grouped by stage.
    List {
        #[arg(long, value_name = "PATH")]
        workdir: PathBuf,
    },
    /// Print a post's frontmatter and body.
    Show {
        slug: String,
        #[arg(long, value_name = "PATH")]
        workdir: PathBuf,
    },
    /// Move a post to the next workflow stage.
    Promote {
        slug: String,
        #[arg(long, value_name = "PATH")]
        workdir: PathBuf,
    },
    /// Move a post one stage back.
    Demote {
        slug: String,
        #[arg(long, value_name = "PATH")]
        workdir: PathBuf,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    dispatch(cli.command)
}

pub fn dispatch(cmd: Command) -> Result<()> {
    match cmd {
        Command::Init { workdir } => commands::init::run(workdir),
        Command::New {
            title,
            workdir,
            slug,
        } => commands::new::run(title, workdir, slug),
        Command::List { workdir } => commands::list::run(workdir),
        Command::Show { slug, workdir } => commands::show::run(slug, workdir),
        Command::Promote { slug, workdir } => commands::promote::run(slug, workdir),
        Command::Demote { slug, workdir } => commands::demote::run(slug, workdir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses_each_subcommand() {
        for args in [
            vec!["blogctl", "init", "--workdir", "/tmp/wd"],
            vec!["blogctl", "new", "Hello", "--workdir", "/tmp/wd"],
            vec![
                "blogctl",
                "new",
                "Hello",
                "--workdir",
                "/tmp/wd",
                "--slug",
                "hello",
            ],
            vec!["blogctl", "list", "--workdir", "/tmp/wd"],
            vec!["blogctl", "show", "hello", "--workdir", "/tmp/wd"],
            vec!["blogctl", "promote", "hello", "--workdir", "/tmp/wd"],
            vec!["blogctl", "demote", "hello", "--workdir", "/tmp/wd"],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_ok(), "{args:?}");
        }
    }

    #[test]
    fn cli_validates_command_layout() {
        Cli::command().debug_assert();
    }

    #[test]
    fn workdir_is_required_for_every_subcommand() {
        for args in [
            vec!["blogctl", "init"],
            vec!["blogctl", "new", "Hello"],
            vec!["blogctl", "list"],
            vec!["blogctl", "show", "hello"],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_err(), "{args:?}");
        }
    }
}
