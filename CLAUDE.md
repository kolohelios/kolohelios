# kolohelios

Personal monorepo for infrastructure, tooling, and projects. This file is
project-specific guidance for AI-assisted development. Personal/global
preferences live in `~/.claude/CLAUDE.md`.

## Layout

Top-level directories are **slots**; each slot contains one directory per
project. Project-local docs and configuration live alongside the project.

| Slot         | Purpose                                              |
| ------------ | ---------------------------------------------------- |
| `apps/`      | End-user applications                                |
| `packages/`  | Shared libraries                                     |
| `projects/`  | Standalone projects that don't fit another slot      |
| `services/`  | Long-running services (reserved; not yet populated)  |
| `tools/`     | Developer tooling (e.g. `tools/shaka`)               |
| `infra/`     | Infrastructure as code (e.g. `infra/devbox`)         |

Every project has a `project.cue` declaring its `name` and `kind` (validated
against `tools/shaka/schema/project.cue`). Per-project `justfile`s are
**generated** by `shaka project generate-justfiles` — never edit them by
hand. CI fails on drift.

## Build system

- **Nix flakes are per-project.** Each project owns its toolchain via its
  own `flake.nix`. There is **no root flake**. `direnv` enters the
  appropriate flake when you `cd` into a project (each has a `.envrc` with
  `use flake`). Today:
  - `tools/shaka/flake.nix` — rust nightly (via rust-overlay) plus workflow
    tools (cue, jj, git, just, jq, nixfmt, nil) used by shaka and by
    workflows at the repo root. On Linux, also includes `cargo-llvm-cov`.
  - `infra/devbox/flake.nix` — opentofu, linode-cli, just, plus
    `nixosConfigurations.devbox`, the `linodeImage` package, and the
    devbox-eval check.
- **`just`** as the command runner for **per-project** recipes (`build`,
  `test`, `fmt-check`, `lint`, `validate`). There is no cross-project root
  justfile — for repo-wide validation, run `shaka preflight` directly.
- **`shaka`** (Rust CLI in `tools/shaka`) is the build/repo Swiss army knife:
  - `shaka preflight` — runs every CI check locally; CI runs the same command,
    so local and CI cannot drift. `--since <ref>` scopes checks to changed
    paths.
  - `shaka project generate-justfiles` — regenerates root and per-project
    `justfile`s from each `project.cue`. `--check` fails on drift (used in
    CI).
  - `shaka project schema-check` — validates every `project.cue` against
    the schema.
  - `shaka commit lint -r <revset>` — enforces conventional commit format,
    title length, body wrap, and atomicity (warns on cross-project commits).
  - `shaka repo sync|send|pr|audit` — jj/PR workflow helpers.

The single CI gate is `shaka preflight`. It runs in two phases:

1. **Repo-level checks** (`CHECKS` in `tools/shaka/src/preflight.rs`) — work
   that spans projects: `shaka project schema-check`, `shaka project
   generate-justfiles --check`. (No `nix flake check` here — flake checks
   are per-project, covered below.)
2. **Per-project checks** — for each project whose files changed
   (or all, with no `--since`), runs `just validate` in the project's
   directory. Per-project quality gates (fmt, lint, test, coverage,
   `nix flake check`, etc.) live in the generated `justfile`'s `validate`
   recipe.

If you need a new per-project check, extend the appropriate template in
`tools/shaka/src/project/generate_justfiles.rs` so the generated
`validate` recipe picks it up. If it spans projects, add it to `CHECKS`.
Either way, do **not** add a new GitHub Actions job.

### Running `shaka`

`shaka` is **not** on `$PATH` globally. Always invoke it via the wrapper
script:

```
tools/shaka/bin/shaka <subcommand>
```

The wrapper always re-enters `nix develop ./tools/shaka` (unless already
inside it, detected via the `IN_SHAKA_DEVSHELL` marker) so shaka inherits
all its runtime dependencies (cue, jj, git, just, jq, etc.) regardless of
how it was invoked. It then runs an incremental `cargo build` (free when no
source has changed) and exec's the resulting debug binary, so it works from
any cwd. Don't reach for `cargo run` or `nix run ./tools/shaka` — the
wrapper subsumes both.

## Version control

- **Jujutsu (`jj`)** for all VCS operations — never `git` for working-copy
  changes. The repo is colocated, so `git` reads work, but mutations should go
  through `jj`.
- **Conventional commits**, enforced by `shaka commit lint`:
  - Title: `<type>(<scope>): <subject>` (max 70 chars, declarative — no
    `Generated`/`Co-Authored-By` trailers)
  - Types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `style`, `perf`,
    `ci`, `build`
  - Scope is usually the project name (`shaka`, `infra/devbox`) or `ci`/`build`
    for cross-cutting changes
  - Body wrapped at 80 chars; explains *why*, not *what*
- **Atomic, vertical commits**: one logical change per commit, one
  layer/concern per commit. `shaka commit lint` warns when a commit spans
  multiple projects.
- **Workflow**: rebase on `main@origin`, self-review, then push and open a PR.
  `shaka repo send` automates push + PR creation.

### Working with jj

A few jj behaviors trip up agents whose mental model comes from git. Read
this before scripting against `jj`:

- **Change IDs come in two lengths.** Templates emit the 32-char form by
  default (`utxssoyuknns...`); the 12-char prefix shown in `jj log`
  (`utxssoyuknns`) only resolves while the change exists. Use
  `change_id.short()` in templates to get the prefix explicitly.
- **Empty `@` auto-abandons.** `jj new <ref>` from an empty `@` switches
  `@` and abandons the empty change. To move `@` without creating a new
  change, use `jj edit <rev>`.
- **Bookmarks track change_id, not commit_id.** `jj describe @` rewrites
  the commit but the bookmark moves with the change — you rarely need to
  re-set a bookmark after editing.
- **`jj restore <path>` resolves paths relative to the repo root**, not
  the cwd. Pass absolute paths from automation.
- **Useful revsets**: `main@origin..@` (commits ahead of remote main),
  `main@origin..@ ~ empty()` (same, excluding empty), `@-` (parent),
  `roots(...)`, `heads(...)`.
- **`jj op log` is the source of truth for past operations** — fetches,
  rebases, snapshots. Filter by first-line `description` content in
  templates when scripting.
- **`jj diff --summary -r @`** prefixes each path with a single letter
  (`A`/`M`/`D`/`R`/`C`); stable enough to parse.
- **`jj git push --allow-new`** (or `-N`) is required the first time a
  bookmark goes to origin; without it, push fails for unknown bookmarks.
- **Colocated repos** keep git refs in `.git/`;
  `.jj/repo/store/git_target` points at the git dir. Useful when poking
  at `FETCH_HEAD` or pack files directly.

## Issue tracking

Work is tracked in **GitHub Issues**. References like `#21` are GitHub issue
numbers; use `gh issue view <n>` to read them. The repo is private and does
not use GitHub Advanced Security — do not propose features that depend on
GHAS (code scanning, secret scanning Push Protection, etc.).

## Secrets

**1Password** is the canonical secret store for local development (`op` CLI),
CI (GitHub Actions integration), and infrastructure. Never commit secrets,
never propose `.env` files checked into the repo.

## Adding a new project

Until `shaka project new` lands (see issue #23), add a project manually:

1. Create `<slot>/<name>/project.cue`:
   ```cue
   package project

   #Project & {
       name: "<name>"
       kind: "rust" | "infra"  // pick one
   }
   ```
2. Run `shaka project schema-check` to confirm the schema accepts it.
3. Run `shaka project generate-justfiles` to produce the per-project `justfile`.
4. Add the project's source files and any flake inputs.
5. If the project introduces new preflight checks, add them to
   `tools/shaka/src/preflight.rs` rather than to CI YAML.

## Things to avoid

- **Don't edit generated `justfile`s.** They carry a "Do not edit by hand"
  header and CI fails on drift. Change
  `tools/shaka/src/project/generate_justfiles.rs` instead.
- **Don't add CI jobs to `.github/workflows/main.yaml`** for new validation
  steps — extend `shaka preflight` so CI and local stay in lockstep.
- **Don't add Claude Code attribution** to commits, code, or docs.
- **Don't propose GHAS-dependent features** (this repo is private, no
  Enterprise).
- **Don't reach for `git commit`/`git rebase`** — use `jj` (and `shaka repo
  sync` for the rebase-on-`main@origin` flow).
