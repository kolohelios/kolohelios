# kolohelios

Personal monorepo for infrastructure, tooling, and projects. This file is
project-specific guidance for AI-assisted development. Personal/global
preferences live in `~/.claude/CLAUDE.md`.

## Tenets

A few principles shape how work happens in this repo. Reach back to
them when scoping or sequencing.

- **Solo developer for the foreseeable future.** Keep processes
  lightweight. Don't suggest contributing guides, multi-developer review
  workflows, or collaboration tooling. Prefer direct edits over RFCs;
  prefer issue comments over docs.
- **Devboxes are ephemeral.** Local devboxes (baremetal mac, cloud VM)
  are ephemeral workspaces, not durable infrastructure. Durable
  artifacts are: the code repo, flake caches, deployed services.
  Configuration changes live in version control. Don't propose stateful
  devbox patterns; the cost to rebuild a devbox should stay close to
  zero.
- **Test the simpler hypothesis before architecting a replacement.**
  When a task is framed as "replace X to avoid Y," ask first whether X
  is still load-bearing. Especially true for CI cleanup steps, caching
  workarounds, retry loops, and other defensively added plumbing. The
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
| `tools/`     | Developer tooling (for example, `tools/shaka`)       |
| `infra/`     | Infrastructure as code (for example, `infra/devbox`) |
| `nix/`       | Shared nix infrastructure (for example, `nix/kolohelios-nix`) |

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
  consumer flake imports via the FlakeHub URL
  (`https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz`), with
  `nixpkgs.follows = "kolohelios-nix/nixpkgs"`. It exports
  `lib.forEachSupportedSystem`, `lib.workflowPackages`, and `formatter` so
  consumers stay thin. Published to FlakeHub by the `build-nix-lib` CI job.
  The audit rule `kolohelios-nix-via-flakehub` (in `shaka project audit`)
  enforces the FlakeHub URL form so no consumer drifts back to a `path:`
  input.
- **`just`** as the command runner for **per-project** recipes (`build`,
  `test`, `fmt-check`, `lint`, `validate`). There is no cross-project root
  `justfile` — for repo-wide validation, run `shaka preflight` directly.
- **`shaka`** (Rust command-line tool in `tools/shaka`) is the build/repo Swiss army knife:
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
  - `shaka repo sync|send|pr|audit` — `jj`/PR workflow helpers.

The single CI gate is `shaka preflight`. It runs in two phases:

1. **Repo-level checks** (`CHECKS` in `tools/shaka/src/preflight.rs`) — work
   that spans projects: `shaka project schema-check`, `shaka project
   generate-justfiles --check`. (No `nix flake check` here — flake checks
   are per-project, covered below.)
2. **Per-project checks** — for each project whose files changed
   (or all, with no `--since`), enters the project's flake (`nix develop
   . --command`) and runs `just validate`. Per-project quality gates
   (`fmt`, lint, test, coverage, `nix flake check`, etc.) live in the
   generated `justfile`'s `validate` recipe.

If you need a new per-project check, extend the appropriate template in
`tools/shaka/src/project/generate_justfiles.rs` so the generated
`validate` recipe picks it up. If it spans projects, add it to `CHECKS`.
Either way, do **not** add a new GitHub Actions job.

The exception is automation that responds to repo events rather than
gating PRs — for example, `auto-rebase-prs.yaml` rebases open PRs on
`push: main`. These can't live in `shaka preflight` because they don't gate
the current change set. Two things to remember when adding one:

- **`permissions: id-token: write`** is required on the job if it
  runs `nix develop` — `DeterminateSystems/nix-installer-action`
  mints an OIDC token to authenticate with FlakeHub for private
  inputs like `kolohelios-nix`, and fails without it.
- **Force-push or status-write operations need a GitHub App token,
  not `GITHUB_TOKEN`.** Pushes via `GITHUB_TOKEN` don't re-trigger CI
  on the destination branch; this is a documented GitHub limitation.
  See `.github/auto-rebase-app.md` for the `kolohelios-bot` setup
  pattern (mint via `actions/create-github-app-token@v3`, pass to
  `actions/checkout` and `GH_TOKEN`).

### Running `shaka`

Inside any project's devshell (the common case via `direnv` or
`nix develop`), `shaka` is on `$PATH` and resolves from any `cwd`:

```
shaka <subcommand>
```

That comes from a tiny shim in `kolohelios-nix.lib.workflowPackages`
that walks up from `cwd` to find the canonical wrapper at
`tools/shaka/bin/shaka` and exec's it.

Outside a devshell (cold-start scripts), invoke the wrapper directly
from the repo root:

```
tools/shaka/bin/shaka <subcommand>
```

The wrapper always re-enters `nix develop ./tools/shaka` (unless already
inside, detected via the `IN_SHAKA_DEVSHELL` marker) so `shaka` inherits
all its runtime dependencies (`cue`, `jj`, `git`, `just`, `jq`,
`cargo-llvm-cov` on Linux, etc.) regardless of how it was invoked. It
then runs an incremental `cargo build` (free when no source has changed)
and exec's the resulting debug binary. Don't reach for `cargo run` or
`nix run ./tools/shaka` — the wrapper subsumes both.

## Version control

- **`Jujutsu` (`jj`)** for all VCS operations — never `git` for working-copy
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
  any sub-agent brief. (#146 tracks teaching `shaka commit lint` to
  catch this automatically.)

### Auto-rebase on `main` movement

When `main@origin` moves, `.github/workflows/auto-rebase-prs.yaml`
rebases every open PR whose base is `main` onto the new tip. Successful
rebases force-push the PR branch (with `--force-with-lease`) and post a
`success` `auto-rebase` commit status; conflicts post a `failure`
status on the PR head describing the conflicting paths, and the
workflow run goes red.

- **Opt out with the `do-not-rebase` label.** Apply it to a PR you
  don't want the bot touching (for example, one you're actively
  rebasing yourself).
- **`auto-rebase` is informational, not a required check.** It only
  appears on a PR after `main` has moved — making it required would
  block every freshly opened PR. Its job is to flag conflicts the
  author needs to resolve, not to gate merge.
- **After a bot rebase, run `jj git fetch` locally.** The bookmark
  tracks the `change_id`, so `jj` reconciles automatically: the local
  bookmark moves to the rebased commit and your `@` (if it was on the
  bookmark) follows. To resolve a conflict the bot couldn't, run
  `jj rebase --branch <bookmark> -d main@origin` then
  `jj git push --bookmark <bookmark>`.
- **The bot authenticates as the `kolohelios-bot` GitHub App** —
  required so post-rebase pushes re-trigger the normal CI for the PR
  (which pushes via `GITHUB_TOKEN` do not). App settings,
  secret/variable names, and rotation steps live in
  `.github/auto-rebase-app.md`.
- See also #147 (`shaka repo rebase-wip`), the local-side companion
  for branches you're actively iterating on.

### Daily kolohelios-nix lock bump

`.github/workflows/bump-kolohelios-nix.yaml` runs daily at 00:00 UTC
(also `workflow_dispatch:` for manual triggers). It runs
`shaka repo bump-locks --input kolohelios-nix --pr-branch
bot/bump-kolohelios-nix`, which:

1. Walks every project, runs `nix flake update kolohelios-nix` inside
   each that consumes the input, and notes which `flake.lock`s changed.
2. If anything changed, branches off `main`, commits all changed
   `flake.lock`s in one commit titled
   `chore(deps): bump kolohelios-nix flake input`, and force-pushes
   to `bot/bump-kolohelios-nix`.
3. Opens a single lockstep PR; if one is already open for that branch,
   the force-push updates it in place instead of creating a duplicate.

No auto-merge — the PR is reviewed and merged manually after CI is
green. The `kolohelios-nix-via-flakehub` audit rule guarantees that
every consumer pins via the FlakeHub URL, so the bumper's grep-based
discovery is safe.

The workflow authenticates as the `kolohelios-bot` GitHub App (same
secret/variable as `auto-rebase-prs.yaml`) so the post-bump push
re-triggers the normal CI for the PR — pushes via the default
`GITHUB_TOKEN` do not.

### Working with `jj`

A few `jj` behaviors trip up agents whose mental model comes from git. Read
this before scripting against `jj`:

- **Change IDs come in two lengths.** Templates emit the 32-char form by
  default (`utxssoyuknns...`); the 12-char prefix shown in `jj log`
  (`utxssoyuknns`) only resolves while the change exists. Use
  `change_id.short()` in templates to get the prefix explicitly.
- **Empty `@` auto-abandons.** `jj new <ref>` from an empty `@` switches
  `@` and abandons the empty change. To move `@` without creating a new
  change, use `jj edit <rev>`.
- **Bookmarks track `change_id`, not `commit_id`.** `jj describe @`
  rewrites the commit but the bookmark moves with the change — you
  rarely need to re-set a bookmark after editing.
- **`jj restore <path>` resolves paths relative to the repo root**, not
  the `cwd`. Pass absolute paths from automation.
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

**1Password** is the canonical secret store for local development
(`op` command-line tool), CI (GitHub Actions integration), and
infrastructure. Never commit secrets, never propose `.env` files checked
into the repo.

## Adding a new project

For rust projects, run:

```
shaka project new --name <name> --slot <slot>
```

`<slot>` is one of `apps`, `packages`, `projects`, `tools`. The command
writes the canonical skeleton (`project.cue`, `Cargo.toml`, `flake.nix`,
`.envrc`, `README.md`, `.gitignore`, `src/main.rs`), generates the
per-project `justfile`, and runs `schema-check` + `audit` against the
result.

For `infra` and `nix-lib` projects (and any non-rust kind), scaffold by
hand for now — `project new` only ships the rust template:

1. Create `<slot>/<name>/project.cue` matching `tools/shaka/schema/project.cue`.
2. Run `shaka project schema-check` to confirm the schema accepts it.
3. Run `shaka project generate-justfiles` to produce the per-project `justfile`.
4. Add the project's source files and flake inputs.
5. If the project introduces new preflight checks, add them to
   `tools/shaka/src/preflight.rs` rather than to CI YAML.

## Workspaces

**Every issue gets its own `shaka workspace` — and the primary tree is
off-limits for issue work.** Before your first `Edit` or `Write` in
this repo, verify `cwd` ends in `kolohelios-i<N>/` (a workspace), not
bare `kolohelios/` (the primary tree). If you're in primary, **stop
and create the workspace first** — don't edit-now-reshuffle-later.

`/start` invokes `shaka workspace new --issue <N>` by default. The
primary tree is reserved for sync, audit, and cross-cutting reads —
never carrying WIP for the issue you're picking up.

Why workspace-per-issue is the default:

- Primary stays clean. Cross-cutting ops (`shaka repo sync`,
  `shaka repo audit`, `gh`-from-cwd, reading state across all in-flight
  changes) never compete with WIP for some specific issue.
- Parallel slices are free. Pick up a second issue while the first is
  in CI; both have their own filesystem trees but share `.jj/repo`.
- Hygiene is solved. `shaka workspace cleanup` finds merged workspaces
  via persisted issue links (no longer dependent on bookmark presence).

Shape:

1. `/start <N>` (or manually: `shaka workspace new --issue <N>`)
   creates a sibling working copy at `../kolohelios-i<N>` that shares
   the same `.jj/repo` but has its own `@`. Each workspace has a
   filesystem-level full copy of the repo but the underlying commit
   storage is shared.
2. Run `claude` inside each workspace, or spawn sub-agents pointed at
   each workspace's path. Each session bookmarks, commits, and pushes
   independently. Concurrent `jj` operations are serialized via the repo
   lock — light contention at worst.
3. As PRs land, rebase the remaining branches on the new `main@origin`.
   Until #147 (`shaka repo rebase-wip`) lands, this is manual:
   `jj rebase --branch <bookmark> -d main@origin` per branch, then
   `jj git push --bookmark <name>` (`jj` allows the non-fast-forward
   "move sideways" without `--force` for rebased branches).
4. `shaka workspace status` shows a per-workspace summary at any time.
5. `shaka workspace cleanup` forgets workspaces whose PRs have merged
   (uses the persisted issue link, so it works regardless of remote
   branch deletion). Workspaces created without `--issue` (ad-hoc
   names) need `shaka workspace forget --force <name>`.

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

- **Don't edit code in the primary tree.** Issue work belongs in a
  `shaka workspace` — see [Workspaces](#workspaces). If you catch
  yourself about to run `Edit` or `Write` from bare `kolohelios/`
  (not `kolohelios-i<N>/`), stop and create a workspace first.
- **Don't edit generated `justfile`s.** They carry a "Do not edit by hand"
  header and CI fails on drift. Change
  `tools/shaka/src/project/generate_justfiles.rs` instead.
- **Don't add CI jobs to `.github/workflows/main.yaml`** for new validation
  steps — extend `shaka preflight` so CI and local stay in lockstep.
- **Don't add Claude Code attribution** to commits, code, or docs.
- **Don't run mutating `git` commands.** The repo is `jj`-colocated;
  `git stash`, `git stash pop`, `git checkout`, `git reset`,
  `git commit`, `git rebase`, `git merge` all leave the working copy
  out of step with what `jj` knows about it. Read-only `git` (`status`, `log`, `diff`)
  is fine. To test "what does main look like without these changes,"
  use `jj new main@origin` (the original change is preserved and
  reachable via `jj log` / `jj edit`); to inspect a previous state, use
  `jj op log` and `jj op restore`. Use `shaka repo sync` for the
  rebase-on-`main@origin` flow.
- **Don't add thin pass-through wrappers.** Don't add `justfile` targets,
  scripts, or aliases whose only job is to forward to another tool. If
  `shaka preflight` is the entry point, document and call it directly —
  don't put a layer in front whose only purpose is to advertise it.
  Per-project `justfile`s with multiple real recipes (`build`, `test`,
  `lint`, `validate`) are not pass-throughs — those are aggregations.
- **Don't embed external-format fixtures as string literals in Rust
  tests.** When testing code that processes external file formats (CUE,
  YAML, JSON, etc.) by shelling out to tooling, write fixtures as real
  files in `<crate>/tests/fixtures/<topic>/{valid,invalid}/...` and walk
  them from integration tests. Real files are syntax-highlighted, can be
  inspected directly with the underlying tool (for example,
  `cue vet schema fixture`), and are the actual artifacts shipped in the
  repo. Adding coverage = adding a fixture file, not editing test code.
  See `tools/shaka/tests/schema.rs` for the pattern.
- **Don't push without re-running `just validate` after your last edit.**
  Even if validate passed earlier in your session, a final tweak can
  introduce `cargo fmt` or `clippy` violations. The CI failure cycle is
  slow; the validate run immediately before pushing is the cheap
  insurance.
- **Don't document or automate clipboard-paste-of-DOM patterns.** Snippets
  shaped like "open DevTools, paste this `JSON.stringify(...)` of
  authenticated DOM data, copy the result" match credential-stealer
  signatures (Atomic, StealC, etc.) and are blocked by macOS Sequoia's
  `XProtect` clipboard scanner. They're also a pattern users should
  rightly distrust on sight. When extracting structured data from an
  authenticated browser session, build the payload in-page and trigger a
  `Blob` download instead — no clipboard transit, no `XProtect`
  involvement, no
  cargo-cultable shape that resembles malware. See `shaka domain
  inventory --help` for the canonical example.
