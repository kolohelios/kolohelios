**Shared Components Beyond the Design System**

`DS` == UX foundations, atomicity.

nav / higher-level UX, team-specific, or design system candidates.

## Who are you building for?

Anchor consumer, then add a second consumer to keep implementation from getting coupled to the anchor consumer's use case.

## Where are you building it?

Keep it separate from the app so it can evolve independently.

Publish artifacts to avoid bad habits (copy-pasta).

## Build it thoughtfully

Use a higher standard than app code.

Make API consistent.

**Common case** — little to no effort; the most custom case will require the most effort (layer 1, 2, 3 with progressive complexity).

Prioritize common use-cases, and don't forget complex but frequent things.

Hard mode can be hard — that's okay.

## Principles

- **Principle 1:** build UP from the design system
- **Principle 2:** design with layered extensibility — **make the right way the easy way** (example: button with icon)
- **Principle 3:** don't build a Homer car
- **Principle 4:** it's okay to be opinionated
- **Principle 5:** consider including batteries — reduce boilerplate, handle data fetching if you can (like `Relay`'s fragments), keep an escape hatch — don't do everything for consumers in case they need to do something themselves
- **Principle 6:** make it maintainable — write thorough tests, document liberally, keep a changelog — what changed and do they need to do anything about it

## Foster an ecosystem

Shared components are super valuable.

Discoverability — doc site / marketplace.

**Good docs matter more than ever.**

## Provide governance

Define patterns and practices.

`Home Assistant` — Quality Scale — bronze / silver / gold.

Recognize authors for their contributions.
