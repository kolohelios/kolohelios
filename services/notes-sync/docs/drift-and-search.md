# Drift control and vault search

The index is disposable. Note text remains authoritative in the per-note
DO's append-only edit log. Git changes enter through that same single
writer, as opaque whole-text deltas, without resetting sequence numbers.

## Operator commands

From a project devshell, after deploying the updated worker:

```sh
shaka notes webhook-install
shaka notes verify
shaka notes verify --repair --wait
shaka notes search "rust storage"
shaka notes search "related ideas" --semantic
```

The CLI reads `wrangler.toml` for the backend origin, owner DID, and Git
repository. It resolves `SESSION_SECRET` through the project's existing
`.env.example` reference using `op read`, then signs short-lived owner
sessions. No browser cookie extraction is needed. Secrets, cookies, and
hook signing keys stay in memory and child-process `stdin`, never arguments,
environment variables, temporary files, or logs. `--project` selects another
project directory; `--url` overrides the HTTPS backend origin.

`webhook-install` creates or updates the matching push hook on
`notes-store`, using the current `gh` account's repository administration
permission. Its signing key is HMAC-SHA256 of `notes-store-webhook-v1` under
`SESSION_SECRET`: GitHub receives only that domain-separated key, not the
cookie-signing secret. Re-run installation after rotating `SESSION_SECRET`.
No additional secret or private-repository workflow is required.

Verification is read-only unless `--repair` is supplied. It scans Git,
indexed, and pending ids, in pages, comparing canonical body/model keys,
metadata, actual full-text chunks, vectors, and pending/conflict state.
Git blob identities detect cold/hot disagreement without fetching every
committed body. Missing branches, incomplete tree responses, unavailable
sources, unsupported ids, and missing embeddings never report clean.
A page observes current snapshots; it does not lock the vault against
concurrent editing.

Repair queues only affected records. `--wait` waits for those jobs, then
performs a fresh read-only scan; a queued response is not proof of repair.
Remaining conflicts, missing credentials, provider failures, and timeouts
exit nonzero. Without `--wait`, a still-stale report also exits nonzero.
This bounded interactive wait is not a periodic indexing schedule.

## Source reconciliation and event delivery

- **On open:** the first socket `Open` can hydrate a clean note from Git
  before its initial `Sync`, then queues an index refresh. No network call
  is added to the accepted-edit/Ack path. A cold-note load failure closes
  the socket rather than presenting an invented empty body.
- **On push:** `POST /vault/events/github` authenticates the exact raw body
  using `X-Hub-Signature-256`, then checks repository and branch. Normal
  pushes queue only changed Markdown paths. Large, forced, or incomplete
  push payloads request durable whole-vault discovery under an alarm;
  acknowledgement does not wait for a full scan. Bodies above 2 MiB fail
  closed; use explicit verification/repair for an oversized delivery.
- **Hot edits win:** background Git imports require an idle note with
  `seq == committed_seq` and no active rename lease. A compare-and-append check rejects a snapshot
  that changed during I/O. Busy/uncommitted notes retain their text and
  report `git_conflict`; the operator can reconcile after editors close.
  Git history retains the competing cold version. Existing lazy backup
  semantics still write the authoritative hot body to Git.
- **Deletion:** an idle external Git deletion appends an empty-text delta
  and a tombstone, preserving the log/sequence. Search records and incoming
  connection caches are invalidated. A later accepted edit clears the
  tombstone. An inaccessible branch is not interpreted as mass deletion.

There is no clock-based scan. Durable jobs retry transient failures; open,
push, explicit repair, and search/model freshness checks are the triggers.

## HTTP interface

Except for the signed GitHub endpoint, these routes require the owner
session. Browser CORS permits the configured app origin and JSON requests;
responses use `Cache-Control: no-store`. POST requests with an unrelated Origin fail.

| Route | Request / result |
| --- | --- |
| `POST /vault/verify` | JSON `{"repair": false, "after": null}`; per-id reasons, `clean`, and `next_after` cursor. Up to 20 records per page. |
| `GET /vault/status` | Pending work/discovery count and last discovery summary. |
| `POST /vault/rebuild` | Request durable discovery and reconciliation of supported Git/index ids. Counts become available through status. |
| `POST /vault/search` | JSON `{"query": "words", "mode": "text", "limit": 20}`; ranked ids/titles, content hashes, excerpts, scores, and conflict flags. |

Text search uses case-insensitive AND matching across canonical prose,
title, path, tags, and aliases, with title/metadata weighting. Full text is
stored in bounded KV chunks, not an excerpt. Semantic mode (`"semantic"`)
embeds the query and uses exact cosine over current-model vectors. Missing,
corrupt, or pending records are excluded and queued for repair; a query
racing index changes returns 409 and should be retried. Semantic mode
returns 503 when no embedding credential is configured. Queries are capped
at 2,000 characters, limits at 100 results, and JSON requests at 16 KiB.

The offline `workerd` smoke test in `tests/runtime-smoke.mjs` exercises real
DO storage/alarms, open hydration, safe Git conflicts/import/deletion,
metadata-only vector reuse, authentication/CORS, and both search modes.
It substitutes local test servers and fake keys, and runs after `worker-build`
inside generated `just validate`; it never contacts GitHub or OpenRouter.
