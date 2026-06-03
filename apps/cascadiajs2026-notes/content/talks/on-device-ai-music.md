AI plus `Strudel` — a web-based, code-driven music tool where you make music with code. The goal is to **extend creativity, not replace it**: use AI for the technical bits, not to hand the whole thing over. A small model under the hood adds structural awareness.

The kicker: it all runs in your browser tab. **No API, no cloud — all local.** Why local? Keep cost down (free), reduce latency (no round-trip), and preserve privacy (input never leaves the browser).

## Picking the model

How do you make a small model good at a niche domain? First pick the *type*. Is the problem translation? Audio classification? Text generation? Each lives in a different model family — text generation made the most sense. `transformers.js` runs Hugging Face models in the browser.

## Shaping the I/O

AI is good at ambiguity, inference, unstructured input; code is good at precision, rules, structured output. The right answer sits on a continuum between them, with tradeoffs. Off-the-shelf "just ask for code" looks plausible but won't run. "Intent JSON → deterministic engine" turns intent into pseudo-code, but bigger schemas and prompts add latency and stop feeling fluid. A **fine-tuned model** gives structural context: small input, sanitized and parsed output. Optimize further and you drop the deterministic guarantee for a tighter surface — just the right number of tokens.

## Training data, validated

The model didn't know `Strudel`, and there wasn't much training data. So Alex used LLMs to generate examples — some of it confident nonsense referencing things that don't exist. The fix: **validate, then scale.** Run each example through `Strudel`; if it doesn't parse, throw it out. Don't scale the hallucinations. Add grounding so the model doesn't freestyle, and reward cross-domain reasoning.

The ongoing loop:

```
identify gap --> generate --> validate --> retrain
```

The work gets focused but never stops; each loop makes the tool more like something Alex actually wants to play with. Finally, fit it into a browser tab via **quantization** — shrink by reducing precision. Runtime is a few lines: import, await the pipeline, initialize. The questions to keep asking: what's the model task? where's the AI/code boundary? where does the training data come from? does it need to be local?
