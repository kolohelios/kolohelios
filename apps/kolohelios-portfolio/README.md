# kolohelios-portfolio

Personal portfolio served from a Cloudflare Worker via Workers Static
Assets. Templates render to a committed `dist/` directory that
Cloudflare's edge serves and caches; the Rust worker in `src/lib.rs`
is a `fallthrough` stub for paths not matched by a static asset.

## Layout

- `templates/` — `askama` templates. `layout.html` is the shared base
  (nav + footer); the four page templates extend it.
- `data/work-history.json` — structured work-history entries that
  `templates/work.html` iterates over. Same file feeds #192's `wasm`
  chart when it lands.
- `styles/input.css` — Tailwind v3 entrypoint (just the `@tailwind`
  directives). The compiler scans rendered HTML for class names and
  emits a minimized `dist/style.css`.
- `src/bin/build-site.rs` — native binary (`build-site` feature)
  that renders the templates, runs `tailwindcss`, and writes the
  result to `dist/`. `--check` mode rebuilds into a temp directory
  and diffs against committed `dist/`.
- `src/lib.rs` — `#[event(fetch)]` handler that returns 404 for
  paths the static assets don't match. Dynamic routes (for example,
  `/api/*` for #193's contact form) land here.
- `dist/` — generated, committed. The `build-check` validate step
  re-runs the pipeline in CI and fails on drift.
- `wrangler.toml` — Worker name, `worker-build` invocation, and the
  `[assets]` block that wires `dist/` into Workers Static Assets.

## Why static assets

Cloudflare cache rules (#190) apply to responses that flow through
the HTTP cache phase. Worker-generated responses bound to a custom
domain bypass that phase entirely — so the cache `ruleset` never
engages on inline-rendered Worker output. Serving via `[assets]`
puts the static portion through the cache phase, which is the whole
point of having cache rules.

## Building

```
nix develop .
cargo run --features build-site --bin build-site
```

Updates `dist/` in place. Commit the result; CI rejects drift.

`just validate` runs `build-check` which exercises the same pipeline
in `--check` mode against a temp directory.

## Deploy

`wrangler deploy` builds via `worker-build --release` and uploads
both the (stub) `wasm` module and the committed `dist/` directory.
CI runs the same command on push to main (see
`.github/workflows/kolohelios-portfolio-deploy.yml`); the custom
domain on `kolohelios.com` is managed by Terraform under
`infra/cloudflare-deploy` — not by `wrangler.toml` — so the deploy
and apply concerns stay separate.
