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

## Derived vault index and connections

`VaultDurableObject` (`VAULT`, owner-scoped) persists disposable per-note
metadata, vectors, a refresh queue, and cached connections. It is not a
second note authority. Refresh reads the note DO's snapshot; a note with no
hot edit log falls back to its Git body. Rebuilding never writes into the
note's edit log. `GET /vault` remains the existing portable Git listing.

Accepted edits persist an index-delivery flag alongside the edit log.
Delivery runs on the existing commit/disconnect cadence, independently of
whether the Git backup succeeds; rename seeds and deletions notify too.
The Vault DO processes up to eight queued notes per alarm. Work survives
eviction, and stale HTTP results cannot overwrite a newer queued refresh.
Failed derivations get three attempts; later reads can retry missing or
stale artifacts. There is no periodic vault scan.

Owner-session-authenticated endpoints:

- `POST /vault/rebuild` discovers Git note paths and queues those plus
  existing index records, reporting a count of skipped unsupported ids.
  Use this once to backfill an existing vault, or
  to rebuild disposable records. A supplied browser Origin must match the
  configured app or backend base URL.
- `GET /vault/index` returns indexed ids, title/tags/aliases, cache keys,
  and source/embedding status, without bodies or vectors.
- `GET /vault/connections?id=<note-id>` returns the five nearest cosine
  neighbors (excluding self), resolved `[[wikilink]]` ids, and unresolved links.
  Exact ids win; otherwise basename/title/alias must resolve uniquely.
  Code spans, fenced code, and existing Markdown links are excluded.

Embedding keys contain the full canonical body hash and a model identity
qualified by provider endpoint and preprocessing recipe. Metadata-only and
cosmetic edits reuse vectors while still refreshing titles, aliases, and
links. Changed bodies, models, or endpoints invalidate vectors. Known
pending/failed source refreshes are marked `source_stale`; their vectors
and outgoing links are not served as fresh. `embedding_fresh` means the
vector matches the indexed content/model key, **not** that the index has
been compared to every uncommitted edit. A read racing an index change
returns HTTP 409; retry shortly. Connection caches are generation-keyed,
so a changed/deleted target or alias invalidates incoming edges too.

The existing `OPENROUTER_API_KEY` secret enables embeddings. Optional Worker
vars `OPENROUTER_EMBEDDINGS_MODEL` and `OPENROUTER_EMBEDDINGS_ENDPOINT`
default to `openai/text-embedding-3-small` and
`https://openrouter.ai/api/v1/embeddings`. An OpenAI-compatible replacement
can use the same adapter; credentials stay in the secret, not in the URL.
Without a key, metadata/wikilinks still work and vectors remain missing.

Long notes are batched in 2,000-character chunks and pooled using a
length-weighted mean of unit vectors; the whole canonical body contributes.
Limits are 128 chunks and 4,096 vector dimensions. Notes that exceed these
limits, or whose provider returns an invalid or error response, keep a
missing vector rather than silently truncating text or claiming freshness. Similarity is brute-force at
personal-vault scale; no external vector database is required.

This implements #993's shared index/embedding seam. Open-time drift
verification, Git-push-triggered reconciliation, repair diagnostics, and
full-text/semantic search remain #996.

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
