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

## Status

Auth is implemented (#476). Task operations are still stubs;
follow-up issues (#477–#479) fill them in.
