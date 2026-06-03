Keep choosing TypeScript — it matters more than ever. LLM output is **not** "the new machine code"; you should still learn a language well, and `TypeScript` is the perfect choice.

## Why the language layer still matters

Look at the world we built for agents: a tower of abstractions. `0/1` at the bottom, then `ASM`, then compilers down to `C`, then `TS`/`Go`/`Python`, then frameworks, then applications. We've spent most of our time in the language layer. People who dabble in predictions imagine squashing the lower languages — even the high-level ones. The truth sits in the middle, between "languages become irrelevant" and "English is a programming language." **English is not a good programming language** (per Paul Graham). If the prediction held, we'd be committing prompts instead of code — and we're not, yet.

## Why TypeScript specifically

Software engineering is **compression** — the time spent compressing personal context. Picture nested, overlapping concepts where `MEMORY` is the outsized layer:

```
types -> code -> docs -> memory
```

Shoot first, ask questions later — code is cheap, right? Imagine there's no TypeScript. **Plan A: documentation.** But docs aren't the harness — non-deterministic, no guarantee they match reality; you've just moved memory into docs. **Plan B: tests.** You move some memory into code via coverage and case exhaustion plus the happy path — but you reduce entropy by adding *more* code, and tests still expect you and the agent to generalize. Pile on tests if you want to throw off 100k lines a day.

## Start with the types

**Then: types.** Move context out of memory and code and into types — if *you* know something, the types should know it too. A prompt is just a specification written in English; step back and define the type **manually**. That's more precise. Fred Brooks: *"Show me your flowcharts and conceal your tables…"* Reach for the right type and you compress memory *and* preserve context — discover the domain *while* you build the model. (`GrillMe` is a skill for refining domain modeling.)

He considered branded types, result types, exhaustiveness, smart constructors, edge type safety — and kept it simpler. `Rust` can do all this and more — maybe more than we need. Meanwhile `TS` keeps evolving: rewritten in `Go`, better errors, legacy baggage shed, still the most popular language.
