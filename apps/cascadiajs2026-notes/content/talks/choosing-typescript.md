**Keep choosing TypeScript** — it matters more than ever.

**LLM output is not "new machine code."**
You should learn a language well, and TS is the perfect choice.

## Why should we care about the language?

Consider the world we have built for the agents: a tower of abstractions, `0`/`1` at the bottom, climb the tower —

```
ASM, compilers -> C, TS/Go/Python -> frameworks -> applications
```

We spent most of our time in the language layer in years past.

People who dabble in predictions — where will they land? Squash lower lower languages, including high-level languages.

The truth is in the middle: between irrelevancy and English being a programming language.

**English is not a good programming language.**

Paul Graham — "… writing in a language…"

We wouldn't be committing code, we would be committing prompts (and we're not, yet).

- `WASP` — framework

## Why TypeScript?

**Software engineering is compression** — time to compress personal context.

```
types -> code -> docs -> memory
```

(overlapping nested concepts)

**MEMORY is outsized in space.**

Shoot first, ask questions later?
Code is cheap? Right?

Imagine there is no TypeScript; what's reasonable as far as compressing complexity?

Did we answer our domain questions? **NO!** Where are the limits, and so on.
So we didn't compress memory into types.

So, hey! Let's add documentation!
We just moved some of our memory to docs.

**Docs are not the harness.**
Docs are not deterministic, and there is no guarantee that docs match reality.

## Plan B: Tests

We can move some memory into code with test coverage and case exhaustion, and add happy path.

Just use JS, not TS, and add a bunch of tests if you want to throw off 100k lines of code a day 🤑

- Reduce entropy by adding extra code.
- Tests expect us *and* the agent to generalize.

## Types

**Move context from memory and code to Types.**
Encode information into the type.

In the naive case, we still don't answer constraint-related questions.

**If you know something, the types should know it as well!**

## What if we start with the questions? With the Types?

Fred Brooks: "Show me your flowcharts and conceal your tables, and I shall continue to be mystified. Show me your tables, and I won't usually need your flowcharts; they'll be obvious."

We could define what we want in a prompt, which is basically a specification.

We should instead take a step back and define our type **MANUALLY**. That's better than using English; it's more precise.

- We can compress our memory AND preserve context by reaching for the right type.
- Think about the questions related to our domain, and then define the appropriate types.
- Discover the domain WHILE building the model.

- `GrillMe` — skill to help refine domain modeling.

He thought about talking about "branded types", result types, ensuring exhaustiveness, smart constructors, edge type safety — decided to make it simpler.

`Rust` can do all of this, and other stuff; it may do stuff we don't need.

## TypeScript continues to evolve

- modern and safer
- rewritten in Go
- better errors
- most popular language
- removed legacy baggage
