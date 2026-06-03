**Choosing the Wrong Abstraction (And What It Cost Us)**

A component that starts simple — and it gets out of control!

What went wrong? What else could have been done?

`Material UI` — when you start, it's fantastic!

```
Material UI -> Base UI
```

Components that can be composed.

2016 essay — Mike Bostock — *What Makes Software Good?*

**Programming interfaces are user interfaces. Programmers are people, too.**

Form must communicate function.

Functions that take many arguments are not good functions.

`chart.js` vs `D3`

```
vending machine vs kitchen
```

## What IS the "right" abstraction?

Configuration-type APIs vs composable APIs.

**Composable APIs are INDEPENDENT and STACKABLE.**

- **Independent** — each part does one thing well; `select`, `find`, `extend`; `read`, `filter`, `count`. Lego blocks.
- **Stackable** — the type of the input == the type of the output — stackkkkkkk!!!

*Composition is All You Need* — Fernando Rojo — 2025 React Universe Conf.

"isn't this a bit complex?"

**Composable APIs AREN'T FREE.**

Configuration is linear, but composition is log(n).

**Lower the cost of composition!**

Why do we have the developers spend all their time learning the lego blocks?

Lower the cost:

- good docs
- good idioms
- defaults
- errors that teach
- types
- consistency
- discoverability
- examples

## Provide an abstraction ladder

- lowest rung — low-level code for constructing views
- higher rung — visual editor

Gives a tradeoff of getting started quickly and getting more power.

`D3` vs `Plot`

`chart.js` vs `Observable Plot`

You should be able to descend or ascend the ladder without having to start all over.

```
const chart = Plot.plot()
d3(chart)   // now descending the ladder
```

```
shadcn -> BaseUI -> HTML/CSS
```

## Can't we just vibe code this?

*Coding After Coders* — NYT magazine.

CEO of Google said devs only working 10% faster; startup is 20x?? Brownfield vs greenfield is the reason.

**AI-ready** — open code for LLMs to read, understand, and improve.

`decepulis/ax-bench` — did better with a composable API.

**Good DX is USUALLY good AX.**

```
|—————————————-|
CONFIG          COMPOSE
defaults        options
```

Add ladder.

Escape hatches are a config trap.

`VideoJS`

- swapping lego blocks
- eject skin to get primitives

The right abstraction leads to smaller bundles (for JS, with tree-shaking at least).
