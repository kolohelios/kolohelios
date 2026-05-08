---
description: Start working on a GitHub issue — create workspace, read, bookmark, and plan
allowed-tools: Bash(shaka repo sync), Bash(shaka repo status:*), Bash(shaka workspace new:*), Bash(shaka issue brief:*), Bash(jj *), Read, Glob, Grep
argument-hint: <issue-number>
---

Pick up GitHub issue `$1` and set up a workspace to work on it. Walk the
user through each step, surface anything that needs human judgment, and stop
for confirmation before writing code.

## Workflow

### 0. Start from a clean conversation

If this conversation already contains unrelated prior context, stop, and
ask the user to run `/clear` and then re-invoke `/start $1` in a fresh
conversation. Picking up a new issue with a polluted context risks
carrying assumptions, file reads, or plans from earlier work into the
new task. Don't proceed unless the conversation is fresh or the user
explicitly confirms it's fine to continue.

### 1. Read the issue

Fetch the issue body and any comments in one shot:

```
shaka issue brief $1
```

This runs `jj git fetch` (so the next step's workspace parents on the
latest main), then prints a tree-formatted summary of the issue header,
body, and comments. If the issue doesn't exist, `brief` exits non-zero
with a clear error — stop and report rather than guessing.

Summarize for the user from the brief output:

- The scope (what the issue is asking for)
- Acceptance criteria (explicit or implied)
- Any constraints or conventions called out

If the issue references other issues (`#NN`), files, or prior PRs, read
those for context too — `shaka issue brief <N>` for sibling issues, or
the relevant files directly. Don't guess; read the referenced material.

### 2. Create a workspace

**Every issue gets its own `shaka workspace` — no exceptions.** This keeps
the primary tree clean for sync, audit, and cross-cutting reads, and it
means the user's WIP elsewhere is undisturbed.

```
shaka workspace new --issue $1
```

This creates a sibling working copy at `../kolohelios-i$1` parented on the
current `main@origin` (already up-to-date from the previous step's fetch).
If the command errors (path collision, repo lock), stop and report.

### 3. Create the bookmark

Derive a bookmark from the issue title in the form
`<type>/<short-description>`:

- `<type>` is the conventional commit type from the issue title prefix
  (`feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `style`, `perf`,
  `ci`, `build`). If the title has no prefix, infer from content and the
  issue's labels.
- `<short-description>` is kebab-case, drops filler words, and is scoped
  to the project where applicable (for example, `feat/shaka-commit-lint`,
  `feat/skills-start-issue`).

Don't ask for confirmation on the name — issues are the canonical entry
point, so the issue number is the load-bearing identifier and the
bookmark name adds no signal worth a round-trip. Create it directly from
the new workspace path (`/Users/jedwards/code/kolohelios-i$1`):

```
jj bookmark create <name> -r @
```

### 4. Plan and confirm

Outline an implementation approach in the response — files to modify,
sequencing of commits (one logical change per commit, per project
conventions), edge cases or open questions. Stop, and wait for the user
to confirm or redirect before writing any code.

## Conventions

- One issue per bookmark — don't mix work from multiple issues.
- One workspace per issue — keeps primary clean for sync, audit, and
  cross-cutting reads.
- Bookmark `<type>` must match the conventional commit type used in the
  eventual commit(s).
- Never use `git` for working-copy mutations — only `jj`.
- The plan step is mandatory: don't start editing files in the same turn
  that creates the bookmark.

## Stop conditions

Halt and report to the user when:

- `shaka workspace new` errors (path collision, repo lock)
- `shaka issue brief` fails (issue doesn't exist, auth missing)
- The issue title doesn't yield an obvious conventional type and labels
  don't disambiguate
- The user has not confirmed the plan
