---
description: Ship the current change — rebase, lint commits, self-review, preflight, push, open PR
allowed-tools: Bash(shaka *), Bash(jj *), Bash(gh pr *), Bash(gh issue view:*), Read, Edit, Glob, Grep
---

Ship the current `jj` change as a PR. Walk the user through each step,
surface any failures, and stop for input when something needs human
judgment. Do not silently push past errors.

## Workflow

### 0. Snapshot state

Run `shaka repo status --json` to get a structured view of the working copy
(`bookmarks`, `change_id`, `parent.is_main_origin`, `dirty`, `ahead`, `pr`,
`last_fetch`). Use this — not `jj st` parsing — to drive decisions throughout
the workflow. Re-run after `repo sync` and `repo send` to confirm state
transitions (for example, parent advanced to a new `main@origin` commit,
`pr` populated after push).

### 1. Rebase on main@origin

Run `shaka repo sync` (it does `jj git fetch` + rebase onto `main@origin`).
If it errors with conflicts, resolve them in place when the merge is
mechanical and both sides' intent is unambiguous (for example: sibling
tests added in the same spot, a new arm in a match expression, the
same import added on both sides). After resolving, run `jj squash` to move
the resolution into the conflicted commit, then surface a one-line
summary of what you resolved so the user can spot-check. Stop and ask
only when: both sides touched the same logical concern in incompatible
ways, the resolution requires picking between competing semantics, or
the conflict spans more than ~2 files or ~3 hunks.

### 2. Lint commits

Run `shaka commit lint -r 'main..@'` to check every commit on this branch
against project conventions:

- Conventional commit title (`<type>(<scope>): <subject>`, max 70 chars)
- Title and body separated by blank line; body lines wrapped at 80
- Atomic, vertical commits — warn on cross-project commits

If `shaka commit lint` reports errors, fix them and re-run before continuing:

- Empty / malformed description → `jj describe -r <change>` to fix
- Cross-project commit → `jj split -r <change>` to break it apart
- Title too long or body unwrapped → `jj describe -r <change>` to rewrite

Treat warnings (for example, cross-project) as a checkpoint: surface them
to the user and ask whether to split before continuing. Don't auto-split.

### 3. Self-review the diff

Run `jj diff -r 'main..@'` and read the full output. Look for:

- Leftover debug prints, `dbg!`, `console.log`, commented-out code
- Unintended file changes (formatter ran on unrelated files, accidental edits)
- Missing test coverage for new behavior
- Style or naming inconsistencies with surrounding code
- Secrets, tokens, or local paths that shouldn't be committed

Report findings to the user. If anything looks wrong, stop, and let the user
decide whether to fix before shipping.

### 4. Run preflight

Run `shaka preflight --since main@origin` to scope checks to changed paths.
The argument is shelled to `jj diff --from`, so it takes a `jj` revision
(use `main@origin`, not git-style `origin/main`).
This is the same gate CI runs, so passing here means CI passes too.

If preflight fails, stop. Do not push a known-broken change. Help the user
diagnose the failure.

### 5. Push and open PR

Run `shaka repo send`. It fetches, rebases onto `main@origin` (re-running
preflight if the rebase moved `@`, since new ancestors could conflict with
our changes in ways `jj` doesn't flag), sets a bookmark from the change
description, pushes, and creates a PR if one doesn't exist. Report the PR
URL.

If `shaka repo send` errors because the change has no description, stop and
ask the user to run `jj describe`. If the rebase hits a conflict, stop and
let the user resolve it.

## Conventions

- Never use `git` for working-copy mutations — only `jj`.
- PR body: brief summary paragraph. **No test plan section** — `shaka
  preflight` already gates correctness.
- No Claude Code attribution in commit messages or PR bodies.
- If the change closes a GitHub issue, include `Closes #<n>` in the PR body.

## Stop conditions

Halt the workflow and report to the user when:

- `shaka repo sync` hits a conflict that isn't a mechanical 2-or-3-way
  merge (see Step 1 for the threshold)
- `shaka commit lint` reports errors that aren't trivially auto-fixable
- Self-review surfaces something unintentional
- `shaka preflight` fails any check (including the conditional re-run inside
  `shaka repo send` when `main@origin` advanced during the work)
- `shaka repo send`'s pre-push rebase hits a conflict — let the user resolve
- The change has no description (push step needs one)

A failed step is a stop — not a prompt to retry blindly. Diagnose first.
