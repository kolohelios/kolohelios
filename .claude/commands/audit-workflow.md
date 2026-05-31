---
description: Audit recent Claude sessions for friction patterns worth fixing with tooling
allowed-tools: Bash(gh issue list:*), Bash(gh issue create:*), Bash(find:*), Read, Grep, Glob, Agent
---

Audit recent Claude Code sessions on this machine for **churn loops**
and **friction points**, then surface them as a triage table the user
can turn into GitHub issues.

Run this every few days or weekly. The output is ephemeral — the
durable artifact is the issues filed from it.

## Workflow

### 1. Pick the window

Default: sessions modified in the last 7 days. If the user passed
`--since <N>d` (for example `--since 14d`, `--since 3d`), use that
window instead.

List candidate session files across **all** project directories on the
machine (friction patterns surface anywhere Claude runs, not just
kolohelios):

```
find ~/.claude/projects -maxdepth 2 -name '*.jsonl' -mtime -<N>
```

Report the count before delegating ("scanning N sessions across M
projects"). If zero, stop and report "no sessions to audit."

### 2. Delegate to one Explore agent

Spawn a single Explore agent with the prompt below (verbatim — the
taxonomy and evidence rules are load-bearing):

> Analyze the listed Claude Code session JSONL files for churn loops
> and friction patterns. Each line in a JSONL is one turn (user
> message, assistant message, or tool result).
>
> **Taxonomy — look for:**
>
> - Push → CI-fail → fix → push loops
> - `just validate` failures after a "final tweak" (validate pass →
>   edit → push without re-validate → CI/local fail)
> - User corrections ("no," "don't," "stop," "actually," "wait,")
>   that redirect a chosen path
> - Permission prompts for the same read-only command across sessions
>   (allowlist candidates for `.claude/settings.json`)
> - Multi-step manual sequences that recur (new `shaka` subcommand or
>   `/skill` candidates)
> - Searches that took 3+ greps/reads to land the right answer
> - Sub-agent briefing gaps (sub-agent forgot a rule the parent should
>   have stated up front)
> - `nix develop`/`direnv` failures that triggered a retry
>
> **Evidence rules — this is the critical guardrail:**
>
> - **Mention counts are NOT evidence.** A finding must cite at least
>   one **failure→fix→retry triple** in the session log:
>   1. A `tool_result` with `is_error: true` (or non-zero exit, or a
>      clear error string in stderr/stdout)
>   2. An edit or decision turn responding to that error
>   3. Another invocation of the same command (or a closely related
>      one) that reflects the fix
>
>   Without that triple, the pattern is dropped.
> - **Sample size:** a finding needs evidence from **≥2 distinct
>   sessions** before it's worth surfacing.
> - **Skip prior audit output:** exclude turns whose `<command-name>`
>   tag contains `audit-workflow` (otherwise we reinforce prior runs).
> - **Sessions are large (multi-MB).** Don't full-read; grep for
>   `"is_error":true`, error keywords, and the user's correction
>   phrases above, then read the surrounding turns.
>
> **Proven-unfixable patterns to skip** (don't surface as findings,
> even when the evidence triples exist):
>
> - `"File has not been read yet"` errors before `Edit`/`Write`. The
>   harness's read-check runs upstream of `PreToolUse` hooks, so no
>   hook-shaped fix is possible; `additionalContext` doesn't
>   substitute for an actual `Read` invocation either. Ref #626 for
>   the full spike. Revisit if the hook API gains a
>   `pre-tool-validate` event or hook ordering changes.
>
> **Output format** — one row per pattern with these fields:
>
> - Pattern: one-sentence description
> - Evidence: 2-3 session ID prefixes + approximate turn number + the
>   triggering command/error
> - Proposed fix shape: new `shaka <subcommand>`, CLAUDE.md hint,
>   `/skill` addition, `settings.json` allowlist entry, or hook
> - Priority: high / medium / low (frequency × time-cost per
>   occurrence)
>
> Cap at **≤10 findings**. Prefer 5 strong ones over 10 weak ones.

### 3. Cross-check against existing issues

For each finding the agent surfaces, search GitHub:

```
gh issue list --state all --limit 30 --search "<keywords from finding>"
```

Annotate each row with a **Dup-Check** column:

- `none` — no overlap
- `extends #N` — related open issue, this would build on it
- `duplicates #N` — already covered; surface for visibility, don't
  file
- `reopens #N` — was closed but the pattern recurs, worth reopening

**Do not silently drop duplicates** — surface them so the user can
decide whether the closed work was sufficient.

If `gh` fails (auth, rate limit), present the table anyway with
Dup-Check set to `unchecked` and tell the user to verify before
filing.

### 4. Present and offer to file

Output the triage table:

| Pattern | Evidence | Proposed Fix | Dup-Check | Priority |
|---------|----------|--------------|-----------|----------|

Ask which rows the user wants to file. For each pick, draft a title in
conventional commit format (`<type>(<scope>): <subject>`) and a body
that includes context (which sessions, what the friction looked like),
scope / out of scope, and acceptance criteria. Then call
`gh issue create`. **Never auto-file** — always wait for the user's
selection.

## Conventions

- This skill runs from the **primary tree**. It's read-only against
  the repo; no workspace needed.
- Findings without a failure→fix→retry triple are dropped silently,
  not surfaced as "weak signal."
- The skill identifies opportunities; it doesn't fix them. Each filed
  issue is downstream work.

## Stop conditions

- No sessions in the window → report and exit.
- Explore agent returns zero findings → report and exit (success).
- `gh issue list` fails → present findings with `unchecked` dup
  column.
