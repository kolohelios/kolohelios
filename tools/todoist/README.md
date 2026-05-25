# `todoist`

Command-line client for `Todoist`. Wraps the `Todoist` REST API for the
operations that come up often enough to want a keyboard shortcut:
listing tasks, adding tasks, completing tasks.

## Auth

The `Todoist` API token is never stored on disk in plaintext. Instead,
`todoist` stores a 1Password secret reference (an `op://` URI) and
resolves it on demand via the `op` command-line tool.

```
todoist auth login --op-ref "op://Personal/Todoist API/credential"
todoist auth status
todoist auth logout
```

The `config.toml` file lives at `$XDG_CONFIG_HOME/todoist/config.toml`
(defaulting to `~/.config/todoist/config.toml`) and contains only
the reference — never the token. `auth login` rejects a reference
that `op read` cannot resolve, so a bad ref fails fast instead of
turning into a surprise 401 later.

## Listing tasks

```
todoist tasks list [--project <name-or-id>] [--filter <todoist-filter>] \
                   [--limit <N>] [--json]
```

`--project` accepts either a project name (resolved via `/projects`)
or a numeric ID. `--filter` passes through the `Todoist` filter query
language verbatim (`today`, `overdue`, `@waiting`, etc.). `--json`
emits one raw API object per line for piping into `jq`; without it
the output is a column table whose `id` shows the first six characters
of the task ID for quick reference.

## Adding tasks

```
todoist tasks add <content> [--project <name-or-id>] \
                            [--due <natural-language>] \
                            [--priority 1|2|3|4] \
                            [--label <name>]... \
                            [--description <text>]
```

`--due` passes through to `Todoist`'s natural-language parser
(`tomorrow at 3pm`, `every monday`, etc.). Priority is `1` (lowest)
through `4` (highest) — out-of-range values are rejected before the
request reaches the server. Repeating `--label` attaches multiple
labels at once.

## Completing tasks

```
todoist tasks complete <id-or-prefix>...
```

Each argument is matched first against an exact task ID, then against
the six-character ID prefix (as shown by `tasks list`). Ambiguous
prefixes print the matching candidates so the next attempt can
disambiguate. A partial failure across multiple tasks still attempts
the rest of the batch and exits non-zero only after all results are
printed. Output uses the Unicode tick `✓` on a TTY, falling back to
plain `ok`/`x` when `stdout` is redirected or `NO_COLOR` is set.

## Status

Basic CRUD (auth, list, add, complete) is implemented. Future
workflows are tracked in #480.
