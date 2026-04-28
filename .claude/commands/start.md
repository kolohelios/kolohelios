---
description: Start working on a GitHub issue — sync, read, bookmark, and plan
allowed-tools: Bash(shaka repo sync), Bash(shaka repo status:*), Bash(jj *), Bash(gh issue view:*), Bash(gh issue list:*), Read, Glob, Grep
argument-hint: <issue-number>
---

Pick up GitHub issue `$1` and set up the workspace to work on it. Walk the
user through each step, surface anything that needs human judgment, and stop
for confirmation before writing code.

## Workflow

### 0. Start from a clean conversation

If this conversation already contains unrelated prior context, stop and ask
the user to run `/clear` and then re-invoke `/start $1` in a fresh
conversation. Picking up a new issue with a polluted context risks carrying
assumptions, file reads, or plans from earlier work into the new task. Don't
proceed unless the conversation is fresh or the user explicitly confirms it's
fine to continue.

### 1. Check for in-progress work

Run `shaka repo status --json` for a structured snapshot (`bookmarks`,
`change_id`, `parent.is_main_origin`, `dirty`, `ahead`, `pr`). If `dirty.total
> 0` or `ahead > 0` and `bookmarks` is empty, there's WIP without a bookmark —
surface the current change to the user before doing anything else. The change
will be preserved either way, but the user should decide whether to:

- Set a bookmark on it first (`jj bookmark create <name> -r @`)
- Continue and let it sit as an orphan change reachable via `jj log`
- Abort the `/start` flow

Don't proceed silently if there's WIP without a bookmark.

### 2. Sync on main@origin

Run `shaka repo sync` to fetch and rebase the working copy onto
`main@origin`. If it errors (conflicts, network), stop and report.

Re-run `shaka repo status --json` to confirm `parent.is_main_origin` is `true`
and `dirty.total` is `0`.

### 3. Read the issue

Run `gh issue view $1` and read the full body. If the issue has comments
worth reading, run `gh issue view $1 --comments`. Summarize for the user:

- The scope (what the issue is asking for)
- Acceptance criteria (explicit or implied)
- Any constraints or conventions called out

If the issue references other issues (`#NN`), files, or prior PRs, read
those for context too. Don't guess — read the actual referenced material.

### 4. Propose a bookmark name

Derive a bookmark from the issue title in the form
`<type>/<short-description>`:

- `<type>` is the conventional commit type from the issue title prefix
  (`feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `style`, `perf`,
  `ci`, `build`). If the title has no prefix, infer from content and the
  issue's labels.
- `<short-description>` is kebab-case, drops filler words, and is scoped
  to the project where applicable (e.g. `feat/shaka-commit-lint`,
  `feat/skills-start-issue`).

Propose the name to the user and wait for confirmation or a counter-suggestion
before creating it. Then run `jj bookmark create <name> -r @`.

### 5. Plan and confirm

Outline an implementation approach in the response — files to touch,
sequencing of commits (one logical change per commit, per project
conventions), edge cases or open questions. Stop and wait for the user to
confirm or redirect before writing any code.

## Conventions

- One issue per bookmark — don't mix work from multiple issues.
- Bookmark `<type>` must match the conventional commit type used in the
  eventual commit(s).
- Never use `git` for working-copy mutations — only `jj`.
- The plan step is mandatory: don't start editing files in the same turn
  that creates the bookmark.

## Stop conditions

Halt and report to the user when:

- `@` has WIP without a bookmark (step 1)
- `shaka repo sync` hits a conflict or network error
- `gh issue view` fails (issue doesn't exist, auth missing)
- The issue title doesn't yield an obvious conventional type and labels
  don't disambiguate
- The user has not confirmed the bookmark name or the plan
