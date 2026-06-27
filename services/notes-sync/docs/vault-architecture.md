# Vault architecture: one source of truth, everything else derived

This is the spec the AI-native vault epic (#989) follows. It expands the
epic's governing principle — **no drift** — into the concrete data model,
the storage shape, and the phase map, so each sub-issue can be read against
a single design rather than re-deriving it.

It sits on top of two existing notes:
[`storage-tiers.md`](storage-tiers.md) (why there is a Durable Object *and*
git) and [`hibernation.md`](hibernation.md) (how the live socket survives
eviction). Read this one for *what the data means*; read those for *where
the bytes live*.

## The hard requirement: derived data can never silently drift

The vault grows a lot of computed structure — titles, tags, folders,
`[[wikilinks]]`, embeddings, similarity edges, the graph. The failure mode
that kills a system like this is **silent drift**: the cached title says
one thing, the note body says another, and nothing notices. Every decision
below exists to make drift *structurally impossible to hide* rather than
merely discouraged.

The principle, stated once:

> There is exactly one source of truth — the note body text. Everything
> else is a pure function of it, keyed so that staleness is detectable and
> self-healing.

## One source of truth: the note body

The single authority for a note is its **body text**: the sequence of
characters the user typed. Concretely that is the Durable Object's
append-only edit log, materialized to `(seq, text)` and mirrored to git as
`notes/<path>.md` (the cold tier — see `storage-tiers.md`). The DO treats
the body as **opaque text**: it assigns sequence numbers, runs the
stale-edit check, and replays deltas, but it never parses the contents.
That opacity is the sync/replay invariant, and the whole metadata model is
built to preserve it.

Everything else — title, tags, folder placement, links, embeddings,
similarity edges, the rendered graph — is **derived**: a pure function of
the body (and, for the AI artifacts, of a named model). None of it is a
second source of truth. If a derived artifact disagrees with the body, the
body wins and the artifact is wrong, by definition.

## The content hash: the key everything derived is stamped with

Derived data is only safe if you can *tell* when it has gone stale. The
mechanism is a canonical **content hash** of the body, and every cached
artifact records the triple it was computed from:

```
(note_id, content_hash, model_id)
```

A cached artifact is **fresh** exactly when `content_hash` still equals the
hash of the current body *and* `model_id` still equals the configured model.
Otherwise it is **stale** and must be recomputed. Staleness is therefore
*detectable* — a test you can evaluate — not something you hope holds.

The hash is defined in `notes-protocol` (`frontmatter::content_hash`, phase
A): **sha256 over the normalized body, excluding the front-matter block.**
Normalization (CRLF → LF, strip trailing per-line whitespace, strip
trailing blank lines) means cosmetic whitespace churn does not invalidate
the whole derived index, and excluding front-matter means a **metadata-only
edit — retitling, re-tagging — does not change the hash.** That last
property is what lets the system *write derived metadata back into the
note* (next section) without the write invalidating the very computation
that produced it.

Three consequences follow directly, and they are the payoff of the whole
scheme:

- **Incremental recompute.** Only notes whose hash changed need their
  derived data rebuilt. The common edit touches one note.
- **Disposable cache.** The entire derived index can be deleted and
  rebuilt from the notes alone. The cache is an optimization, never a
  source of truth — so a corrupted cache is a non-event.
- **Detectable, healable drift.** A `--check`/repair pass (the data analog
  of `shaka project generate-justfiles --check`) can walk every note,
  recompute the hash, and flag or heal any artifact whose stored hash no
  longer matches. Phase G.

## Front-matter: human metadata that can't drift

Human-authored metadata — `title`, `tags`, `aliases` — lives in a YAML
**front-matter** block at the head of the note body:

```
---
title: My Note
tags: [rust, notes]
aliases: [scratchpad]
---
the note body…
```

The shape is defined twice, deliberately, so the two halves can't drift
from each other: the Rust struct `notes_protocol::frontmatter::FrontMatter`
and an open CUE schema (`packages/notes-protocol/schema/frontmatter.cue`).
The schema lives beside the crate rather than under a central schema tree
because it is one half of the same contract as the struct — the crate's
integration test `cue vet`s fixtures against the schema *and* round-trips
them through the struct, so a schema change and the matching struct change
land in one commit. The shape mirrors Obsidian's front-matter so a vault
stays portable to any Markdown tool — and that portability is load-bearing:
both halves *accept and preserve* unknown keys (the schema is open with
`...`; the struct captures foreign keys with `#[serde(flatten)]`), so
writing a title back into a note that carries `created`, `cssclasses`, or
any custom Obsidian key never strips that key. Only the known fields
(`title`, `tags`, `aliases`) are type-checked.

Why front-matter rather than a side table:

- **It is part of the source of truth.** Front-matter is *inside* the body
  text, so it cannot drift from the body the way a separate metadata store
  could — there is no second store. It travels with the note in git and is
  Obsidian-portable. Open the `.md` in any editor and the metadata is right
  there.
- **It is git-native and human-readable.** Versioned, reviewable in a diff,
  and editable by hand, exactly like the prose.
- **The DO stays oblivious.** Because front-matter is just leading bytes of
  the opaque body, the Durable Object's replay/sync invariant is untouched.
  It never learns what front-matter is.

The one rule that keeps it honest: **derived metadata is written back
*through the DO as a delta*** (the single-writer edit path), never as an
out-of-band git write. When phase B auto-names an untitled note, it computes
a title and applies a front-matter edit on the live sequence, exactly as if
the user had typed it. The write flows through `seq` assignment and the
edit log like any keystroke, so there is no second writer and no way for git
and the DO to disagree. And because the content hash excludes front-matter,
writing the title back does not mark the body stale — the auto-naming doesn't
trigger its own re-run.

## OpenRouter for chat and embeddings (no lock-in)

The AI work — auto-naming/auto-filing (chat completions) and embeddings —
goes through **OpenRouter**, one client over `worker::Fetch` (transport
shaped like `git.rs`'s `WorkerGitHubClient`; `serde` shapes adapted from
`apps/blogctl/src/openrouter.rs`). Base URL and model are **configurable**,
and `model_id` is part of every cache key. So switching providers or models
is a configuration change plus a recompute (the changed `model_id` marks the
old artifacts stale and they heal themselves) — never a data migration.

Cloudflare Workers AI and `Vectorize` are **deliberately not used**: they are
lock-in, and the brute-force approach below is sufficient at personal-vault
scale.

## The Vault DO and brute-force cosine (no `Vectorize`)

The cross-note index — the registry of which notes exist, their embedding
vectors, and the similarity edges between them — lives in a **Vault Durable
Object**. Similarity is **brute-force cosine** over the stored vectors: at
personal-vault scale (thousands of notes, not millions) a linear scan is
fast enough and carries no proprietary dependency. The Vault DO can, like
every derived store, **be rebuilt from the notes** — embeddings are
content-hash + `model_id` keyed, so a cold rebuild just re-embeds each note
once.

This also subsumes the "vault freshness gap" mitigation from
`storage-tiers.md`: promoting the index to a hot-tier `Vault` DO is option
2 there, and the AI layer needs that registry anyway.

## Frontend: Rust-WASM editor, HTMX enhancer

The editing surface is the **Rust-WASM editor** (`apps/notes-editor`) — the
same crate that shares `notes-protocol` types with the DO, so the two ends
can't drift on the wire. **HTMX is the progressive enhancer** for the chrome
around it (sidebar, login, vault tree). Two things layer on top as
enhancements, not rewrites:

- **Markdown rendering** (phase F): a Rust-WASM render path with an
  edit/reading toggle; front-matter hidden in the rendered view;
  `[[wikilinks]]` clickable.
- **The graph** (phase E): **2D force-directed first**, a 3D toggle later.

Phase A makes a first, small move here: the editor now surfaces the
front-matter **title** in place of the raw note id, falling back to the id
when a note has no title.

## The phase map

The epic decomposes into phases A–G; each is its own sub-issue. A is the
gate; the rest fan out behind it.

| Phase | What it lands | Depends on |
| --- | --- | --- |
| **A — Metadata foundation** | Front-matter parse/serialize + CUE schema + canonical content hash in `notes-protocol`; editor shows the title; this doc. | #981 |
| **B — Worker LLM module + auto-naming** | `notes-sync/src/llm.rs` (OpenRouter over `worker::Fetch`); auto-name untitled notes on the lazy-commit cadence, writing the title into front-matter through the DO. | A |
| **C — Vault index + auto-filing** | Vault DO registry (rebuilt from the notes); LLM picks a folder from the existing vault for new notes. | A |
| **D — Embeddings + connections** | Embed notes (hash + `model_id` keyed) in the Vault DO; similarity edges via brute-force cosine; explicit `[[wikilink]]` edges parsed from the body. | B + C |
| **E — Graph view + connection UI** | `GET /vault/graph` (nodes+edges JSON, session-gated); 2D force-directed graph; backlinks/suggested-links panel. | D |
| **F — Markdown rendering mode** | Rust-WASM render with edit/reading toggle; front-matter hidden; clickable `[[wikilinks]]`. | A (independent of the AI track) |
| **G — Drift control + search** | `shaka notes verify`/repair (lazy self-heal + event-driven re-index on a `notes-store` push); full-text + semantic search. | C / D |

Dependency order: **A is the gate.** Then B, C, and F can proceed in
parallel (B/C in `notes-sync`, F in `notes-editor`). D depends on B + C; E
depends on D; G rides on C/D.

## Relationship to #963 / #981

This epic (#989) is the **AI / `Zettelkasten` layer**. It runs alongside the
**Obsidian-vault navigation** track:

- **#963** is the navigation vision: hierarchy → `[[wikilinks]]` → render →
  search.
- **#981** is that track's phase 1: paths-as-ids, the sidebar, and the
  vault index.

This epic **reuses #981's vault index** rather than duplicating it, shares
the markdown-render goal (#963 phase 3 = phase F here) and the search goal
(#963 phase 4 = part of phase G), and otherwise layers the AI features on
the same engine. Note-ids-as-paths itself is #981's work, not this epic's —
phase A builds on it.

## Out of scope

Multi-user collaboration / sharing, CRDT merge (the whole-body delta seam
stays clean for a future upgrade), in-browser LLM (we use the worker +
OpenRouter), and Cloudflare-proprietary AI / vector services.
