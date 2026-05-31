use std::error::Error;

use clap::{Args, Parser, Subcommand};
use snafu::{ResultExt, Snafu};

mod api;
mod auth;
mod projects;
mod tasks;

use auth::OpRunner;

#[derive(Debug, Snafu)]
enum CliError {
    #[snafu(display("{source}"))]
    Auth { source: auth::AuthError },

    #[snafu(display("{source}"))]
    Projects { source: projects::ProjectsError },

    #[snafu(display("{source}"))]
    Tasks { source: tasks::TasksError },

    #[snafu(display("could not read {} — run `todoist auth login` first", path.display()))]
    ReadConfig {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("could not parse stored auth config"))]
    ParseConfig { source: toml::de::Error },

    #[snafu(display("could not resolve token from 1Password: {source}"))]
    ResolveToken { source: auth::OpError },
}

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
    /// Inspect Todoist projects
    Projects(ProjectsCmd),
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
struct ProjectsCmd {
    #[command(subcommand)]
    command: ProjectsSubcommand,
}

#[derive(Subcommand)]
enum ProjectsSubcommand {
    /// List all projects
    List {
        /// Emit raw project objects as ndjson
        #[arg(long)]
        json: bool,
    },
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

fn main() {
    if let Err(e) = dispatch(Cli::parse().command) {
        eprintln!("{e}");
        let mut source = e.source();
        while let Some(s) = source {
            eprintln!("  caused by: {s}");
            source = s.source();
        }
        std::process::exit(1);
    }
}

fn dispatch(command: Command) -> Result<(), CliError> {
    match command {
        Command::Auth(AuthCmd { command }) => handle_auth(command),
        Command::Projects(ProjectsCmd { command }) => handle_projects(command),
        Command::Tasks(TasksCmd { command }) => handle_tasks(command),
    }
}

fn handle_projects(cmd: ProjectsSubcommand) -> Result<(), CliError> {
    match cmd {
        ProjectsSubcommand::List { json } => {
            let token = resolve_token()?;
            let client = api::RealClient::default();
            let projects = projects::run_list(&client, &token).context(ProjectsSnafu)?;
            if json {
                println!("{}", projects::render_ndjson(&projects));
            } else {
                println!("{}", projects::render_table(&projects));
            }
            Ok(())
        }
    }
}

fn handle_tasks(cmd: TasksSubcommand) -> Result<(), CliError> {
    match cmd {
        TasksSubcommand::List {
            project,
            filter,
            limit,
            json,
        } => {
            let token = resolve_token()?;
            let client = api::RealClient::default();
            let outcome = tasks::run_list(
                &client,
                &token,
                &tasks::ListOpts {
                    project,
                    filter,
                    limit,
                },
            )
            .context(TasksSnafu)?;
            if json {
                println!("{}", tasks::render_ndjson(&outcome.tasks));
            } else {
                println!(
                    "{}",
                    tasks::render_table(&outcome.tasks, &outcome.projects_by_id)
                );
            }
            Ok(())
        }
        TasksSubcommand::Add {
            content,
            project,
            due,
            priority,
            labels,
            description,
        } => {
            let token = resolve_token()?;
            let client = api::RealClient::default();
            let created = tasks::run_add(
                &client,
                &token,
                &tasks::AddOpts {
                    content,
                    project,
                    due,
                    priority,
                    labels,
                    description,
                },
            )
            .context(TasksSnafu)?;
            println!("{}", tasks::render_added(&created));
            Ok(())
        }
        TasksSubcommand::Complete { ids } => {
            let token = resolve_token()?;
            let client = api::RealClient::default();
            let results = tasks::run_complete(&client, &token, &ids).context(TasksSnafu)?;
            let use_unicode = std::io::IsTerminal::is_terminal(&std::io::stdout())
                && std::env::var_os("NO_COLOR").is_none();
            let (out, any_failure) = tasks::render_complete_results(&results, use_unicode);
            println!("{out}");
            if any_failure {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

fn resolve_token() -> Result<String, CliError> {
    let path = auth::default_config_path().context(AuthSnafu)?;
    let op = auth::RealOp;
    let raw = std::fs::read_to_string(&path).context(ReadConfigSnafu { path: path.clone() })?;
    let config: auth::Config = toml::from_str(&raw).context(ParseConfigSnafu)?;
    op.read(&config.token_op_ref).context(ResolveTokenSnafu)
}

fn handle_auth(cmd: AuthSubcommand) -> Result<(), CliError> {
    let path = auth::default_config_path().context(AuthSnafu)?;
    let op = auth::RealOp;
    match cmd {
        AuthSubcommand::Login { op_ref } => {
            auth::login(&op, &path, &op_ref).context(AuthSnafu)?;
            println!("saved token reference to {}", path.display());
            Ok(())
        }
        AuthSubcommand::Status => {
            println!(
                "{}",
                render_status(auth::status(&op, &path).context(AuthSnafu)?)
            );
            Ok(())
        }
        AuthSubcommand::Logout => {
            auth::logout(&path).context(AuthSnafu)?;
            println!("removed token reference (if any)");
            Ok(())
        }
    }
}

fn render_status(state: auth::AuthState) -> String {
    match state {
        auth::AuthState::NotLoggedIn => "not logged in".to_string(),
        auth::AuthState::Configured {
            op_ref,
            resolve_err,
        } => {
            let line = format!("token reference: {op_ref}");
            let check = match resolve_err {
                None => "resolve check: ok".to_string(),
                Some(detail) => format!("resolve check: failed ({detail})"),
            };
            format!("{line}\n{check}")
        }
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
    fn render_status_handles_not_logged_in() {
        let out = render_status(auth::AuthState::NotLoggedIn);
        assert_eq!(out, "not logged in");
    }

    #[test]
    fn render_status_handles_resolved_configuration() {
        let out = render_status(auth::AuthState::Configured {
            op_ref: "op://x".to_string(),
            resolve_err: None,
        });
        assert!(out.contains("op://x"));
        assert!(out.contains("ok"));
    }

    #[test]
    fn render_status_handles_failed_resolution() {
        let out = render_status(auth::AuthState::Configured {
            op_ref: "op://x".to_string(),
            resolve_err: Some("vault locked".to_string()),
        });
        assert!(out.contains("op://x"));
        assert!(out.contains("vault locked"));
    }

    #[test]
    fn parses_projects_list_default() {
        let cli = Cli::try_parse_from(["todoist", "projects", "list"]).expect("should parse");
        match cli.command {
            Command::Projects(ProjectsCmd {
                command: ProjectsSubcommand::List { json },
            }) => assert!(!json),
            _ => panic!("expected Projects::List"),
        }
    }

    #[test]
    fn parses_projects_list_with_json_flag() {
        let cli =
            Cli::try_parse_from(["todoist", "projects", "list", "--json"]).expect("should parse");
        match cli.command {
            Command::Projects(ProjectsCmd {
                command: ProjectsSubcommand::List { json },
            }) => assert!(json),
            _ => panic!("expected Projects::List"),
        }
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
