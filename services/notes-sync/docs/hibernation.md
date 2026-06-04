# Durable Object WebSocket Hibernation — design & verification

Phase 1 of #757 de-risks the **WebSocket Hibernation** lifecycle on the
Rust `worker` crate (0.8.3). This note records how the lifecycle is wired,
the one sharp edge found, and how to verify the evict-and-wake cycle.

## Why this is the de-risk

While a note is open its Durable Object is the single writer and source of
truth. Hibernation lets the runtime **evict that object from memory during
idle periods while keeping the socket open**, then reconstruct it (rerun
the constructor) and deliver the next message. If that state lived in struct
fields, eviction would silently lose it. The whole design depends on the
object surviving eviction without loss.

## How the lifecycle is wired (`src/runtime.rs`)

1. **Accept via the hibernation API**, not `ws.accept()`:

   ```rust
   self.state.accept_web_socket(&server);
   ```

   This tells the runtime the socket is `hibernatable`, so the object is not
   pinned in memory.

2. **Per-connection state rides the socket**, not a struct field — struct
   fields don't survive eviction:

   ```rust
   server.serialize_attachment(SocketAttachment { note_id })?;
   // ...on the next message, after a possible eviction:
   let attachment: SocketAttachment = ws.deserialize_attachment()?...;
   ```

3. **Durable state lives in DO storage**, read fresh each message so the
   value is whatever survived in the durable tier:

   ```rust
   let mut seq: u64 = self.state.storage().get("seq").await?.unwrap_or(0);
   seq += 1;
   self.state.storage().put("seq", seq).await?;
   ```

4. **Lean constructor.** `DurableObject::new` reruns on every wake. It does
   no eager I/O — handlers read from storage lazily — so a wake is cheap
   and reads only what it needs. A `console_log!` marks each construction
   so wakes are observable in `wrangler tail`.

The phase-1 object is a hello-world echo: it bumps the storage-backed
`seq` and replies `echo[<note_id>#<seq>]: <text>`. Phase 2 replaces the
`seq` counter with the append-only edit log, but the survival mechanism is
identical.

## Sharp edge found: the `#[durable_object]` macro needs `wasm_bindgen`

The macro expands to JS-glue code that references `wasm_bindgen` by bare
name. Without it in scope the build fails for the `wasm32` target with ~17
cascading, misleading errors (`cannot find module or crate wasm_bindgen`,
`cannot find function inform`, `can't use Self from outer item`, …) that
point at the `#[durable_object]` attribute rather than the missing import.
The fix is a one-line import (the workers-rs `counter.rs` example does the
same):

```rust
use worker::{ /* … */ wasm_bindgen /* … */ };
```

This is the kind of pre-1.0 friction the phase was meant to surface. With
the import in place the crate compiles to `wasm32-unknown-unknown` and
`worker-build --release` produces a clean package. **Verdict: the `worker`
crate supports hibernation cleanly — no fallback to a TypeScript DO is
needed.**

## Verifying the evict-and-wake cycle

The build-time correctness (compiles + `worker-build`) is gated by
`just validate`. The **runtime** evict-and-wake confirmation needs a
running `workerd`, so it is a manual step:

### Local smoke test (`wrangler dev`)

```sh
cd services/notes-sync
wrangler dev            # local workerd; no Cloudflare login needed
# in another shell, drive the socket (any WS client):
websocat ws://127.0.0.1:8787/note/demo/ws
> hello
< echo[demo#1]: hello
> world
< echo[demo#2]: world
```

`seq` incrementing across messages shows the storage round-trip; the
`note_id` in the reply shows the attachment round-trip.

### Observing a true eviction

Local `wrangler dev` does not aggressively evict idle objects, so it
proves the storage/attachment plumbing but not eviction itself. A true
eviction-and-wake is observed against a deployed Worker:

1. `wrangler deploy` (needs the Cloudflare account; credentials come from
   1Password via `op`, never committed).
2. `wrangler tail` in one shell.
3. Open the socket, send a message (`seq` → N), then leave it idle long
   enough for the runtime to evict the object.
4. Send another message. In `wrangler tail` the
   `NoteDurableObject constructed …` line reappears — that is the wake —
   and the reply is `echo[…#N+1]`, proving `seq` survived the eviction in
   storage and the `note_id` survived in the attachment.

If the counter ever resets to 1 after a wake, state is leaking into struct
fields instead of the durable tier — the regression this phase exists to
prevent.
