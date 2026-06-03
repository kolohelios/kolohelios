Specs are the primary contract for generation of code.

Why?

**AI as intern** — one small deviation can result in very different results. Get planning and product involved.

Can't the latest frontier models do everything??

Let's keep a few things in mind:

- **Too much context!!** Models get confused.
- `AGENTS.md` / steering — keep it targeted.
- Use skills — create specs and implement, and agents can help with the implementation plan.
- **Too much trust!!** Are we code reviewing? We are the human in the loop, it's important.
- Set up AI code reviews as well / task rabbit etc.

Don't get off the intended target — **outcome divergence**.

Speed over maintainability — what patterns are being created?

History lesson — Alexa at AWS, Kiro, first as VS Code fork.

## Iterating on spec-driven development

How do you do this stuff WITHOUT Kiro?

Tell the AI IDE what it should do:

- include the following
    - user requirements
    - design document from that
    - take both of those and create implementation details
    - `Spec-kit` / `Open Spec` / `BMad`

Can use Kiro spec mode in a brownfield application.

**EARS** — Easy Approach to Requirements Syntax.

```
requirements -> design phase
```

Review the markdown, approve, move to implementation. Implementation phase can be out of order.

Create an MVP from the steps — take a vertical slice, pedantically.

- Keep it short and tight, continue to review.

## MCP

Isn't `MCP` dead? It's not as popular now… maybe still valuable for spec-driven development?

Switching to CLIs is pretty common.

```
MCP <-> Spec Driven Development
```

- specs can be pulled from project management service
- pulled from PM — take app/PoC and reverse-engineer it
- will follow rules set forth in your steering files

**Property-based tests** — testing to run regression tests against the requirements.

"Take all these steps and create the four that I can use to prove that this works."

Conceptually, does it work as I expect?
