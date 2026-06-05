# notes-web

The front-end Worker for the note app: an HTMX shell that hosts the
`notes-editor` Rust-WASM editing surface. Served as Workers Static Assets
(`[assets]` in `wrangler.toml`) — Cloudflare serves `dist/` at the edge
and only unmatched paths fall through to the 404 stub in `src/lib.rs`,
the same pattern as the other asset-serving apps.

The editing and sync logic runs entirely in the browser-wasm editor over
a WebSocket to the `notes-sync` Durable Object; this Worker carries no
dynamic logic. The session cookie minted by `notes-sync`'s `OAuth` flow
rides the upgrade and gates it.

## Assets

- `dist/index.html` — the HTMX shell; loads the editor module and starts
  it against `wss://<backend>/note/<id>/ws`.
- `dist/style.css` — the shell styling.
- `dist/editor/` — the built editor bundle. **Not committed** (it's a
  build artifact of `apps/notes-editor`); populate it before deploy:

  ```sh
  (cd ../notes-editor && just wasm-build)
  mkdir -p dist/editor && cp ../notes-editor/dist/* dist/editor/
  ```

The `notes-sync` backend host is injected at deploy as
`window.NOTES_BACKEND` (defaults to the page origin for local
development).

## Status

Phased build tracked in issue #757; the front end lands in #766. A custom
domain (`serving` / `deploy`) is wired once a host name is registered in
`infra/cloudflare-dns/domains/`.
