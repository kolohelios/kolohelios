Shared components that live *beyond* the design system. The design system is
UX foundations and atomicity; above it sits navigation, higher-level UX,
team-specific components, and design-system candidates.

## Before you build

- **Who are you building for?** Anchor on one consumer, then add a *second*
  consumer early so the implementation doesn't get coupled to the anchor's use
  case.
- **Where are you building it?** Keep it separate from the app so it can evolve
  independently, and publish artifacts to avoid copy-paste habits.
- **Build it thoughtfully** — hold it to a higher standard than app code, and
  keep the API consistent. The common case should take little to no effort; the
  most custom case will take the most effort. Layer the complexity (1 → 2 → 3),
  prioritize common use cases, and don't forget the complex-but-frequent ones.
  Hard mode is allowed to be hard.

## Six principles

1. **Build up** from the design system.
2. **Design with layered extensibility** — make the right way the easy way (e.g.
   a button with an icon).
3. **Don't build a Homer car** — don't cram in every feature.
4. **It's okay to be opinionated.**
5. **Consider including batteries** — reduce boilerplate, handle data fetching
   where you can (like Relay's fragments), but keep an escape hatch so consumers
   can do it themselves when needed.
6. **Make it maintainable** — thorough tests, liberal docs, and a changelog
   that says what changed and whether consumers need to act.

## Ecosystem and governance

Shared components are valuable, so invest in **discoverability** — a docs site
or marketplace; good docs matter more than ever. Provide **governance**: define
patterns and practices, use a quality scale (Home Assistant's bronze / silver /
gold is a nice model), and **recognize authors** for their contributions.
