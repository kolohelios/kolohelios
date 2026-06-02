A war story: a component starts simple and gets out of control. What went
wrong, and what else could we have done?

Material UI is fantastic *when you start*. Then you outgrow it — the migration
path was Material UI → Base UI (components you can compose).

## Programming interfaces are user interfaces

From Mike Bostock's 2016 essay *What Makes Software Good?*:

> Programming interfaces are user interfaces. Programmers are people, too.

- **Form must communicate function.**
- Functions that take many arguments are not good functions.
- `chart.js` vs. D3 is a **vending machine vs. a kitchen**.

## Configuration vs. composition

The "right" abstraction question is really config-type APIs vs. composable
APIs. **Composable APIs are independent and stackable:**

- **Independent** — each part does one thing well (select, find, extend; read,
  filter, count). Lego blocks.
- **Stackable** — the type of the input equals the type of the output, so they
  stack. (cf. Fernando Rojo's *Composition Is All You Need*, React Universe
  Conf 2025.)

But composable APIs **aren't free.** Configuration cost is linear; composition
is log(n) — and someone asks "isn't this a bit complex?" So **lower the cost of
composition**: good docs, good idioms, sensible defaults, errors that teach,
types, consistency, discoverability, examples.

## The abstraction ladder

Don't make developers spend all their time learning the Lego blocks. Provide a
ladder:

- **Lowest rung** — low-level code for constructing views.
- **Higher rung** — a visual editor.

You should be able to ascend or descend **without starting over**:

```js
const chart = Plot.plot()
d3(chart) // descend the ladder to the lower-level primitive
```

Examples: D3 ↔ Plot, chart.js ↔ Observable Plot, shadcn → Base UI → HTML/CSS,
Video.js (eject the skin to get the primitives). **Escape hatches are a config
trap** — design the ladder instead.

## "Can't we just vibe-code this?"

Per *Coding After Coders* (NYT Magazine): the Google CEO said devs are only ~10%
faster, while a startup claimed 20×. The difference is **brownfield vs.
greenfield.** Make code **AI-ready** — open for LLMs to read, understand, and
improve. `decepulis/ax-bench` did better with a composable API. **Good DX is
usually good AX.** And the right abstraction leads to smaller bundles (with
tree-shaking, at least).
