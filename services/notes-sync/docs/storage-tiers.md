# Storage tiers: why both a Durable Object and git

notes-sync keeps every note in **two** stores — a per-note Durable Object
(DO) and a git repo (`notes-store`). That looks redundant until you see
that they are a **hot tier** and a **cold tier** doing different jobs, and
that neither can do the other's job. This note explains the division of
labour, the one subtle seam it creates, and the consequence that seam has
for the vault listing.

## The two tiers

| | Hot tier — Durable Object | Cold tier — git/GitHub |
| --- | --- | --- |
| Holds | append-only edit log + materialized `(seq, text)` snapshot | `notes/<id>.md` per note, full history |
| Write cadence | every accepted edit, durably, before the `Ack` | lazy (`debounce` + `backstop` alarm, and on last disconnect) |
| Latency | milliseconds | hundreds of ms to seconds |
| Scope | one note (`idFromName(id)`) | the whole vault |
| Role | live editing, durability *now* | backup, history, listing, durability *forever* |

## Why git can't be the live store (so you need the DO)

- **Rate limits and latency.** Every keystroke streams to the server. A
  git commit goes through GitHub's contents API: hundreds of ms to
  seconds each, capped around 5,000 requests/hour. Commit-per-keystroke
  would exhaust the limit in seconds and feel terrible. The DO persists an
  edit to its own storage and `Ack`s in milliseconds — which is exactly
  why the git cadence is allowed to be lazy: **the fast path never touches
  git.**
- **Single-writer coordination.** The per-note DO is *the* one authority
  for that note. It assigns the monotonic `seq`, runs the stale-edit check
  (`is_stale`), and serializes concurrent edits and reconnects. Git has no
  live single writer and no sequence — only optimistic ref-checking at
  commit time.
- **It hosts the socket and the edit log.** The DO owns the live
  (`hibernatable`) WebSocket and the append-only delta log that makes
  lossless replay and reconnect-from-`since_seq` possible. Git stores
  whole-file snapshots, not the fine-grained log.

## Why the DO can't be the only store (so you need git)

- **It is per-note and proprietary.** Each DO knows only *its own* note;
  there is no cross-note view, and DO storage is not a history you can
  clone, diff, or walk. Git gives the portable, human-readable,
  navigable backup — plus versioning and a future embedding/vector
  source.
- **It is the listing.** `GET /vault` learns *which notes exist* by
  walking the git tree of `notes-store`. A note is in the drawer if and
  only if `notes/<id>.md` is in git.

This last point is also why **deleting a note makes its own git commit**:
removing the file from git is what actually makes the note disappear from
the vault. If delete only wiped DO storage, the file would linger and the
note would reappear on the next tree walk. Every contents-API mutation —
including a delete — is a commit, because git history is append-only.

## The seam: authority is split

Read the two roles together and the subtle part falls out — **authority is
split between the tiers**:

- **Content** truth lives in the **DO**. On wake, `load_state` reads
  `(seq, text)` straight from DO storage; **git is never read back**.
- **Existence / the listing** truth lives in **git** (the `/vault` tree
  walk).

So the DO is the source of truth for *what a note says*, and git is the
source of truth for *which notes exist*.

## Consequence: the vault freshness gap

That split has one user-visible cost. A freshly created note is durable in
its DO immediately, but it does not appear in the drawer until its first
git commit lands — because the listing comes from the cold tier, and the
commit is lazy (`debounce` + `backstop`). The drawer also does not refresh
itself when that commit lands. So a new note can be invisible in the
listing for up to the backstop window even though it is fully safe.

This is not a bug so much as the bill for keeping the listing in the cold
tier. Two ways to address it, cheapest first:

1. **Mitigate in the shell.** Inject the note you are currently viewing
   into the drawer even if it is uncommitted, and refresh the listing when
   the editor receives a `BackedUp` signal. Closes most of the gap without
   new infrastructure.
2. **Promote the index to the hot tier.** A small `Vault` DO that tracks
   the set of note paths as notes are created/renamed/deleted. Then
   creation is instant in the drawer and git becomes pure backup. More
   moving parts; the principled fix if the gap keeps biting.

The git-tree walk was chosen first per the repo's "test the simpler
hypothesis" tenet, accepting the freshness gap as a known limitation.

## Summary

The DO is **live, fast, single-writer, durable now**; git is **portable,
historical, the listing, durable forever**. The two-tier split is the
design. The freshness gap is the price of letting the cold tier own the
listing.
