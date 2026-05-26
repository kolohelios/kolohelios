---
description: File a new GitHub issue with a duplicate-search gate (the well-lit path; a PreToolUse hook may also be configured per-repo as the safety net)
allowed-tools: Bash(gh issue *), Read
---

File a new GitHub issue. This skill is the well-lit path that runs the
search up front so you see candidates before drafting a body. Some
repos also configure a `PreToolUse` hook on `gh issue create` as a
safety net (kolohelios is one); the skill works the same with or
without the hook.

## Workflow

### 1. Search first

Take the topic the user wants to file. Run:

```
gh issue list --state open --search "<keywords>" --limit 10
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

Stop, show the draft, and wait for the user to confirm or tweak.

### 4. File

Run `gh issue create --title "<title>" --body "<body>" [--label ...]`.

If the repo has a `PreToolUse` hook configured, it will re-run the
search and block on matches. If the hook blocks but the user has
already confirmed in step 2 that filing is intended, re-run with a
`BYPASS_ISSUE_DUP_CHECK=1` prefix (kolohelios convention; other repos
may use a different bypass mechanism):

```
BYPASS_ISSUE_DUP_CHECK=1 gh issue create --title ... --body ...
```

### 5. Report

Print the issue number and URL. If the issue is the first sub-task
of a tracking issue, mention it so the user can decide whether to
update the tracker.

## Conventions

- Labels are scoped per project — use the ones that already exist;
  `gh label list` if unsure.
- One issue per discrete feature/bug. Sub-tasks of a larger effort
  get their own issues with a `Blocked by` or `Part of` line.
- Conventional commit type belongs in the title:
  `feat(<scope>): <subject>` / `fix(<scope>): <subject>` / etc.

## Stop conditions

- Search returns matches → stop, show them, wait for user.
- No clear conventional-commit type for the title → stop and ask.
- Issue body draft → stop, show it, wait for confirmation.
- Hook blocks the create despite step-2 confirmation → ask before
  using `BYPASS_ISSUE_DUP_CHECK=1`.
