# notes-editor

The live document surface and socket client for the note app, compiled to
**browser-wasm** (`web-sys` / `wasm-bindgen`). First occupant of the
`wasm-app` kind. Served as a static bundle by `apps/notes-web`.

The pure [`client`](src/client.rs) logic — the document state and the
protocol decisions — is native-tested off the browser glue. The wasm-only
[`dom`](src/dom.rs) entrypoint wires a `<textarea>` and a `WebSocket` to
it. The editor and the `notes-sync` Durable Object share the
`notes-protocol` types, so the two ends serialize the same bytes.

## Protocol

On connect the editor sends `Open { since_seq }` and adopts the `Sync`
the server returns. Each keystroke sends `Edit { base_seq, delta }` with
the whole body as the delta (the always-correct v1 fallback); the server
replies `Ack { seq }`, or a fresh `Sync` if the edit was stale. A
`BackedUp` tells the editor the note reached git. On a dropped socket the
client reconnects and syncs again from scratch, so no edits are lost.

The session cookie minted at login (phase 4) rides the WebSocket upgrade
automatically and gates it server-side.

## Build

`just wasm-build` compiles to `wasm32-unknown-unknown`, runs `wasm-bindgen`
for the JS glue, and `wasm-opt` to shrink — emitting `dist/` (ignored by
git). `apps/notes-web` serves that bundle.

Phased build tracked in issue #757; the editor lands in #766.
