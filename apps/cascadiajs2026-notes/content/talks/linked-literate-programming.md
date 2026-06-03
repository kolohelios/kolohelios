When agents write code, where does the intent live?

**We still need durable intent.**

Agent harnesses have memory, but it's tightly coupled — and we have lock-in.

Markdown files, on the other hand, are portable.

Use markdown to create "memory."

**LLP** — markdown specs + references from code.

```
// @ref LLP 0000mediastream-constructor
```

```
# Constructors [mediastream-constructor]
```

## Why links matter

Links matter for coverage, accuracy, and intent.

- **Coverage** — we can see if we implemented each part of the spec.
- **Accuracy** — does the spec actually match the code?
- **Intent** — are decisions visible *above* the code level?

## Web specs as inputs

Web specs are good inputs for LLP:

- edge cases already considered
- designed APIs

Agents import the spec, then use linking comments back to the spec.

**LLP pattern** — implementation for web and native apps using the same spec??

## Workflow

Set up LLP, get the relevant parts of a spec, test coverage.

`WPT` — Web Platform Tests.

Make a feedback loop with LLP.

**LLP + web specs** — more programmatic.
