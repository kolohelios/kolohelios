`Azoth` — open source library.

- Winter 2023–24: built it.
- When AI hit: shelved it.
- Nov 2025: AI changed the math, and brought back `Azoth`!

Creating a dashboard for a real estate brokerage — Google Looker — couldn't find the right thing in the "vending machine."

Visually intensive components — just write HTML and CSS?

Combining a brokerage with a property developer.

Added integration analysis — high user touch.

`JSX` everywhere; the AI defaults to `React` — **same syntax, wrong semantics.**

`Azoth` — without vDOM, controlling renderer, no JS creating the DOM — `<p>Hello</p>` just returns a `p` tag!

First instinct: treat the new framework like a new framework (take an adjacent concept); just use it a bit.

**This backfired!** Use the language, then it confuses the LLM.

## "Don't Say…, Say Instead" Map

Don't confuse our innovation with the incumbent's.

**Don't onboard the LLM: hire it as the new senior maintainer.**

AND… they're curious about your tech!

Shared mental model: a system metaphor that is shared; start a conversation.

Don't describe in negative space against something else — talk about the thing as it is.

## Azoth JSX is real DOM

- What does it compile to? Doesn't interfere with JavaScript.
- Takes the HTML and makes a template (the JS creates no DOM).
- Doesn't use a factory function to make the DOM.
- Add how to bind, small helper functions — deduplicated.

Like the opposite of `React` — what you opt *into* with `React` you opt *out of* with `Azoth`.

## Subtraction, not replacement

Don't replace the vDOM, **subtract it.**

The leap: closures and DOM — reduce to a DOM problem.

`component = constructor`, literally.

LLM, hey, **THIS IS DOM!** Reach for a tool that makes sense for the problem.

`Azoth` basically accepts everything that `JSX` already has. No proprietary primitives, just JS.

**Notice the shape** — this was the unlock. **REMOVE the abstraction.**

New mapping is **SUBTRACT and unlock**, not "don't say, say this instead."

## Prompts as Navigation

Prompts aren't engineering instructions. **They're navigation.**

Push away from what is being subtracted, and pull towards what's unlocked.

Avoid NEGATION, like "Don't think about a pink elephant."

## Law of AI Era Innovation

- **Context** — what you carry into the conversation.
- **Corpus** — what's already in the model — your budget! Be smart about this!

**Subtract your abstractions, unlock the corpus.**
