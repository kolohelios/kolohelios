# shaka

Build and repository tooling for the kolohelios monorepo. `shaka` is a Rust
CLI that consolidates the workflows that would otherwise live as a
collection of shell scripts: project metadata validation, justfile
generation, commit linting, jj/PR helpers, jj workspace management, and the
single CI gate (`shaka preflight`).

## Running

`shaka` is not on `$PATH` globally. Always invoke it via the wrapper:

```
tools/shaka/bin/shaka <subcommand>
```

The wrapper enters this project's nix devshell so shaka inherits its runtime
dependencies (cue, jj, git, just, jq, cargo-llvm-cov on Linux), then runs an
incremental `cargo build` (no-op when nothing has changed) and exec's the
debug binary.

## Subcommands

- `preflight` — runs every CI check locally; CI runs the same command, so
  local and CI cannot drift. `--since <ref>` scopes checks to changed paths.
- `project schema-check` — validates every `project.cue` against the shared
  CUE schema.
- `project generate-justfiles [--check]` — regenerates per-project
  `justfile`s from each `project.cue`. `--check` fails on drift.
- `project lint` — structural lints (README/.gitignore presence, rust test
  presence, coverage threshold sanity).
- `commit lint -r <revset>` — enforces conventional commit format, title
  length, body wrap, and atomicity.
- `repo sync|send|pr|audit|status` — jj/PR workflow helpers.
- `workspace new|list|forget|status|cleanup` — jj workspace management for
  parallel work.

See `tools/shaka/bin/shaka <subcommand> --help` for full options.

## Development

```
nix develop . --command just validate
```

runs the same fmt/clippy/test/coverage/flake checks CI runs for this project.
