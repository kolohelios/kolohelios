---
description: Ship the current change — rebase, lint commits, self-review, preflight, push, open PR
allowed-tools: Bash(shaka *), Bash(jj *), Bash(gh pr *), Bash(gh issue view:*), Read, Edit, Glob, Grep
---

Ship the current jj change as a PR. Walk the user through each step, surface
any failures, and stop for input when something needs human judgment. Do not
silently push past errors.

## Workflow

### 1. Rebase on main@origin

Run `shaka repo sync` (it does `jj git fetch` + rebase onto `main@origin`). If
it errors with conflicts, stop and report — the user resolves conflicts.

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

Treat warnings (e.g. cross-project) as a checkpoint: surface them to the user
and ask whether to split before continuing. Don't auto-split.

### 3. Self-review the diff

Run `jj diff -r 'main..@'` and read the full output. Look for:

- Leftover debug prints, `dbg!`, `console.log`, commented-out code
- Unintended file changes (formatter ran on unrelated files, accidental edits)
- Missing test coverage for new behavior
- Style or naming inconsistencies with surrounding code
- Secrets, tokens, or local paths that shouldn't be committed

Report findings to the user. If anything looks wrong, stop and let the user
decide whether to fix before shipping.

### 4. Run preflight

Run `shaka preflight --since origin/main` to scope checks to changed paths
(`--since` takes a git ref, not a jj revset).
This is the same gate CI runs, so passing here means CI will pass.

If preflight fails, stop. Do not push a known-broken change. Help the user
diagnose the failure.

### 5. Push and open PR

Run `shaka repo send`. It sets a bookmark from the change description, pushes
it, and creates a PR if one doesn't exist. Report the PR URL.

If `shaka repo send` errors because the change has no description, stop and
ask the user to run `jj describe`.

## Conventions

- Never use `git` for working-copy mutations — only `jj`.
- PR body: brief summary paragraph. **No test plan section** — `shaka
  preflight` already gates correctness.
- No Claude Code attribution in commit messages or PR bodies.
- If the change closes a GitHub issue, include `Closes #<n>` in the PR body.

## Stop conditions

Halt the workflow and report to the user when:

- `shaka repo sync` hits a merge conflict
- `shaka commit lint` reports errors that aren't trivially auto-fixable
- Self-review surfaces something unintentional
- `shaka preflight` fails any check
- The change has no description (push step needs one)

A failed step is a stop — not a prompt to retry blindly. Diagnose first.
