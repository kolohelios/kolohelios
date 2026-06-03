**AI and Strudel** — web-based music production tool.

**Make music with code!**

Extend creativity, not replace it — using AI to get help, not handing something to AI to do.

**Structural awareness** — a small model under the hood helping with the technical bits.

**In your browser tab!** No API, no cloud — all local.

## Why local

Alex wants to:

- **keep cost down** — free!
- **reduce latency** — beats having a round-trip
- **privacy** — input never leaves the browser

So that's why local.

## Getting a small model good at a niche domain

How do you get a small model to be good at a niche domain?

Pick a model — but first pick *which type* of model?

- `transformers.js` — HuggingFace models to run in the browser.

Is this problem translation? Is it audio classification? Each one is in a different model family.

Text generation made the most sense; what does the interface look like?

Shaping the model's I/O —

- **AI** good at ambiguity, inference, unstructured input
- **Code** good at precision, rules, and structured output

The right answer depends on the need.

## Finding the AI/code boundary

- **Off-the-shelf model: ask for code** — NOT right! Looks plausible, but won't run.
- **Try intent JSON** — deterministic engine; take intent and make pseudo code.

A continuum of AI `<——>` code boundary, with tradeoffs between them.

Latency to get a structured output; bigger schema, bigger prompt — doesn't feel fluid anymore.

- **Try fine-tuned** -> structural context. Keep input context small; model output is sanitized and parsed.
- **Optimized** — remove the deterministic guarantee for tighter and smaller surface area, just the right amount of tokens.

But the model didn't know Strudel! Training took a lot more time.

## Training data

There wasn't a lot of training data available — didn't have time to create a lot of examples.

Alex used LLMs to generate examples; ask for Strudel, see what it does.

Some things didn't actually exist — **confident nonsense.**

- Added validation step — run it through Strudel, throw it out if it doesn't parse.
- Then scale — **DON'T scale the hallucinations.**

Added grounding to what the LLM is being asked to do; don't freestyle, be concise — what is Strudel.

Cross-domain reasoning.

## The ongoing loop

The work gets focused but doesn't stop; identify gap -> generate -> validate -> retrain.

```
identify gap -> generate -> validate -> retrain
```

Each loop makes the tool better — to become something Alex wants to work with.

## Shrink it to fit

Make something small enough to fit in a browser tab — **shrink it small, ship it small.**

**Quantization!**

- Shrink down by reducing precision.
- Runtime in a few lines — import, await pipeline, initialize.

**Ideas are endless.**

## Questions to ask

- Model task?
- AI/code boundary?
- Training data?
- Does it need to be local?
