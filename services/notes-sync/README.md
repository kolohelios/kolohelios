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
  optimistic retry on a stale ref. Git is also the **vault listing**:
  `GET /vault` learns which notes exist by walking the tree.

Why *both* tiers exist (and the freshness gap the split creates) is
written up in [`docs/storage-tiers.md`](docs/storage-tiers.md).

The AI-native vault built on top of these tiers — one source of truth
(the note body), with title/tags/links/embeddings/graph all *derived* and
content-hash-keyed so drift is detectable and self-healing — is specified
in [`docs/vault-architecture.md`](docs/vault-architecture.md) (epic #989).

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

- **Phase 1 — hibernation de-risk.** Stood up the `hibernatable`
  WebSocket Durable Object and confirmed the eviction-and-wake lifecycle
  on the `worker` crate: durable state lives in DO storage and
  per-connection state in the socket attachment, never in struct fields.
  See [`docs/hibernation.md`](docs/hibernation.md) for the verification
  procedure.
- **Phase 2 — edit log.** The editor speaks the `notes-protocol` wire
  types over the socket. On `Open` the object replies with a `Sync` of the
  current `seq` and `text`; an `Edit` whose `base_seq` matches is applied,
  appended to the append-only log in DO storage, and `Ack`ed, while a
  stale edit (or one that can't apply) is rejected with a fresh `Sync`.
  The body is rebuilt by replaying the log, so an evicted object
  reconstructs without loss.
- **Phase 3 — git cold tier (this).** Edits keep persisting to DO storage
  synchronously; the body is committed to GitHub lazily — a `debounce`
  (commit shortly after the last edit, coalescing a burst) and a
  `backstop` (commit at least once per backstop interval under continuous
  editing) multiplexed onto the DO's single alarm, plus a commit when the
  last socket disconnects. The exact intervals are the `COMMIT_DEBOUNCE` /
  `COMMIT_BACKSTOP` constants in `src/runtime.rs`. Each commit is a single-file write to the contents
  API with an optimistic retry when the ref moves; a `BackedUp` is then
  broadcast to connected editors. The repo/branch live in `wrangler.toml`;
  the commit token is a `wrangler secret` (`GITHUB_TOKEN`).

## Local development

```sh
cd services/notes-sync   # direnv loads the flake
just validate            # the same gates shaka preflight runs
wrangler dev             # local workerd; see docs/hibernation.md
```
