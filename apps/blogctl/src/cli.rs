use std::env;
use std::io;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::commands;
use crate::error::{Error, Result};
use crate::kind::Kind;
use crate::openrouter;
use crate::sync::{Jj, RealJj};
use crate::target::Target;

/// Resolve an optional `--workdir` to a concrete path, defaulting to
/// the current working directory when the flag is omitted.
fn workdir_or_pwd(workdir: Option<PathBuf>) -> Result<PathBuf> {
    match workdir {
        Some(p) => Ok(p),
        None => env::current_dir().map_err(Error::CurrentDirUnavailable),
    }
}

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
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Create a new post in the `concept` stage.
    New {
        /// Post title — slug is derived from this unless `--slug` is given.
        title: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
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
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// List every post in the workdir, grouped by stage.
    List {
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
    },
    /// Print a post's frontmatter and body.
    Show {
        slug: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
    },
    /// Move a post to the next workflow stage.
    Promote {
        slug: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Move a post one stage back.
    Demote {
        slug: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Manage the generated workdir `README.md`.
    Readme {
        #[command(subcommand)]
        action: ReadmeAction,
    },
    /// Audit a workdir for layout, frontmatter, and config problems.
    /// Exits non-zero if any finding is surfaced.
    Doctor {
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
    },
    /// Apply doctor's auto-correctable repairs. Skipped findings
    /// remain non-zero so `fix` followed by `doctor` is a useful
    /// idempotency check.
    Fix {
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Print the plan without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Bring the workdir into a known-good shape against the remote:
    /// fetch, move `@` directly onto `<bookmark>@<remote>`, fast-forward
    /// the local bookmark. Refuses if `@` has uncommitted edits or the
    /// local bookmark has unpushed commits.
    Update {
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Refuse to run (sync is the whole point of `update`).
        #[arg(long)]
        no_sync: bool,
    },
    /// LLM interaction subcommands.
    Ai {
        #[command(subcommand)]
        action: AiAction,
    },
    /// Generate a post body via OpenRouter from a single prompt seeded
    /// with the post's title + existing notes. Post must be in the
    /// `ideation` stage. Requires `OPENROUTER_API_KEY` in the
    /// environment — wrap the invocation in `op run --env-file=.env --
    /// ...` to resolve it from 1Password.
    Draft {
        slug: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Instruction to send to the model (alongside the post's
        /// title + existing body).
        #[arg(long)]
        prompt: String,
        /// OpenRouter model identifier.
        #[arg(long, default_value = openrouter::DEFAULT_MODEL)]
        model: String,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Iterate on a post body via OpenRouter, seeded with the post's
    /// title + current body. Post must be in the `editing` stage.
    /// Requires `OPENROUTER_API_KEY` in the environment.
    Refine {
        slug: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Refinement instruction to send to the model (alongside the
        /// post's title + current body).
        #[arg(long)]
        prompt: String,
        /// OpenRouter model identifier.
        #[arg(long, default_value = openrouter::DEFAULT_MODEL)]
        model: String,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Polish-pass a post body via OpenRouter. Post must be in the
    /// `final-editing` stage. Without `--prompt`, applies a baked-in
    /// LinkedIn-flavored polish prompt (strip LLMisms, cap at 500
    /// words, plain text only). Requires `OPENROUTER_API_KEY`.
    FinalEdit {
        slug: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Override the default polish instruction. Use this for
        /// non-LinkedIn surfaces (articles, etc.) where the baked-in
        /// LinkedIn rules don't apply.
        #[arg(long)]
        prompt: Option<String>,
        /// OpenRouter model identifier.
        #[arg(long, default_value = openrouter::DEFAULT_MODEL)]
        model: String,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Set classification dimensions on a post (format, hook, tone,
    /// audience, strategic-role, theme). Missing flags leave the
    /// existing value alone; `--clear-<dim>` removes it.
    Classify {
        slug: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        #[arg(long)]
        format: Option<String>,
        #[arg(long)]
        hook: Option<String>,
        #[arg(long)]
        tone: Option<String>,
        #[arg(long)]
        audience: Option<String>,
        #[arg(long)]
        strategic_role: Option<String>,
        /// Comma-separated list of themes. Replaces the existing
        /// theme list entirely.
        #[arg(long, value_delimiter = ',')]
        theme: Vec<String>,
        #[arg(long)]
        clear_format: bool,
        #[arg(long)]
        clear_hook: bool,
        #[arg(long)]
        clear_tone: bool,
        #[arg(long)]
        clear_audience: bool,
        #[arg(long)]
        clear_strategic_role: bool,
        #[arg(long)]
        clear_theme: bool,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Register a distribution target on a post. v1 fixes the new
    /// entry's status at `planned`; `published` / `retracted` (which
    /// require `--url` / `--published-at`) are deferred.
    AddTarget {
        slug: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        #[arg(long, value_enum)]
        target: Target,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Import an already-published post directly into `published/`,
    /// pre-populating a `targets:` entry with the venue URL and
    /// publish date. Used to backfill posts that went out on another
    /// surface (LinkedIn, an external blog) before the workdir existed.
    Import {
        /// Post title — slug is derived from this unless `--slug` is given.
        #[arg(long)]
        title: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Override the auto-derived slug.
        #[arg(long)]
        slug: Option<String>,
        /// Surface this targets: `post` (short-form feed) or `article`
        /// (long-form).
        #[arg(long, value_enum, default_value_t = Kind::Post)]
        kind: Kind,
        /// Narrative theme. Defaults to the workdir's
        /// `defaults.theme`; must be declared in `[themes.*]`.
        #[arg(long)]
        theme: Option<String>,
        /// Distribution venue this post was published to.
        #[arg(long, value_enum)]
        target: Target,
        /// URL of the already-published post on `--target`.
        #[arg(long)]
        url: String,
        /// RFC 3339 timestamp the post went live on `--target`.
        /// Also used as the post's `created_at` and `updated_at`.
        #[arg(long, value_name = "RFC3339")]
        published_at: String,
        /// Path to a file containing the post body. Use `-` to read
        /// from stdin. Body is written verbatim — no formatting
        /// munging.
        #[arg(long, value_name = "PATH")]
        body_file: PathBuf,
        /// Free-form tag for the post. Repeatable.
        #[arg(long = "tag", value_name = "TAG")]
        tags: Vec<String>,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Per-target performance metrics (impressions, reactions,
    /// comments, reposts).
    Metrics {
        #[command(subcommand)]
        action: MetricsAction,
    },
    /// Walk every published post and fill in missing classifications
    /// + metrics interactively, or batch-import from a JSON file.
    Backfill {
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Path to a JSON file of per-slug classifications/metrics
        /// entries. When set, runs in non-interactive batch mode.
        #[arg(long, value_name = "PATH")]
        import: Option<PathBuf>,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Analytics across classifications + metrics. Read-only.
    Analytics {
        #[command(subcommand)]
        action: AnalyticsAction,
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
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum MetricsAction {
    /// Set the latest observed metrics for a target on a post.
    Update {
        slug: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        #[arg(long, value_enum)]
        target: Target,
        #[arg(long)]
        impressions: u64,
        #[arg(long)]
        reactions: u64,
        #[arg(long)]
        comments: u64,
        #[arg(long)]
        reposts: u64,
        /// RFC 3339 timestamp. Defaults to `now()` if omitted.
        #[arg(long, value_name = "RFC3339")]
        sampled_at: Option<String>,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
    /// Print the latest metrics for every target on a post.
    Show {
        slug: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
    },
    /// Walk a daily batch of published posts whose target needs a
    /// metrics refresh (no metrics, or `sampled_at` older than
    /// `--stale-after`). Prompts interactively for each, then commits
    /// the whole batch.
    Collect {
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        /// Required — picks which venue's metrics this run refreshes.
        #[arg(long, value_enum)]
        target: Target,
        /// Posts with `sampled_at` older than this are eligible.
        /// `humantime`-shaped: `7d`, `1w`, `24h`. Default: 7 days.
        #[arg(long, value_name = "DURATION", default_value = "7d")]
        stale_after: String,
        /// Cap on how many posts to walk per invocation. Default 10
        /// gives a manageable daily rotation without forcing the
        /// whole backlog every time. Pass a large number to do all
        /// eligible in one sitting.
        #[arg(long, default_value_t = 10)]
        batch_size: usize,
        /// Skip the post-write `jj` commit + push for this invocation.
        #[arg(long)]
        no_sync: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum AnalyticsAction {
    /// Per-dimension counts and median engagement for every
    /// classification value across the workdir.
    Summary {
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        #[arg(long, value_enum)]
        target: Option<Target>,
        /// Limit to one dimension (e.g. `format`). Omit to report
        /// every dimension.
        #[arg(long)]
        dimension: Option<String>,
        /// Emit machine-readable JSON instead of formatted text.
        #[arg(long)]
        json: bool,
    },
    /// Pairwise crosstab of two dimensions plus their marginals.
    Compare {
        /// First dimension (e.g. `format`).
        dim_a: String,
        /// Second dimension (e.g. `hook`).
        dim_b: String,
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        #[arg(long, value_enum)]
        target: Option<Target>,
        /// Suppress cells with fewer than this many posts from the
        /// text output; JSON still includes them tagged `low_n`.
        #[arg(long, default_value_t = 3)]
        min_n: usize,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Surface heuristic patterns with explicit hedge language.
    Recommendations {
        /// Workdir path. Defaults to the current working directory.
        #[arg(long, value_name = "PATH")]
        workdir: Option<PathBuf>,
        #[arg(long, value_enum)]
        target: Option<Target>,
        /// Posts-per-cell threshold for "high confidence" heuristics.
        /// Below this count, observations are flagged as
        /// "insufficient data".
        #[arg(long, default_value_t = 5)]
        min_n: usize,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let jj = RealJj;
    dispatch_with_jj(cli.command, &jj)
}

/// Dispatch with a caller-provided `Jj` impl. Tests use this entry
/// point with a `FakeJj` to assert on the sync call sequence without
/// shelling out to a real `jj` binary.
pub fn dispatch_with_jj(cmd: Command, jj: &dyn Jj) -> Result<()> {
    match cmd {
        Command::Init { workdir, no_sync } => commands::init::run(jj, workdir, no_sync),
        Command::New {
            title,
            workdir,
            slug,
            kind,
            theme,
            no_sync,
        } => commands::new::run(
            jj,
            title,
            workdir_or_pwd(workdir)?,
            slug,
            kind,
            theme,
            no_sync,
        ),
        Command::List { workdir } => commands::list::run(workdir_or_pwd(workdir)?),
        Command::Show { slug, workdir } => commands::show::run(slug, workdir_or_pwd(workdir)?),
        Command::Promote {
            slug,
            workdir,
            no_sync,
        } => commands::promote::run(jj, slug, workdir_or_pwd(workdir)?, no_sync),
        Command::Demote {
            slug,
            workdir,
            no_sync,
        } => commands::demote::run(jj, slug, workdir_or_pwd(workdir)?, no_sync),
        Command::Readme { action } => match action {
            ReadmeAction::Regenerate { workdir, no_sync } => {
                commands::readme::regenerate(jj, workdir_or_pwd(workdir)?, no_sync)
            }
        },
        Command::Doctor { workdir } => commands::doctor::run(jj, workdir_or_pwd(workdir)?),
        Command::Fix {
            workdir,
            dry_run,
            no_sync,
        } => commands::fix::run(jj, workdir_or_pwd(workdir)?, dry_run, no_sync),
        Command::Update { workdir, no_sync } => {
            commands::update::run(jj, workdir_or_pwd(workdir)?, no_sync)
        }
        Command::Ai { action } => match action {
            AiAction::Ping { prompt, model } => commands::ai::ping(prompt, model),
        },
        Command::Draft {
            slug,
            workdir,
            prompt,
            model,
            no_sync,
        } => commands::draft::run(
            jj,
            commands::draft::DraftArgs {
                slug,
                workdir: workdir_or_pwd(workdir)?,
                prompt,
                model,
                no_sync,
            },
        ),
        Command::Refine {
            slug,
            workdir,
            prompt,
            model,
            no_sync,
        } => commands::refine::run(
            jj,
            commands::refine::RefineArgs {
                slug,
                workdir: workdir_or_pwd(workdir)?,
                prompt,
                model,
                no_sync,
            },
        ),
        Command::FinalEdit {
            slug,
            workdir,
            prompt,
            model,
            no_sync,
        } => commands::final_edit::run(
            jj,
            commands::final_edit::FinalEditArgs {
                slug,
                workdir: workdir_or_pwd(workdir)?,
                prompt,
                model,
                no_sync,
            },
        ),
        Command::Classify {
            slug,
            workdir,
            format,
            hook,
            tone,
            audience,
            strategic_role,
            theme,
            clear_format,
            clear_hook,
            clear_tone,
            clear_audience,
            clear_strategic_role,
            clear_theme,
            no_sync,
        } => commands::classify::run(
            jj,
            commands::classify::ClassifyArgs {
                slug,
                workdir: workdir_or_pwd(workdir)?,
                format,
                hook,
                tone,
                audience,
                strategic_role,
                theme,
                clear_format,
                clear_hook,
                clear_tone,
                clear_audience,
                clear_strategic_role,
                clear_theme,
                no_sync,
            },
        ),
        Command::AddTarget {
            slug,
            workdir,
            target,
            no_sync,
        } => commands::add_target::run(
            jj,
            commands::add_target::AddTargetArgs {
                slug,
                workdir: workdir_or_pwd(workdir)?,
                target,
                no_sync,
            },
        ),
        Command::Import {
            title,
            workdir,
            slug,
            kind,
            theme,
            target,
            url,
            published_at,
            body_file,
            tags,
            no_sync,
        } => commands::import::run(
            jj,
            commands::import::ImportArgs {
                title,
                workdir: workdir_or_pwd(workdir)?,
                slug,
                kind,
                theme,
                target,
                url,
                published_at,
                body_file,
                tags,
                no_sync,
            },
        ),
        Command::Metrics { action } => match action {
            MetricsAction::Update {
                slug,
                workdir,
                target,
                impressions,
                reactions,
                comments,
                reposts,
                sampled_at,
                no_sync,
            } => commands::metrics::update(
                jj,
                commands::metrics::UpdateArgs {
                    slug,
                    workdir: workdir_or_pwd(workdir)?,
                    target,
                    impressions,
                    reactions,
                    comments,
                    reposts,
                    sampled_at,
                    no_sync,
                },
            ),
            MetricsAction::Show { slug, workdir } => {
                commands::metrics::show(commands::metrics::ShowArgs {
                    slug,
                    workdir: workdir_or_pwd(workdir)?,
                })
            }
            MetricsAction::Collect {
                workdir,
                target,
                stale_after,
                batch_size,
                no_sync,
            } => {
                let stdin = io::stdin();
                let stdout = io::stdout();
                let mut input = io::BufReader::new(stdin.lock());
                let mut output = stdout.lock();
                commands::metrics::collect(
                    jj,
                    commands::metrics::CollectArgs {
                        workdir: workdir_or_pwd(workdir)?,
                        target,
                        stale_after,
                        batch_size,
                        no_sync,
                    },
                    &mut input,
                    &mut output,
                )
            }
        },
        Command::Backfill {
            workdir,
            import,
            no_sync,
        } => commands::backfill::run(
            jj,
            commands::backfill::BackfillArgs {
                workdir: workdir_or_pwd(workdir)?,
                import,
                no_sync,
            },
        ),
        Command::Analytics { action } => match action {
            AnalyticsAction::Summary {
                workdir,
                target,
                dimension,
                json,
            } => commands::analytics::summary(commands::analytics::SummaryArgs {
                workdir: workdir_or_pwd(workdir)?,
                target,
                dimension,
                json,
            }),
            AnalyticsAction::Compare {
                dim_a,
                dim_b,
                workdir,
                target,
                min_n,
                json,
            } => commands::analytics::compare(commands::analytics::CompareArgs {
                dim_a,
                dim_b,
                workdir: workdir_or_pwd(workdir)?,
                target,
                min_n,
                json,
            }),
            AnalyticsAction::Recommendations {
                workdir,
                target,
                min_n,
            } => commands::analytics::recommendations(commands::analytics::RecommendationsArgs {
                workdir: workdir_or_pwd(workdir)?,
                target,
                min_n,
            }),
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
            vec!["blogctl", "init", "--workdir", "/tmp/wd", "--no-sync"],
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
            vec![
                "blogctl",
                "new",
                "Hello",
                "--workdir",
                "/tmp/wd",
                "--no-sync",
            ],
            vec!["blogctl", "list", "--workdir", "/tmp/wd"],
            vec!["blogctl", "show", "hello", "--workdir", "/tmp/wd"],
            vec!["blogctl", "promote", "hello", "--workdir", "/tmp/wd"],
            vec![
                "blogctl",
                "promote",
                "hello",
                "--workdir",
                "/tmp/wd",
                "--no-sync",
            ],
            vec!["blogctl", "demote", "hello", "--workdir", "/tmp/wd"],
            vec![
                "blogctl",
                "demote",
                "hello",
                "--workdir",
                "/tmp/wd",
                "--no-sync",
            ],
            vec!["blogctl", "readme", "regenerate", "--workdir", "/tmp/wd"],
            vec![
                "blogctl",
                "readme",
                "regenerate",
                "--workdir",
                "/tmp/wd",
                "--no-sync",
            ],
            vec!["blogctl", "doctor", "--workdir", "/tmp/wd"],
            vec!["blogctl", "fix", "--workdir", "/tmp/wd"],
            vec!["blogctl", "fix", "--workdir", "/tmp/wd", "--dry-run"],
            vec!["blogctl", "fix", "--workdir", "/tmp/wd", "--no-sync"],
            vec!["blogctl", "update", "--workdir", "/tmp/wd"],
            vec!["blogctl", "update", "--workdir", "/tmp/wd", "--no-sync"],
            vec!["blogctl", "ai", "ping", "hello"],
            vec![
                "blogctl",
                "ai",
                "ping",
                "hello",
                "--model",
                "openai/gpt-4o-mini",
            ],
            // classify — every dimension flag, both set and clear shapes.
            vec!["blogctl", "classify", "hello", "--workdir", "/tmp/wd"],
            vec![
                "blogctl",
                "classify",
                "hello",
                "--workdir",
                "/tmp/wd",
                "--format",
                "thesis",
                "--hook",
                "contradiction",
                "--tone",
                "sharp",
                "--audience",
                "engineering",
                "--strategic-role",
                "career-brand",
                "--theme",
                "ambiguity,delivery",
            ],
            vec![
                "blogctl",
                "classify",
                "hello",
                "--workdir",
                "/tmp/wd",
                "--clear-format",
                "--clear-hook",
                "--clear-tone",
                "--clear-audience",
                "--clear-strategic-role",
                "--clear-theme",
            ],
            vec![
                "blogctl",
                "classify",
                "hello",
                "--workdir",
                "/tmp/wd",
                "--no-sync",
            ],
            // metrics — update + show.
            vec![
                "blogctl",
                "metrics",
                "update",
                "hello",
                "--workdir",
                "/tmp/wd",
                "--target",
                "linkedin",
                "--impressions",
                "1842",
                "--reactions",
                "67",
                "--comments",
                "14",
                "--reposts",
                "5",
            ],
            vec![
                "blogctl",
                "metrics",
                "update",
                "hello",
                "--workdir",
                "/tmp/wd",
                "--target",
                "linkedin",
                "--impressions",
                "1",
                "--reactions",
                "0",
                "--comments",
                "0",
                "--reposts",
                "0",
                "--sampled-at",
                "2026-05-14T00:00:00Z",
                "--no-sync",
            ],
            vec![
                "blogctl",
                "metrics",
                "show",
                "hello",
                "--workdir",
                "/tmp/wd",
            ],
            // import — backfill an already-published post.
            vec![
                "blogctl",
                "import",
                "--title",
                "Hello",
                "--workdir",
                "/tmp/wd",
                "--target",
                "linkedin",
                "--url",
                "https://www.linkedin.com/posts/x",
                "--published-at",
                "2026-05-08T14:32:00Z",
                "--body-file",
                "/tmp/body.md",
            ],
            vec![
                "blogctl",
                "import",
                "--title",
                "Hello",
                "--workdir",
                "/tmp/wd",
                "--slug",
                "custom-slug",
                "--kind",
                "article",
                "--theme",
                "parable",
                "--target",
                "blog",
                "--url",
                "https://example.com/x",
                "--published-at",
                "2026-05-08T14:32:00Z",
                "--body-file",
                "-",
                "--tag",
                "rust",
                "--tag",
                "blogctl",
                "--no-sync",
            ],
            // backfill — interactive + import.
            vec!["blogctl", "backfill", "--workdir", "/tmp/wd"],
            vec![
                "blogctl",
                "backfill",
                "--workdir",
                "/tmp/wd",
                "--import",
                "/tmp/data.json",
                "--no-sync",
            ],
            // analytics — every action.
            vec!["blogctl", "analytics", "summary", "--workdir", "/tmp/wd"],
            vec![
                "blogctl",
                "analytics",
                "summary",
                "--workdir",
                "/tmp/wd",
                "--target",
                "linkedin",
                "--dimension",
                "format",
                "--json",
            ],
            vec![
                "blogctl",
                "analytics",
                "compare",
                "format",
                "hook",
                "--workdir",
                "/tmp/wd",
            ],
            vec![
                "blogctl",
                "analytics",
                "compare",
                "format",
                "hook",
                "--workdir",
                "/tmp/wd",
                "--target",
                "blog",
                "--min-n",
                "5",
                "--json",
            ],
            vec![
                "blogctl",
                "analytics",
                "recommendations",
                "--workdir",
                "/tmp/wd",
            ],
            vec![
                "blogctl",
                "analytics",
                "recommendations",
                "--workdir",
                "/tmp/wd",
                "--target",
                "linkedin",
                "--min-n",
                "10",
            ],
            vec!["blogctl", "completions", "bash"],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_ok(), "{args:?}");
        }
    }

    #[test]
    fn metrics_update_rejects_unknown_target() {
        let args = vec![
            "blogctl",
            "metrics",
            "update",
            "hello",
            "--workdir",
            "/tmp/wd",
            "--target",
            "mastodon",
            "--impressions",
            "1",
            "--reactions",
            "0",
            "--comments",
            "0",
            "--reposts",
            "0",
        ];
        assert!(Cli::try_parse_from(args).is_err());
    }

    #[test]
    fn metrics_update_requires_every_count_flag() {
        // Missing --reactions, --comments, --reposts.
        let args = vec![
            "blogctl",
            "metrics",
            "update",
            "hello",
            "--workdir",
            "/tmp/wd",
            "--target",
            "linkedin",
            "--impressions",
            "1",
        ];
        assert!(Cli::try_parse_from(args).is_err());
    }

    #[test]
    fn analytics_summary_rejects_unknown_target() {
        let args = vec![
            "blogctl",
            "analytics",
            "summary",
            "--workdir",
            "/tmp/wd",
            "--target",
            "twitter",
        ];
        assert!(Cli::try_parse_from(args).is_err());
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
    fn init_requires_explicit_workdir() {
        // `init` creates the workdir layout; defaulting to PWD would
        // scatter stage dirs into wherever the user happens to be.
        // Every other subcommand defaults to PWD (see
        // `workdir_defaults_to_pwd_when_omitted`).
        assert!(Cli::try_parse_from(["blogctl", "init"]).is_err());
    }

    #[test]
    fn workdir_defaults_to_pwd_when_omitted() {
        // Every non-`init` subcommand should parse successfully without
        // an explicit `--workdir`; the dispatcher resolves the missing
        // value to `std::env::current_dir()` at run time.
        for args in [
            vec!["blogctl", "new", "Hello"],
            vec!["blogctl", "list"],
            vec!["blogctl", "show", "hello"],
            vec!["blogctl", "promote", "hello"],
            vec!["blogctl", "demote", "hello"],
            vec!["blogctl", "readme", "regenerate"],
            vec!["blogctl", "doctor"],
            vec!["blogctl", "fix"],
            vec!["blogctl", "update"],
            vec!["blogctl", "classify", "hello"],
            vec![
                "blogctl",
                "metrics",
                "update",
                "hello",
                "--target",
                "linkedin",
                "--impressions",
                "1",
                "--reactions",
                "0",
                "--comments",
                "0",
                "--reposts",
                "0",
            ],
            vec!["blogctl", "metrics", "show", "hello"],
            vec![
                "blogctl",
                "import",
                "--title",
                "Hello",
                "--target",
                "linkedin",
                "--url",
                "https://www.linkedin.com/posts/x",
                "--published-at",
                "2026-05-08T14:32:00Z",
                "--body-file",
                "/tmp/body.md",
            ],
            vec!["blogctl", "backfill"],
            vec!["blogctl", "analytics", "summary"],
            vec!["blogctl", "analytics", "compare", "format", "hook"],
            vec!["blogctl", "analytics", "recommendations"],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_ok(), "{args:?}");
        }
    }
}
