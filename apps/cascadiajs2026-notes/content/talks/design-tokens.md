**Design.. not what ends up getting built!**

- `default`, `primary`, `special`
- add `primary` / `muted`

> Hey agent! Use the design system, don't make mistakes

Not everything needs to be in a design system though — some complicated thing that's a one-off, we've just bound an extreme edge case to every implementation.

**Too many decisions in each component!**

All of the different targets need a different language to use the design system.

- give them names
- then give them values
- and give them instructions

**Single source-of-truth** for getting branded targets.

## Design Tokens

- Design Tokens Form Module — capture design decision in the design token, in JSON files
- then define relationships, swap them when we want

Lots of tooling that supports the spec, including `Figma`, `Sketch`, `penpot`, and so on.

Design Tokens Community Group

## Teaching Agents

How do we teach agents to follow these though?

Create skills! `MCP`s! hooks! plugins! `CLI`s! train a model on your design system and tokens.

```
find tokens - PLAN
code       - VERIFY (lint)
validate suggestion
```

A lot of teams are investing time in getting agents to follow design tokens.

No to little design-token tooling for AIs.

**Join the fight against UI slop!!**

`designtokens.org`
