use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "todoist", version, about = "Command-line client for Todoist")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage Todoist API credentials
    Auth(AuthCmd),
    /// Manage tasks
    Tasks(TasksCmd),
}

#[derive(Args)]
struct AuthCmd {
    #[command(subcommand)]
    command: AuthSubcommand,
}

#[derive(Subcommand)]
enum AuthSubcommand {
    /// Store a 1Password reference to your Todoist API token
    Login {
        /// 1Password secret reference (e.g. op://Personal/Todoist API/credential)
        #[arg(long = "op-ref")]
        op_ref: String,
    },
    /// Show the stored token reference and whether it currently resolves
    Status,
    /// Forget the stored token reference
    Logout,
}

#[derive(Args)]
struct TasksCmd {
    #[command(subcommand)]
    command: TasksSubcommand,
}

#[derive(Subcommand)]
enum TasksSubcommand {
    /// List active tasks
    List {
        /// Filter to a project (name or ID)
        #[arg(long)]
        project: Option<String>,
        /// Todoist filter query (e.g. "today", "overdue", "@waiting")
        #[arg(long)]
        filter: Option<String>,
        /// Maximum number of tasks to show
        #[arg(long)]
        limit: Option<usize>,
        /// Emit raw task objects as ndjson
        #[arg(long)]
        json: bool,
    },
    /// Add a new task
    Add {
        /// Task content
        content: String,
        /// Target project (name or ID); defaults to Inbox
        #[arg(long)]
        project: Option<String>,
        /// Natural-language due string (e.g. "tomorrow at 3pm")
        #[arg(long)]
        due: Option<String>,
        /// Priority 1-4 (4 = highest)
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
        priority: Option<u8>,
        /// Label to attach (repeatable)
        #[arg(long = "label")]
        labels: Vec<String>,
        /// Long-form description
        #[arg(long)]
        description: Option<String>,
    },
    /// Close one or more tasks by full ID or short prefix
    Complete {
        /// Task IDs or unique short prefixes
        #[arg(required = true)]
        ids: Vec<String>,
    },
}

fn main() -> Result<()> {
    dispatch(Cli::parse().command)
}

fn dispatch(command: Command) -> Result<()> {
    match command {
        Command::Auth(_) => bail!("auth subcommands are not yet implemented"),
        Command::Tasks(_) => bail!("tasks subcommands are not yet implemented"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_auth_login_with_op_ref() {
        let cli = Cli::try_parse_from([
            "todoist",
            "auth",
            "login",
            "--op-ref",
            "op://Personal/Todoist API/credential",
        ])
        .expect("should parse");
        match cli.command {
            Command::Auth(AuthCmd {
                command: AuthSubcommand::Login { op_ref },
            }) => assert_eq!(op_ref, "op://Personal/Todoist API/credential"),
            _ => panic!("expected Auth::Login"),
        }
    }

    #[test]
    fn parses_tasks_list_with_filter() {
        let cli = Cli::try_parse_from(["todoist", "tasks", "list", "--filter", "today"])
            .expect("should parse");
        match cli.command {
            Command::Tasks(TasksCmd {
                command: TasksSubcommand::List { filter, json, .. },
            }) => {
                assert_eq!(filter.as_deref(), Some("today"));
                assert!(!json);
            }
            _ => panic!("expected Tasks::List"),
        }
    }

    #[test]
    fn parses_tasks_add_with_labels_and_priority() {
        let cli = Cli::try_parse_from([
            "todoist",
            "tasks",
            "add",
            "buy milk",
            "--priority",
            "3",
            "--label",
            "errand",
            "--label",
            "shopping",
        ])
        .expect("should parse");
        match cli.command {
            Command::Tasks(TasksCmd {
                command:
                    TasksSubcommand::Add {
                        content,
                        priority,
                        labels,
                        ..
                    },
            }) => {
                assert_eq!(content, "buy milk");
                assert_eq!(priority, Some(3));
                assert_eq!(labels, vec!["errand", "shopping"]);
            }
            _ => panic!("expected Tasks::Add"),
        }
    }

    #[test]
    fn rejects_priority_outside_one_to_four() {
        assert!(Cli::try_parse_from(["todoist", "tasks", "add", "x", "--priority", "5"]).is_err());
    }

    #[test]
    fn dispatch_auth_returns_unimplemented_error() {
        let err = dispatch(Command::Auth(AuthCmd {
            command: AuthSubcommand::Status,
        }))
        .unwrap_err();
        assert!(err.to_string().contains("auth"));
    }

    #[test]
    fn dispatch_tasks_returns_unimplemented_error() {
        let err = dispatch(Command::Tasks(TasksCmd {
            command: TasksSubcommand::Complete {
                ids: vec!["abc".into()],
            },
        }))
        .unwrap_err();
        assert!(err.to_string().contains("tasks"));
    }

    #[test]
    fn parses_tasks_complete_with_multiple_ids() {
        let cli =
            Cli::try_parse_from(["todoist", "tasks", "complete", "abc123", "def456"]).expect("");
        match cli.command {
            Command::Tasks(TasksCmd {
                command: TasksSubcommand::Complete { ids },
            }) => assert_eq!(ids, vec!["abc123", "def456"]),
            _ => panic!("expected Tasks::Complete"),
        }
    }
}
