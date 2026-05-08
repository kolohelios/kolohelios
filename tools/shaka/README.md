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
- `project audit` — structural audit (README/.gitignore presence, rust test
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

## Error handling

Shaka uses [`snafu`](https://docs.rs/snafu) for all per-module error
types. New subcommands and modules should follow the same pattern rather
than rolling a hand-written struct + `Display` + `From<io::Error>` impls
(the form every existing error used before #252).

A new error type looks like:

```rust
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum MyError {
    #[snafu(display("failed to run foo: {source}"))]
    Spawn { source: std::io::Error },

    #[snafu(display("foo {command}: {stderr}"))]
    FooCommand { command: String, stderr: String },

    #[snafu(display("failed to parse JSON from {context}: {source}"))]
    JsonParse {
        context: String,
        source: serde_json::Error,
    },
}
```

Conventions:

- One variant per *user-distinguishable* failure mode, not per call site.
  Two call sites that produce identical user messages collapse into one
  variant.
- Wrap underlying errors with `source: SomeError` rather than baking them
  into a `format!` string. `std::error::Error::source()` then walks the
  cause chain — see
  `object_store::registry::tests::parse_project_error_exposes_serde_source`
  for the canonical test.
- Use `.context(SomeSnafu { ... })` for eager arguments,
  `.with_context(|_| SomeSnafu { ... })` when the context selector
  contains a `format!` call (so the allocation only happens on the error
  path).
- Mark the enum `#[snafu(visibility(pub(crate)))]` so cross-module callers
  can construct the context selectors when they shell out via the same
  helper but need to fail with the wrapped error type.
