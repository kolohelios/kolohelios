# kolohelios-portfolio

Personal portfolio served from a Cloudflare Worker. Rust + `askama`; the
WASM module ships to `*.workers.dev` via `wrangler deploy`, and the
generated TF at
`infra/cloudflare-deploy/terraform/generated/kolohelios-portfolio.tf`
attaches `kolohelios.com` as the Worker custom domain.

## Layout

- `src/lib.rs` — `#[event(fetch)]` handler that renders the `askama`
  template.
- `templates/index.html` — single-page "under construction" placeholder
  (slice #3 of #186 — the content is the *next* slice's problem).
- `wrangler.toml` — Worker name + `worker-build` invocation that wrangler
  drives.

## Deploy

`wrangler deploy` builds via `worker-build --release` and uploads to the
account's `*.workers.dev`. The custom domain on `kolohelios.com` is
managed by Terraform under `infra/cloudflare-deploy` — not by
`wrangler.toml` — so the deploy and apply concerns stay separate.
