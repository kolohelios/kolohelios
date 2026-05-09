# `shaka`

Build and repository tooling for the kolohelios monorepo. `shaka` is a Rust
command-line tool that consolidates the workflows that would otherwise
live as a collection of shell scripts: project metadata validation,
`justfile` generation, commit linting, `jj`/PR helpers, `jj` workspace
management, and the single CI gate (`shaka preflight`).

## Running

Inside any project's devshell (`direnv` / `nix develop`), `shaka` is
on `$PATH` and resolves from any `cwd`:

```
shaka <subcommand>
```

The shim ships from `kolohelios-nix.lib.workflowPackages`. For
cold-start invocations (outside any devshell), call the wrapper
directly from the repo root:

```
tools/shaka/bin/shaka <subcommand>
```

The wrapper enters this project's nix devshell so `shaka` inherits its
runtime dependencies (`cue`, `jj`, `git`, `just`, `jq`, `cargo-llvm-cov`
on Linux), then runs an incremental `cargo build` (no-op when nothing has
changed) and exec's the debug binary.

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
- `repo sync|send|pr|audit|status` — `jj`/PR workflow helpers.
- `workspace new|list|forget|status|cleanup` — `jj` workspace management
  for parallel work.

See `shaka <subcommand> --help` for full options.

## Auditing other repositories

`shaka repo audit` is a generic GitHub repo-settings auditor: any repo
that wants to use it authors a `.shaka/repo.cue` describing its desired
GitHub configuration (default branch, merge modes, branch protection
status checks, `ruleset` rules, `Dependabot` toggle), and the command
reports drift between the policy and the live repo. `--fix` PATCHes the
divergent settings back to policy values. The schema lives at
`tools/shaka/schema/repo-policy-schema.cue` in this repo; optional
groups (`branchProtection`, `rulesets`, `security`, `issues`) can be
omitted entirely to opt out of audit dimensions a particular repo
doesn't enforce — typical for personal repos without branch protection.

Outside the kolohelios devshell, run `shaka` directly from FlakeHub:

```
nix run https://flakehub.com/f/kolohelios/shaka/*.tar.gz -- repo audit
```

Run from anywhere inside the target repo's working copy; the loader
walks upward looking for `.shaka/repo.cue`. The wrapped binary already
ships `cue`, `jj`, `git`, and `gh` on its `PATH`, so external consumers
don't need a devshell. Pass `--repo owner/name` if `jj git remote list`
can't auto-detect the GitHub repo (for example, plain-git checkouts
without a colocated `.jj`).

## Development

```
nix develop . --command just validate
```

runs the same fmt/clippy/test/coverage/flake checks CI runs for this project.

## Error handling

`shaka` uses [`snafu`](https://docs.rs/snafu) for all per-module error
types. New subcommands and modules should follow the same pattern rather
than rolling a hand-written struct + `Display` + `From<io::Error>` `impl`s
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
- Mark the `enum` `#[snafu(visibility(pub(crate)))]` so cross-module callers
  can construct the context selectors when they shell out via the same
  helper but need to fail with the wrapped error type.
