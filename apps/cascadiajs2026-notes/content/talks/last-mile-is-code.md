Agents got really good at code, really fast — and you can represent almost any
problem as code (one demo: `gc.cpp`, a million lines transpiled from LISP).

On SWE-bench, the contamination-resistant **Pro** variant has moved fast:
quality from ~33 (Aug '24) to 94, with Pro at 78 — and roughly 40 → 70 over the
year.

## The stack shifted

```
old: programming language --compiler--> machine code
new: natural language --LLM--> programming language --compiler--> machine code
```

The hard part **isn't the code anymore** — it's everything underneath it:
ClickOps, infra, API keys, secrets. As Karpathy put it, *"Building a modern app
is a bit like assembling IKEA furniture."*

## The last mile

For most of this, the last mile has always been **someone else's job** — a
platform team, an SRE rotation, the person on call. And it's wildly lopsided:
the agent only has what it was trained on, or what's in context.

So **put the last mile in code, too.** Take action in code space; a runtime
(IaC) reflects it back into the cloud. **IaC is like `git diff` for your infra**
— you see what it's going to do before it does it. Vibe infra like apps: ask for
code you can review, approve, and merge.

This isn't just ergonomics. Per *CodeAct (Executable Code Actions Elicit Better
LLM Agents)*, agents do **~20% better** when they act by **writing code** rather
than emitting config. The shift is already underway: agents now drive **28% of
deployments**, up from 4% late last year. **Shipping isn't a separate team's job
anymore.**
