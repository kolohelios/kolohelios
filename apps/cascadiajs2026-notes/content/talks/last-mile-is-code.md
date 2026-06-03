`gc.cpp` — 1 million lines of code transpiled from LISP.

Agents got really good at code, really fast.

Represent any sort of problem as code.

**`SWE Bench`** — Pro agent is contamination-resistant; quality has moved from 33 in Aug '24 to 94; Pro is 78.

40 → 70 this year.

## The Stack Has Changed

The old stack:

```
programming language --compiler--> machine code
```

The new stack:

```
natural language --LLM--> programming language --compiler--> machine code
```

**The hard part isn't the code anymore** — it's everything underneath it: ClickOps, infra, API keys/secrets.

Andrej Karpathy — *"Building a modern app is a bit like assembling IKEA furniture."*

## The Last Mile

For most of this, this part has always been someone else's job (a platform team, an SRE rotation, the person on call).

It's wildly lopsided: agents have either what it was trained on, or it's in context.

**Put the last mile in code, too.**

Take action in code space; a runtime (IaC) reflects it back into the cloud.

`IaC` is like `git diff`, but for your infra — see what it's going to do before it does it.

Vibe infra like apps, by asking — code we can review, approve, and merge in code.

**+20%** — agents do better when they act by writing code, not by emitting config (from *CodeAct — Executable Code Actions Elicit Better LLM Agents*).

Agents now drive **28% of deployments**, up from 4% late last year.

**Shipping isn't a separate team's job anymore.**
