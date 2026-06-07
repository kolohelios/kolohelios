# kolohelios-portfolio

Personal portfolio served from a Cloudflare Worker via Workers Static
Assets. Templates render to a committed `dist/` directory that
Cloudflare's edge serves and caches; the Rust worker in `src/lib.rs`
is a `fallthrough` stub for paths not matched by a static asset.

## Layout

- `templates/` — `askama` templates. `layout.html` is the shared base
  (nav + footer); the five page templates extend it.
- `data/work-history.json` — structured work-history entries that
  `templates/work.html` iterates over. Same file feeds #192's `wasm`
  chart when it lands.
- `data/profile.json` — **generated**, committed. Profile prose
  (summary, skills, education) that `templates/about.html` renders. It
  is produced from `tools/resume/profile.cue` by `shaka profile
  generate` — the same canonical source the résumé renders from — so the
  about page never hand-duplicates profile content. See
  [Résumé dependency](#résumé-dependency).
- `templates/resume.html` — the `/resume` page. Its body is
  `tools/resume/resume.md` rendered to HTML at build time (see
  `build-site.rs`), so the page never hand-duplicates résumé content;
  it also links the downloadable `resume.pdf` / `resume.docx` copied
  from that project. See [Résumé dependency](#résumé-dependency).
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
  re-runs the pipeline in CI and fails on drift. Includes the
  `resume.{pdf,docx}` copies and the rendered `resume/index.html`.
- `wrangler.toml` — **generated** from the `wrangler:` block in
  `project.cue` by `shaka deploy generate-wrangler` (`shaka preflight`
  fails on drift). Carries the Worker name, `[assets]` block, `[vars]`,
  and the `[[unsafe.bindings]]` rate limiter. Never hand-edit it — change
  `project.cue` and regenerate.

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

## Résumé dependency

The `/resume` page and the `resume.{pdf,docx}` downloads are sourced
from the sibling `tools/resume` project: `build-site.rs` reads
`../../tools/resume/resume.md` (rendered to HTML for the page) and
copies that project's committed `resume.pdf` / `resume.docx` into
`dist/`. So a résumé change requires rebuilding and committing this
project's `dist/` too.

That cross-project staleness is gated outside this project's own
`build-check`: a repo-level `shaka preflight` check byte-compares
`tools/resume/resume.{pdf,docx}` against the copies committed here, and
fires whenever *either* side changes (the per-project `build-check`
only runs when this project's files change). Because `tools/resume`'s
own drift check ties `resume.md` to its rendered `PDF`/`DOCX`, the
markdown can't change without those changing — so guarding the
`PDF`/`DOCX` copies also guards the rendered page.

The about page's profile prose is shared the same way: `data/profile.json`
is generated from `tools/resume/profile.cue` by `shaka profile generate`,
and a repo-level `shaka profile generate --check` gates it against the
canonical CUE so a `profile.cue` edit that isn't propagated here fails CI.
`build-site` reads the committed JSON directly (no `cue` at build time),
so `build-check` covers `about.html` drift the same way it covers every
other rendered page.

## Deploy

`wrangler deploy` builds via `worker-build --release` and uploads
both the `wasm` module and the committed `dist/` directory.
CI runs the same command on push to main (see
`.github/workflows/kolohelios-portfolio-deploy.yml`); the custom
domain on `kolohelios.com` is managed by Terraform under
`infra/cloudflare-deploy` — not by `wrangler.toml` — so the deploy
and apply concerns stay separate.

## Kit setup (contact + newsletter)

The `/contact` page posts both forms to the worker's `/api/subscribe`,
which proxies to [Kit](https://kit.com) so the API key never reaches the
browser. Provisioning is a one-time manual step:

1. In Kit, create two **forms** — one for contact, one for the
   newsletter — and a **custom field** named `message` (the contact
   form sends the message body into it).
2. Copy each form's numeric id into `project.cue`'s `wrangler.vars`
   (`KIT_FORM_ID_CONTACT`, `KIT_FORM_ID_NEWSLETTER`), then run
   `shaka deploy generate-wrangler`. Form ids are not secret.
3. Set the API key as a Worker secret (not committed):

   ```
   wrangler secret put KIT_API_KEY
   ```

   The deploy workflow does not push Worker secrets yet (#794), so set
   it once against the production worker.
4. Enable double opt-in on both Kit forms so Kit owns confirmation and
   unsubscribe/compliance.

Spam handling lives in the worker (honeypot field + per-IP rate limiting
via the `SUBSCRIBE_RATE_LIMITER` binding + server-side email
validation); Turnstile/hCaptcha is intentionally deferred.
