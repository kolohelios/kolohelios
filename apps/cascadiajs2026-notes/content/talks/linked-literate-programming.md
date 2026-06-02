When agents write the code, **where does the intent live?** We still need
durable intent.

## The problem with harness memory

Agent harnesses have memory, but it's **tightly coupled** to the harness and
brings lock-in. Markdown files, by contrast, are **portable**. So use markdown
to hold the "memory."

## Linked Literate Programming (LLP)

LLP = **markdown specs + references from code**. Code links back to the spec
with a comment:

```
// @ref LLP 0000mediastream-constructor
```

```markdown
# Constructors [mediastream-constructor]
```

The links matter for three things:

- **Coverage** — you can see whether each part of the spec was implemented.
- **Accuracy** — does the spec actually match the code?
- **Intent** — are decisions visible *above* the code level?

## Web specs are great LLP inputs

Web specs already have the edge cases considered and the APIs designed. Agents
import the spec, then leave linking comments back to it. The same spec can drive
**both web and native** implementations. Pair it with **WPT** (Web Platform
Tests) to close the loop: set up LLP, pull in the relevant parts of a spec, and
test coverage against it. LLP + web specs makes the whole thing more
programmatic.
