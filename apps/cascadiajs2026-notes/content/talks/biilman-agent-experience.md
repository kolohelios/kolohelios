The progression is real: co-pilot → AI agent → AI assistance. We're moving
from autocomplete toward genuine autonomy, and it changes the economics of
building software.

**Build vs. buy has flipped.** Things that weren't viable to build before are
now in reach — even whole HR systems. The SaaS budget is being replaced with a
dev budget. Autonomous coding agents (e.g. **openclaw.ai**) are still early;
one fun pattern is repurposing an old laptop as the host for one.

The analogy that stuck: autonomous cars made us rethink cities; autonomous
agents make us rethink programming and systems. **Software is now read/write
for everyone.**

## Chapter II: AX (Agent Experience)

Build tools for the people that build tools.

- **UX** differentiates products from competitors.
- **DX** differentiates platforms.
- **AX** differentiates platforms *and* products.

The question: what experience do agents have with your tools and products? AX
is **not** a single feature or protocol — and it's definitely not "just MCP."

## The four pillars

**Access** — can agents reach the product at all? `netlify.ai` is built for
humans *and* agents; the database is "batteries included" and available to
agents directly. Emerging standards matter here too — e.g. WorkOS exploring
self-authenticating identity for AI agents.

**Context** — does the agent understand the product? Optimize for agent
consumption: prefer plain text / markdown, negotiate content types. **MCP is UI
for LLMs** — context is the most important piece. Expose what the agent can act
on, and steer it.

**Tools** — agents will use your product *even if you provide no tools*. Every
product **already has an agent experience** — the only question is whether it's
good or bad. A CLI is often great DX but bad AX (interactive prompts are
terrible for agents). API vs. MCP is the same product, different surfaces —
naively shifting an entire API to MCP overwhelms the context window, so limit
to a handful of tools. Provide **prompt escape hatches**.

**Orchestration** — can agents do real work? Linear and Notion let you kick off
longer-running tasks. Nobody wants another chat bot; orchestration lets people
use tools they're *already* comfortable with. Agent runners execute in a
sandbox so they can **actually do the thing** — async agents. The goal: make the
whole team **builders**.

## Measuring it

**Evals** are how you measure agent experience — `axis.run` actually tries to
onboard against your product and measure the AX. As this gets more abstract, it
also gets more required.
