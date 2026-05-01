# kolohelios

Personal monorepo for infrastructure, tooling, and projects. This file is
project-specific guidance for AI-assisted development. Personal/global
preferences live in `~/.claude/CLAUDE.md`.

## Tenets

A few principles shape how we work in this repo. Reach back to them when
scoping or sequencing.

- **Solo developer for the foreseeable future.** Keep processes
  lightweight. Don't suggest contributing guides, multi-developer review
  workflows, or collaboration tooling. Prefer direct edits over RFCs;
  prefer issue comments over docs.
- **Devboxes are ephemeral.** Local devboxes (baremetal mac, cloud VM)
  are ephemeral workspaces, not durable infrastructure. Durable
  artifacts are: the code repo, flake caches, deployed services. Config
  changes live in version control. Don't propose stateful devbox
  patterns; the cost to rebuild a devbox should stay close to zero.
- **Test the simpler hypothesis before architecting a replacement.**
  When a task is framed as "replace X to avoid Y," ask first whether X
  is still load-bearing. Especially true for CI cleanup steps, caching
  workarounds, retry loops, and other defensively-added plumbing. The
  cheapest experiment is to remove and see, before designing the
  replacement.

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
| `nix/`       | Shared nix infrastructure (e.g. `nix/kolohelios-nix`) |

Every project has a `project.cue` declaring its `name` and `kind` (validated
against `tools/shaka/schema/project.cue`). Per-project `justfile`s are
**generated** by `shaka project generate-justfiles` — never edit them by
hand. CI fails on drift.

## Build system

- **Nix flakes are per-project.** Each project owns its toolchain via its
  own `flake.nix`. There is **no root flake**. `direnv` enters the
  appropriate flake when you `cd` into a project (each has a `.envrc` with
  `use flake`).
- **`kolohelios-nix`** (`nix/kolohelios-nix/`) is the shared lib every
  consumer flake imports as a `path:` input, with
  `nixpkgs.follows = "kolohelios-nix/nixpkgs"`. It exports
  `lib.forEachSupportedSystem`, `lib.workflowPackages`, and `formatter` so
  consumers stay thin. Published to FlakeHub by the `build-nix-lib` CI job.
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
   (or all, with no `--since`), enters the project's flake (`nix develop
   . --command`) and runs `just validate`. Per-project quality gates (fmt,
   lint, test, coverage, `nix flake check`, etc.) live in the generated
   `justfile`'s `validate` recipe.

If you need a new per-project check, extend the appropriate template in
`tools/shaka/src/project/generate_justfiles.rs` so the generated
`validate` recipe picks it up. If it spans projects, add it to `CHECKS`.
Either way, do **not** add a new GitHub Actions job.

### Running `shaka`

`shaka` is **not** on `$PATH` globally. Always invoke it via the wrapper:

```
tools/shaka/bin/shaka <subcommand>
```

The wrapper always re-enters `nix develop ./tools/shaka` (unless already
inside, detected via the `IN_SHAKA_DEVSHELL` marker) so shaka inherits all
its runtime dependencies (cue, jj, git, just, jq, cargo-llvm-cov on Linux,
etc.) regardless of how it was invoked. It then runs an incremental `cargo
build` (free when no source has changed) and exec's the resulting debug
binary. Don't reach for `cargo run` or `nix run ./tools/shaka` — the
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

### Pull requests

- **PR body is a brief summary paragraph — no test plan section.**
  `shaka preflight` gates correctness; long PR bodies add noise without
  value.
- **PR title comes from your latest commit's title** — `shaka repo send`
  propagates the `@` commit title and body straight to the PR. Plan the
  tip commit's message accordingly.
- **If the change closes an issue, include `Closes #<N>` in the commit
  body.** GitHub auto-links and auto-closes on merge. Sub-agents don't
  have the `/ship` skill that bakes this in — include it explicitly in
  any sub-agent brief. (#146 will eventually have `shaka commit lint`
  catch this automatically.)

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
numbers; use `gh issue view <n>` to read them.

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
       kind: "rust" | "infra" | "nix-lib"  // pick one
       // rust: also requires a `coverage:` block (see schema)
   }
   ```
2. Run `shaka project schema-check` to confirm the schema accepts it.
3. Run `shaka project generate-justfiles` to produce the per-project `justfile`.
4. Add the project's source files and any flake inputs.
5. If the project introduces new preflight checks, add them to
   `tools/shaka/src/preflight.rs` rather than to CI YAML.

## Parallel work in jj workspaces

For non-trivial work that splits into independent slices (typically a
parent issue with N sibling sub-issues), `shaka workspace` lets you run
multiple Claude Code sessions or sub-agents in parallel without
trampling each other.

Shape:

1. `shaka workspace new --issue <N>` (or `shaka workspace new <name>`)
   creates a sibling working copy at `../kolohelios-i<N>` that shares
   the same `.jj/repo` but has its own `@`. Each workspace has a
   filesystem-level full copy of the repo (~66 files) but the underlying
   commit storage is shared.
2. Run `claude` inside each workspace, or spawn sub-agents pointed at
   each workspace's path. Each session bookmarks, commits, and pushes
   independently. Concurrent jj operations are serialized via the repo
   lock — light contention at worst.
3. As PRs land, rebase the remaining branches on the new `main@origin`.
   Until #147 (`shaka repo rebase-wip`) lands, this is manual:
   `jj rebase --branch <bookmark> -d main@origin` per branch, then
   `jj git push --bookmark <name>` (jj allows the non-fast-forward
   "move sideways" without `--force` for rebased branches).
4. `shaka workspace status` shows a per-workspace summary at any time.
5. `shaka workspace cleanup` forgets workspaces whose PRs have merged.
   Caveat: this repo has `deleteBranchOnMerge: true`, so after `repo
   sync` the local bookmark is gone (jj propagates the remote
   deletion) and `cleanup`'s bookmark-based lookup misses the
   workspace. Fall back to `shaka workspace forget --force <name>`
   per workspace until #164 lands.

When briefing sub-agents to work in their own workspaces, the brief must
restate two rules that `/start` and `/ship` would otherwise enforce:

- **The standard PR conventions apply** — including `Closes #<N>` in the
  tip commit body. See [Pull requests](#pull-requests). Sub-agents don't
  have `/ship` to bake this in; the brief must require it explicitly.
- **Re-run `nix develop . --command just validate` immediately before
  pushing.** It's easy to make a final tweak after a successful validate
  and forget to re-run; the resulting CI failure cycle is far more
  expensive than the local validate.

## Things to avoid

- **Don't edit generated `justfile`s.** They carry a "Do not edit by hand"
  header and CI fails on drift. Change
  `tools/shaka/src/project/generate_justfiles.rs` instead.
- **Don't add CI jobs to `.github/workflows/main.yaml`** for new validation
  steps — extend `shaka preflight` so CI and local stay in lockstep.
- **Don't add Claude Code attribution** to commits, code, or docs.
- **Don't run mutating `git` commands.** The repo is jj-colocated; `git
  stash`, `git stash pop`, `git checkout`, `git reset`, `git commit`,
  `git rebase`, `git merge` all desync the working copy from jj's view.
  Read-only `git` (`status`, `log`, `diff`) is fine. To test "what does
  main look like without my changes," use `jj new main@origin` (the
  original change is preserved and reachable via `jj log` / `jj edit`);
  to inspect a previous state, use `jj op log` and `jj op restore`.
  Use `shaka repo sync` for the rebase-on-`main@origin` flow.
- **Don't add thin pass-through wrappers.** Don't add justfile targets,
  scripts, or aliases whose only job is to forward to another tool. If
  `shaka preflight` is the entry point, document and call it directly —
  don't put a layer in front whose only purpose is to advertise it.
  Per-project justfiles with multiple real recipes (`build`, `test`,
  `lint`, `validate`) are not pass-throughs — those are aggregations.
- **Don't embed external-format fixtures as string literals in Rust
  tests.** When testing code that processes external file formats (CUE,
  YAML, JSON, etc.) by shelling out to tooling, write fixtures as real
  files in `<crate>/tests/fixtures/<topic>/{valid,invalid}/...` and walk
  them from integration tests. Real files are syntax-highlighted, can be
  inspected directly with the underlying tool (e.g.
  `cue vet schema fixture`), and are the actual artifacts shipped in the
  repo. Adding coverage = adding a fixture file, not editing test code.
  See `tools/shaka/tests/schema.rs` for the pattern.
- **Don't push without re-running `just validate` after your last edit.**
  Even if validate passed earlier in your session, a final tweak can
  introduce `cargo fmt` or `clippy` violations. The CI failure cycle is
  slow; the immediately-before-push validate is the cheap insurance.
