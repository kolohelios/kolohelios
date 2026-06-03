`Azoth` is an open-source library Marty built in winter 2023-24, shelved when AI hit, then revived in Nov 2025 — the AI era changed the math and brought it back. The lesson it taught him: you don't onboard an LLM to a new framework, you **subtract abstractions** until the model already knows the problem.

## The setup

Building a dashboard for a real estate brokerage on `Google Looker`, Marty needed a visually intensive component that wasn't in the "vending machine." Why not just write HTML and CSS? Because `JSX` is everywhere and the AI defaults to `React` — **same syntax, wrong semantics**. `Azoth` has no virtual DOM and controls rendering: `<p>Hello</p>` just returns a real `p` element.

## The wrong move

The first instinct was to treat the new framework like a new framework — borrow an adjacent concept, "just use it a bit." It backfired: leaning on the incumbent's language confuses the LLM. So Marty wrote a "Don't say… / Say instead" map. But **describing your thing in the negative space of someone else's thing is still the wrong move** — *don't think about a pink elephant.*

## The unlock: subtract

The real unlock is a shared mental model — a system metaphor you both hold, so you can talk about the thing AS IT IS. Don't onboard the LLM; **hire it as the new senior maintainer**, and they're curious about your tech. `Azoth`'s `JSX` is real DOM: it compiles HTML to a template, the JS creates no DOM, no factory function builds it — you add small, deduplicated binding helpers. What you opt INTO with `React`, you opt OUT of with `Azoth`. It accepts everything `JSX` already has — no proprietary primitives, just JS.

Notice the shape: **remove the abstraction.** Don't replace the vDOM; subtract it. Reduce everything to a DOM problem — a component is, literally, a constructor — and the LLM reaches for a tool that fits.

So prompts aren't engineering instructions; they're navigation. Push AWAY from what you're subtracting, pull TOWARD what you've unlocked. Two budgets matter now: **Context** (what you carry in) and **Corpus** (what's already in the model — spend it wisely). Subtract your abstractions, unlock the corpus.
