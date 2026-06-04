# notes-sync

The Cloudflare backend for a web-based, Obsidian-like note app whose
defining property is a live backup with **one source of truth and no
reliance on browser storage**. First occupant of the `services/` slot.

Each note maps to one **Durable Object** via `idFromName(noteId)`. While
a note is open, that object is the single writer and source of truth. The
editor streams edits over a `hibernatable` WebSocket; the object persists to
its own storage on a short cadence and commits to a GitHub repo lazily.

## Storage tiers

- **Hot (DO storage).** A SQLite-backed Durable Object holds an
  append-only edit log, so an evicted object replays without loss on wake.
  This is the fast-durable tier; the fast cadence never touches git.
- **Cold (Git/GitHub).** Current note state plus full history. Commits are
  lazy (a long alarm and on last-socket-disconnect), single-file, with an
  optimistic retry on a stale ref.

## Wire protocol

Types live in the shared `packages/notes-protocol` crate (phase 2) so the
Durable Object and the WASM editor share one set of `serde` structs. The
client sends `open { sinceSeq }` and `edit { baseSeq, delta }`; the server
sends `ack { seq }`, `sync { seq, text }`, and `backedUp { commitSha? }`.

## Auth

Sign-in is `ATProto` `OAuth`, **authentication only** (minimal `atproto`
scope, never write scope). The Worker serves the client-metadata JSON and
the callback, resolves handle → DID → PDS → authorization server, runs the
flow, verifies `sub` against the resolved DID and issuer, then discards
the `atproto` tokens. It mints its own signed session cookie; that cookie
gates the WebSocket upgrade. The GitHub commit credential is an unrelated
static server-side secret in 1Password.

## Status

Phased build tracked in issue #757.

- **Phase 1 (this scaffold).** Hello-world `hibernatable` WebSocket echo
  Durable Object that de-risks the hibernation eviction-and-wake cycle on
  the `worker` crate. A `/note/<id>/ws` upgrade is forwarded to that
  note's object, which echoes each message and bumps a storage-backed
  `seq` counter; per-connection state rides the socket attachment so it
  survives hibernation. See [`docs/hibernation.md`](docs/hibernation.md)
  for the eviction-and-wake verification procedure.

## Local development

```sh
cd services/notes-sync   # direnv loads the flake
just validate            # the same gates shaka preflight runs
wrangler dev             # local workerd; see docs/hibernation.md
```
