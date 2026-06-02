Capability is going exponential with Opus 4.5+, and the leverage point is no
longer the model — it's the **harness**.

```
model | harness | context
```

**Harness engineering is the state of the art.** The Claude Code architecture
diagram is wild once you see everything it bundles: prompts, skills,
directives, subagents, tools, bundled infra, orchestration logic, hooks,
middleware, and observability (logs!).

The takeaway in three words: **own the tools.** `opencode` gave us the
opportunity to build our own.

## Swarms

A swarm is **multi-agent coordination to survive context death** — when one
agent's context fills up, the work survives across the swarm. Useful pattern:
use the Socratic method to align intent and outcomes, plan, then fan out
parallel agents.

## Build a harness that makes you smile

- `SOUL.md` — give the agent a personality and an icon. Your harness should
  make you smile.
- There's another harness called **Pi** whose workflow implementation is much
  better than the default. Pi **updates itself** — it reads its own source code
  to add capabilities. Out of the box it's intentionally not featureful.
- Pi subagents run in parallel and chains — a great extension for swarm-style
  work.
- `pi-cmux` — a terminal replacement built on Ghostty, managed by a Pi
  extension.
- `pi-notes` — HTML is *not* better than markdown; notes should be human
  readable but also agent readable. (`mdsvex` is the Svelte take on MDX.)
- `pi-feedback` — a support inbox: the last turn of a session sends a diff to
  process feedback. With a lot of users you get a lot of support tickets, so a
  feedback tool that improves the automated support over many turns pays off.

A recurring theme: **slow down.** Don't do everything, and don't do it quickly
(cf. *Slow Productivity*).

## Worth following

Cloudflare Durable Objects for state. Sunil Pai (something called "Think").
Dillon Mulroy on artifacts, memory, and primitives. Addy Osmani's **"The
Orchestration Tax"** — cognitive bandwidth is *not* parallelizable. Agent swarms
are genuinely hard; the real skill is building systems that can pull any and
all threads through the context limit.
