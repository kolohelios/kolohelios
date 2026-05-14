use std::io;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::commands;
use crate::error::Result;
use crate::kind::Kind;
use crate::openrouter;

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
        /// Surface this targets: `post` (short-form feed) or `article`
        /// (long-form). Drives prompt/exit-criteria selection downstream.
        #[arg(long, value_enum, default_value_t = Kind::Post)]
        kind: Kind,
        /// Narrative theme. Defaults to the workdir's
        /// `defaults.theme`; must be declared in `[themes.*]`.
        #[arg(long)]
        theme: Option<String>,
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
    /// Manage the generated workdir `README.md`.
    Readme {
        #[command(subcommand)]
        action: ReadmeAction,
    },
    /// Audit a workdir for layout, frontmatter, and config problems.
    /// Exits non-zero if any finding is surfaced.
    Doctor {
        #[arg(long, value_name = "PATH")]
        workdir: PathBuf,
    },
    /// Apply doctor's auto-correctable repairs. Skipped findings
    /// remain non-zero so `fix` followed by `doctor` is a useful
    /// idempotency check.
    Fix {
        #[arg(long, value_name = "PATH")]
        workdir: PathBuf,
        /// Print the plan without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// LLM interaction subcommands.
    Ai {
        #[command(subcommand)]
        action: AiAction,
    },
    #[command(hide = true)]
    Completions { shell: Shell },
}

#[derive(Subcommand, Debug)]
pub enum AiAction {
    /// Send a one-shot prompt to OpenRouter and print the reply.
    /// Requires `OPENROUTER_API_KEY` in the environment — wrap the
    /// invocation in `op run --env-file=.env -- ...` to resolve it
    /// from 1Password.
    Ping {
        /// Prompt text to send to the model.
        prompt: String,
        /// OpenRouter model identifier.
        #[arg(long, default_value = openrouter::DEFAULT_MODEL)]
        model: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ReadmeAction {
    /// Overwrite the workdir `README.md` with the canonical template
    /// baked into `blogctl`. Use after a `blogctl` upgrade to pick up
    /// template changes.
    Regenerate {
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
            kind,
            theme,
        } => commands::new::run(title, workdir, slug, kind, theme),
        Command::List { workdir } => commands::list::run(workdir),
        Command::Show { slug, workdir } => commands::show::run(slug, workdir),
        Command::Promote { slug, workdir } => commands::promote::run(slug, workdir),
        Command::Demote { slug, workdir } => commands::demote::run(slug, workdir),
        Command::Readme { action } => match action {
            ReadmeAction::Regenerate { workdir } => commands::readme::regenerate(workdir),
        },
        Command::Doctor { workdir } => commands::doctor::run(workdir),
        Command::Fix { workdir, dry_run } => commands::fix::run(workdir, dry_run),
        Command::Ai { action } => match action {
            AiAction::Ping { prompt, model } => commands::ai::ping(prompt, model),
        },
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut io::stdout());
            Ok(())
        }
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
            vec![
                "blogctl",
                "new",
                "Hello",
                "--workdir",
                "/tmp/wd",
                "--kind",
                "article",
            ],
            vec![
                "blogctl",
                "new",
                "Hello",
                "--workdir",
                "/tmp/wd",
                "--theme",
                "parable",
            ],
            vec!["blogctl", "list", "--workdir", "/tmp/wd"],
            vec!["blogctl", "show", "hello", "--workdir", "/tmp/wd"],
            vec!["blogctl", "promote", "hello", "--workdir", "/tmp/wd"],
            vec!["blogctl", "demote", "hello", "--workdir", "/tmp/wd"],
            vec!["blogctl", "readme", "regenerate", "--workdir", "/tmp/wd"],
            vec!["blogctl", "doctor", "--workdir", "/tmp/wd"],
            vec!["blogctl", "fix", "--workdir", "/tmp/wd"],
            vec!["blogctl", "fix", "--workdir", "/tmp/wd", "--dry-run"],
            vec!["blogctl", "ai", "ping", "hello"],
            vec![
                "blogctl",
                "ai",
                "ping",
                "hello",
                "--model",
                "openai/gpt-4o-mini",
            ],
            vec!["blogctl", "completions", "bash"],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_ok(), "{args:?}");
        }
    }

    #[test]
    fn new_rejects_unknown_kind() {
        let args = vec![
            "blogctl",
            "new",
            "Hello",
            "--workdir",
            "/tmp/wd",
            "--kind",
            "essay",
        ];
        assert!(Cli::try_parse_from(args).is_err());
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
            vec!["blogctl", "doctor"],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_err(), "{args:?}");
        }
    }
}
