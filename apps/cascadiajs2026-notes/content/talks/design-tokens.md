Design is not what ends up getting built. You've got `default`, `primary`, `special` — add `primary`/`muted` — and then you tell the agent "use the design system, don't make mistakes." Easier said than done.

## Not everything belongs in a design system

Bind some complicated one-off edge case to every implementation and you've made a mess — too many decisions packed into each component. And every target needs a different language to consume the design system. So: **give the decisions names, give the names values, and give them instructions.** That's a single source of truth for branded targets — design tokens.

The **Design Tokens Format Module** captures each design decision as a token in JSON, then defines relationships so you can swap values when you want. Lots of tooling already supports the spec — `Figma`, `Sketch`, `Penpot`, and more — coordinated by the **Design Tokens Community Group**.

## Teaching agents to follow them

But how do we teach *agents* to follow them? Create skills, MCPs, hooks, plugins, CLIs; train a model on your design system and tokens. The loop:

```
find tokens (PLAN) --> code (VERIFY, e.g. lint) --> validate the suggestion
```

A lot of teams are investing in getting agents to follow design tokens — and there's little-to-no design-token tooling aimed at AIs yet.

Join the fight against UI slop. `designtokens.org`.
