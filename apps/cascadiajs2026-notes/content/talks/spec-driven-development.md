Specs are the **primary contract** for code generation. (Context: AWS's Kiro,
which started life as a VS Code fork — Alexa lineage at AWS.)

## Why specs?

Treat the AI like an intern: one small deviation can produce wildly different
results. So get planning and product involved, and write the contract down.

Can't the latest frontier models just do everything? A few things to keep in
mind:

- **Too much context** confuses models. Keep `AGENTS.md` / steering files
  targeted. Use skills to create specs and implement them; agents can help with
  the implementation plan.
- **Too much trust** is a trap. Are we code reviewing? We're the human in the
  loop — that matters. Set up AI code reviews as a second pass too.
- Watch for **outcome divergence** (drifting off the intended target) and
  **speed over maintainability** (what patterns are we creating?).

## Doing it without Kiro

Tell the AI IDE what to produce, in order:

1. **User requirements**
2. A **design document** derived from them
3. **Implementation details** derived from both

Tools in this space: Spec-Kit, OpenSpec, BMad. Kiro's spec mode also works in a
**brownfield** app. Use **EARS** (Easy Approach to Requirements Syntax) to go
from requirements → design phase. Review the markdown, approve, then move to
implementation — which can happen out of order. Create an MVP from the steps by
taking a **vertical slice**, pedantically; keep it short and tight, and keep
reviewing.

## MCP and spec-driven development

Isn't MCP dead? It's less hyped now — switching to CLIs is common — but it may
still be valuable *for spec-driven development*:

- Specs can be **pulled from a project-management service**.
- You can take an existing app/PoC and **reverse-engineer** a spec from it.
- It will follow the rules set in your steering files.

Close the loop with **property-based tests** that run as regression tests
against the requirements: "take all these steps and create the few checks I can
use to prove this works." Conceptually — does it work the way I expect?
