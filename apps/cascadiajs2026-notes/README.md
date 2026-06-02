# cascadiajs2026-notes

Cleaned-up notes from CascadiaJS 2026, served at
`cascadiajs2026.kolohelios.com` from a Cloudflare Worker via Workers
Static Assets. Each talk is its own navigable page; an index groups
talks by day. The Rust worker in `src/lib.rs` is a `fallthrough` stub
for paths not matched by a static asset.

## Layout

- `data/talks.cue` — structured talk metadata (speaker, company,
  title, slug, day, order, optional slides/projects/sources),
  self-validated by `cue export` at build time.
- `content/talks/<slug>.md` — each talk's prose body in Markdown.
- `templates/` — `askama` templates. `layout.html` is the shared base
  (header + attribution footer); `index.html` lists talks by day,
  `talk.html` renders one talk with prev/next navigation.
- `styles/input.css` — Tailwind v3 entrypoint. The compiler scans
  rendered HTML for class names and emits a minimized `dist/style.css`.
- `src/bin/build-site.rs` — native binary (`build-site` feature) that
  exports `data/talks.cue`, renders each talk's Markdown via
  `pulldown-cmark`, fills the templates, runs `tailwindcss`, and
  writes the result to `dist/`. `--check` mode rebuilds into a temp
  directory and diffs against committed `dist/`.
- `src/lib.rs` — `#[event(fetch)]` handler that returns 404 for paths
  the static assets don't match.
- `dist/` — generated, committed. The `build-check` validate step
  re-runs the pipeline in CI and fails on drift.
- `wrangler.toml` — Worker name, `worker-build` invocation, and the
  `[assets]` block that wires `dist/` into Workers Static Assets.

## Attribution

These are personal notes. The more complete community notes at
[`hurrendor/cascadiajs26`](https://github.com/hurrendor/cascadiajs26)
(GPL-3.0) were used as a cross-reference for coverage and accuracy;
no prose is copied from them. Each talk that leans on a community
note links it via the `sources` field in `data/talks.cue`.

## Building

```
nix develop .
cargo run --features build-site --bin build-site
```

Updates `dist/` in place. Commit the result; CI rejects drift.
`just validate` runs `build-check`, which exercises the same pipeline
in `--check` mode against a temp directory.

## Deploy

`wrangler deploy` builds via `worker-build --release` and uploads
both the (stub) `wasm` module and the committed `dist/` directory. CI
runs the same command on push to main (see
`.github/workflows/cascadiajs2026-notes-deploy.yml`); the custom
domain on `cascadiajs2026.kolohelios.com` is managed by Terraform
under `infra/cloudflare-deploy` — not by `wrangler.toml`.
