# kolohelios-portfolio

Personal portfolio served from a Cloudflare Worker via Workers Static
Assets. The static content under `dist/` is served and cached at
Cloudflare's edge; the Rust worker in `src/lib.rs` is a `fallthrough`
stub for paths not matched by a static asset.

## Layout

- `dist/index.html` — single-page "under construction" placeholder.
  Real content lands in #191 (slice #5 of #186).
- `src/lib.rs` — `#[event(fetch)]` handler that returns 404 for
  unmatched paths. Dynamic routes (for example, `/api/*` for #193's
  contact form) land here.
- `wrangler.toml` — Worker name, `worker-build` invocation, and the
  `[assets]` block that wires `dist/` into Workers Static Assets.

## Why static assets

Cloudflare cache rules (#190) apply to responses that flow through the
HTTP cache phase. Worker-generated responses bound to a custom domain
bypass that phase entirely — so the cache `ruleset` never engages on
inline-rendered Worker output. Serving via `[assets]` puts the
static portion through the cache phase, which is the whole point of
having cache rules.

## Deploy

`wrangler deploy` builds via `worker-build --release` and uploads
both the (stub) `wasm` module and the `dist/` directory. CI runs the
same command on push to main (see
`.github/workflows/kolohelios-portfolio-deploy.yml`); the custom
domain on `kolohelios.com` is managed by Terraform under
`infra/cloudflare-deploy` — not by `wrangler.toml` — so the deploy
and apply concerns stay separate.
