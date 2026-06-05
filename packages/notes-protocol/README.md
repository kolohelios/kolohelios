# notes-protocol

The wire protocol for the live-synced note app, shared by the
`notes-sync` Durable Object and the WASM editor as **one set of `serde`
structs** — so both sides serialize the exact same bytes. First occupant
of the `packages/` slot. Compiles natively (for `cargo test`) and to
`wasm32-unknown-unknown` (the Worker and the editor).

## Messages

Client → server:

- `Open { since_seq }` — open the note and sync from `since_seq` (`0`
  for a fresh client). The server replies with a `Sync`.
- `Edit { base_seq, delta }` — apply `delta` on top of `base_seq`. A
  stale `base_seq` is rejected with a fresh `Sync` instead of an `Ack`.

Server → client:

- `Ack { seq }` — the edit was accepted at sequence `seq`.
- `Sync { seq, text }` — full state; adopt `text` at `seq`.
- `BackedUp { commit_sha? }` — the note was committed to git.

Each message is a `serde` tagged `enum` (`{"type":"open",…}`).

## Deltas

A `Delta` is either a `Splice { at, remove, insert }` (the small-delta
common case, byte offsets into the current UTF-8 text) or a
`Whole { text }` (the always-correct fallback and fresh-document
bootstrap). `Delta::apply` folds a delta into a text and is the seam
where a CRDT update type could slot in later without disturbing the
surrounding protocol.

## Status

Phased build tracked in issue #757; the crate lands in #763.
