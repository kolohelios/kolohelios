---
description: File a new GitHub issue via `shaka issue create`, which enforces a canonical scope label and a conventional-commit title, behind a duplicate-search gate up front
allowed-tools: Bash(shaka issue *), Read
---

File a new GitHub issue. This skill is the well-lit path that runs the
search up front so you see candidates before drafting a body, then files
through `shaka issue create` — which enforces a canonical scope label
(from `.shaka/labels.cue`) and validates the title the same way
`shaka commit lint` does. The
search-first step below is the duplicate gate: the repo's `PreToolUse`
dup-check hook only intercepts `gh issue create`, so it does **not** fire
on `shaka issue create`.

## Workflow

### 1. Search first

Take the topic the user wants to file. Run:

```
shaka issue list --search "<keywords>"
```

Pick keywords from the topic — typically the project scope plus the
distinguishing nouns/verbs (for example, `analytics summary` or
`auth token storage`).

### 2. Show candidates

Report what came back. For each result, show the number, state, and
title. Explicitly say one of:

- "No matches — safe to file as new."
- "N candidates found — review before filing."

If candidates look like potential duplicates, surface them by number
and quote the relevant title fragments. Stop and wait for the user
to confirm one of:

- File as new anyway (genuinely different despite title overlap).
- Pick up one of the candidates instead (read its body, decide
  whether to continue work there).
- Add a comment to an existing issue instead of filing fresh.

### 3. Draft the issue body

Once the user confirms "file as new," draft a body using a standard
issue shape:

- `## Problem` — what's broken or missing
- `## Proposed implementation` — the shape, not every detail
- `## Acceptance criteria` — checklist
- `## Out of scope` — what we're explicitly deferring

The title must be conventional-commit form (`<type>(<scope>): <subject>`)
or `shaka issue create` rejects it. Stop, show the draft, and wait for
the user to confirm or tweak.

### 4. File

Write the drafted body to a temp file and run:

```
shaka issue create --title "<title>" --scope <scope> --body-file <path> [--label ...] [--parent <N>]
```

- `--scope` (required) — the canonical scope label from
  `.shaka/labels.cue`; usually the project name (`shaka`, `infra/home`).
  `shaka` attaches it and fails if it isn't canonical.
- `--label` (repeatable) — extra labels beyond the scope label
  (for example, `enhancement`).
- `--parent <N>` — link as a sub-issue via GitHub's native sub-issue
  API; use this instead of freeform "Part of #N" body text.
- `--body-file <path>` — read the body from a file (cleaner than
  `--body` for multi-line). `--body "<text>"` also works.
- `--dry-run` — print the planned `gh` calls without executing, to
  preview before filing.

### 5. Report

Print the issue number and URL. If the issue is the first sub-task
of a tracking issue, mention it so the user can decide whether to
update the tracker (or pass `--parent <N>` in step 4 to link it
natively).

## Conventions

- Scope label comes from `--scope` — one of the canonical labels in
  `.shaka/labels.cue`. Use `--label` for any extras on top.
- One issue per discrete feature/bug. Sub-tasks of a larger effort
  get their own issues, linked with `--parent <N>`.
- Conventional commit type belongs in the title:
  `feat(<scope>): <subject>` / `fix(<scope>): <subject>` / etc. `shaka`
  validates this.

## Stop conditions

- Search returns matches → stop, show them, wait for user.
- No clear conventional-commit type for the title → stop and ask
  (`shaka` rejects a non-conforming title).
- No canonical scope label fits the topic → stop and ask which scope
  to use (or whether to add a label to `.shaka/labels.cue` first).
- Issue body draft → stop, show it, wait for confirmation.
