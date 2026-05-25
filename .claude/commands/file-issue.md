---
description: File a new GitHub issue with a duplicate-search gate (the well-lit path; the PreToolUse hook is the safety net)
allowed-tools: Bash(gh issue *), Bash(shaka *), Bash(tools/shaka/bin/shaka *), Read
---

File a new GitHub issue. The `PreToolUse` hook on `gh issue create`
catches duplicates as a safety net; this skill is the well-lit path
that runs the search up front so you see candidates before drafting
a body.

## Workflow

### 1. Search first

Take the topic the user wants to file. Run:

```
gh issue list --state open --search "<keywords>" --limit 10
```

Pick keywords from the topic — typically the project scope plus the
distinguishing nouns/verbs (for example, `blogctl refine command`
or `shaka issue list search`).

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

Once the user confirms "file as new," draft a body using this repo's
standard issue shape:

- `## Problem` — what's broken or missing
- `## Proposed implementation` — the shape, not every detail
- `## Acceptance criteria` — checklist
- `## Out of scope` — what we're explicitly deferring

Stop, show the draft, and wait for the user to confirm or tweak.

### 4. File

Run `gh issue create --title "<title>" --body "<body>" [--label ...]`.

The `PreToolUse` hook will re-run the search and block if it finds
matches that this skill missed. That's fine — it's the safety net.
If the hook blocks but the user has already confirmed in step 2 that
filing is intended, re-run with `BYPASS_ISSUE_DUP_CHECK=1` prefix:

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
