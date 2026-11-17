# NovelAI text candidates

`blogctl novelai` adds optional text generation to the private experiments in
[WRITING.md](WRITING.md). It does not change OpenRouter commands, post stages,
editable drafts, or publishing behavior. No model is selected implicitly.

## Commands

First import a private experiment with `blogctl experiment`. Its seed,
instructions, target, and named approaches remain private. Existing `workshop`,
`woodland`, and `ordinary` variants work without special provider configuration.
Fiction prompts prohibit invented autobiography; nonfiction prompts prohibit
invented facts and firsthand experiences.

Prepare the prompt and preview the exact JSON request without credentials or
network access:

```sh
blogctl experiment --workdir /private/writing-lab prepare EXPERIMENT_ID --variant workshop
blogctl novelai generate EXPERIMENT_ID --workdir /private/writing-lab \
  --variant workshop --model MODEL --dry-run
```

Model discovery makes one authenticated GET, not a generation:

```sh
blogctl novelai models
```

Choose an ID actually returned by discovery. Access still depends on subscription
and model entitlement; discovery does not guarantee that generation is allowed.
The tool never falls back to another model or sends a discovery request during
generation.

To request one candidate, invoke the explicit generation command:

```sh
blogctl novelai generate EXPERIMENT_ID --workdir /private/writing-lab \
  --variant workshop --model MODEL
```

Supply credentials as described below. The `generate` command itself is the
explicit action: there is no `--run`, typed `GENERATE`, or terminal requirement.
It also works from a coding agent or a redirected shell when you request a
candidate. Use `--dry-run` when you want only a credential-free local preview;
the older preview-by-default behavior has intentionally changed.
`experiment prepare` remains local-only.

## Instruction transport and output limits

Standalone generation defaults to `--transport chat`. It sends one request to
`/oa/v1/chat/completions`, with separate system and user messages and
`enable_thinking=false`. The system message supplies editorial instructions;
the user message supplies the target, original seed, explicitly selected source
material, and task. There is no trailing `Candidate:` continuation cue.
The exact wire request is visible in the credential-free preview.

Use `--transport completion` only for deliberate legacy raw-text continuation.
That mode retains the existing prompt and `/oa/v1/completions` endpoint. A model
that rejects the selected transport fails without switching endpoints or models.
The engine does not infer transport from model names.

Workflow passes opt in with `"transport": "chat"`. Omitted transport preserves
legacy continuation semantics, with a warning, so an existing session is not
silently reinterpreted. Changing consumer transport, instructions, or output
limits changes the workflow fingerprint and requires a new session. Keep old
sessions and failed candidates unchanged.

Each invocation sends one POST with streaming turned off, requesting one candidate:
`n=1`, `best_of=1`, `stream=false`, and `echo=false`. Output defaults to
`max_tokens=600` and `temperature=0.8`. A deliberate invocation may override
those two bounded fields:

```sh
blogctl novelai generate EXPERIMENT_ID --workdir /private/writing-lab \
  --variant workshop --model MODEL --max-tokens 1200 --temperature 0.25 \
  --artifact-kind draft --parent PRIOR_EXPERIMENT/PRIOR_CANDIDATE
```

Artifact kind and parent are optional structured provenance. A supplied parent
must already exist and pass candidate integrity checks; invalid lineage fails
before credential lookup or a request. No parent is inferred from timestamps or
the latest candidate.

`max_tokens` must be 1–8192 and temperature must be a finite value from 0–2.
A token limit is not a word target: 900 output tokens cannot reliably accommodate
a revised 1,000-word chapter. Consumers must choose adequate bounds for the
requested deliverable, for example 4096 tokens for a full chapter refinement,
then inspect the exact preview. The engine neither raises a cap silently nor
continues an incomplete response through another request.
Invalid values fail before credential lookup, network access, or candidate writes.
The exact effective values appear in both `--dry-run` output and immutable
candidate provenance. Candidate count, retries, streaming, fallback, and request
chaining remain non-configurable. The request has a 60-second timeout. No
redirects, automatic retries, schedules, autonomous loops, or chained generations
are implemented. Connection pooling is disabled to avoid hidden stale-connection
retries by the HTTP client.

The MCP surface remains read-only; this change does not expose generation or
manuscript mutation through MCP. There are no automatic batches or follow-up
requests. Candidate review is separate from the decision to generate.

NovelAI's [subscription documentation](https://docs.novelai.net/en/subscription/)
currently includes unlimited text generation with eligible subscriptions; `Anlas`
are used for images. The output cap is a length limit, not a dollar budget.
The CLI does not report billing receipts; account/model entitlements still apply.

## `Xialong` token-only responses and offline decoding

NovelAI's live chat endpoint can return a `chat.completion.chunk` object with
empty `choices[0].text` and a `token_ids` array instead of `message.content`,
even when `stream=false`. Its [public web client](https://novelai.net/stories)
maps `xialong-v1` to the GLM vocabulary and decodes these IDs locally. The OpenAPI
response schema doesn't specify that token-only contract.

For this exact model and response shape, `blogctl` uses the official
[GLM vocabulary](https://novelai.net/tokenizer/compressed/glm45_tokenizer.def?v=2&static=true).
The compressed resource is pinned by SHA-256
`d862f215272db05b6708671eb027f31ab83fdda9f3d41f8c6102c5f5002e2768`.
`cli.buildResources` in `project.cue` gives package builds and development shells
the same fixed-output Nix resource. Only public vocabulary data enters the Nix store;
writing material never does. The compiled binary retains the resource path.
No decoder or vocabulary is fetched during preview, generation, or inspection.
The provider resource is fetched verbatim; no private response or full provider
application bundle is vendored in this repository.

Before a `Xialong` chat request, the engine loads and verifies the pinned resource.
A missing or altered resource fails before the request. Decode-only support
reverses the vocabulary's byte-to-Unicode mapping, checks complete UTF-8, and
bounds compressed bytes, inflated data, token count, and decoded output.
Unknown IDs, invalid numeric IDs, a mismatched model or response object, and
conflicting text/token output fail closed with the raw capture retained. There
is no vocabulary guessing, remote decoding, or endpoint/model fallback.

A final token `151336` (`<|user|>`) is a protocol terminator for this contract.
Only that final token, and only with finish reason `stop`, is removed. Every
other byte remains unchanged, including leading/trailing whitespace, embedded
markers, and reasoning tags. Provenance records the resource hash, original
token count, SHA-256 of the raw decoded text, and exact removed token IDs. CLI
output reports the protocol handling. Embedded markers remain visible with an
advisory warning; decoding doesn't establish editorial or factual correctness.

Already captured responses can be inspected without another generation:

```sh
blogctl novelai decode-response RESPONSE_ID --workdir /private/writing-lab
```

This local-only command verifies a complete, private, hash-consistent response
record and prints JSON containing `body`, finish reason, usage, decoding
provenance, and the response-record ID. It resolves no credentials, writes no
files, imports no candidate, and changes no session, review, or selection. Keep
its output private. Incomplete captures and unsafe paths are refused; the
read-only MCP surface doesn't expose this command.

## 1Password at the invocation boundary

The engine reads only `NOVELAI_API_TOKEN`. It never calls `op`, consults a
keyring, signs into NovelAI, or stores a credential. Use your own **persistent
API token**, kept in 1Password; do not supply an account password or paste the
token into an agent conversation, shell argument, Nix expression, or manuscript.

Authenticate the 1Password CLI locally, then supply an `op://` field reference
through [`op run`](https://developer.1password.com/docs/cli/reference/commands/run/).
These are example references, not assumptions about your vault:

```sh
NOVELAI_API_TOKEN='op://YOUR_VAULT/NovelAI/credential' \
  op run -- blogctl novelai models

NOVELAI_API_TOKEN='op://YOUR_VAULT/NovelAI/credential' \
  op run -- blogctl novelai generate EXPERIMENT_ID --workdir /private/writing-lab \
    --variant workshop --model MODEL
```

Alternatively use `op run --env-file /private/novelai.env -- ...` with a private
file containing only `NOVELAI_API_TOKEN=op://...`, not a literal token. Keep real
references outside the public repo. Never print a resolved token with bare
`op read`, enable shell tracing around secrets, or persist resolved credentials
in shell startup files, Dropbox, logs, or candidates. Update/revoke the token in
1Password/NovelAI to rotate it; no runtime copy needs updating.

This slice does not install/configure `op` or change Home Manager. Toolchain and
prototype migration are described in [INTEGRATIONS.md](INTEGRATIONS.md). The process environment is still
accessible to sufficiently privileged processes; this is not credential isolation
from other processes running as the same user.

The production API base is fixed to `https://text.novelai.net/oa/v1`. There is no
API-base environment override that could silently redirect a real token to a
server you did not choose. Mock tests use private constructors and local servers.

## Artifacts and failures

Success prints one candidate ID. The text is published through the existing
immutable candidate store, with provider/model, exact prompt and seed hashes,
UTC record timestamp, parameters, attribution, fiction flag, and initial
`review_status: unreviewed`. New provenance parameters include the endpoint,
request-format version, exact structured request, provider `finish_reason`, and
available prompt/completion/total token usage. The historical prompt hash still
identifies the deterministic experiment prompt; the structured request records
what was actually sent to the selected endpoint. The prompt/experiment snapshot is captured before
network access, so subsequent definition edits cannot alter it.
A `length` finish reason produces an explicit warning after preserving the exact
response body as a candidate. Missing or other non-`stop` reasons also warn;
an HTTP 200 alone is not proof of a complete deliverable. Any separate
`reasoning_content` is stored in private provenance, never spliced into prose.
Unexpected reasoning and leaked think/Approach/Candidate scaffolding warn without
silent stripping, retries, selection, or advancement. Neither transport guarantees
editorial quality; live behavior still needs an explicitly authorized canary.
Malformed chat shapes or empty output fail without fallback; the bounded
response evidence remains available privately even when no candidate is saved.

No headers or credentials enter provenance. No post body, canonical manuscript,
stage, or VCS state is modified.

```sh
blogctl candidate --workdir /private/writing-lab list EXPERIMENT_ID
blogctl candidate --workdir /private/writing-lab read EXPERIMENT_ID CANDIDATE_ID
```

### Private response records

Every generation call prepares private response storage before sending a request.
It publishes a response record before JSON or schema validation under
`.writing/provider-responses/response-*.json` in the explicitly selected private
working directory. Final records use mode `0400`, their temporary files use `0600`, and
managed directories use `0700`. Publication never overwrites an existing record.
Known unsafe paths or inability to prepare storage abort before the request.
No response records are created by previews, imports, or read-only MCP tools.
Model discovery retains its existing sanitized behavior without response capture.

Each record contains the exact structured request, endpoint, model, HTTP status
when available, a SHA-256 digest, retained byte count, and response bytes encoded
as base64. It never copies request or response headers. Response capture retains
at most 4 MiB; an additional byte detects overflow. A truncated body is explicitly
marked as a prefix, not a complete response. Read failures retain any partial
bytes, and transport failures record that no response was available. If the raw
body contains the exact API token, it is withheld entirely and the record marks
credential echo rather than persisting that credential.

Validation errors identify safe stages: invalid JSON with line/column, missing
or wrong-shaped choices/message/content, wrong assistant role, empty output,
reasoning-only output, invalid finish reason, or invalid usage counters. Errors
include the private record ID, not provider text or raw parser/transport errors.
Successful candidates also reference their response record in provenance, so a
later candidate-save failure does not erase the provider evidence.

Inspect metadata locally before decoding any private response bytes:

```sh
record="/private/writing-lab/.writing/provider-responses/RESPONSE_ID.json"
jq '{status, retained_bytes, truncated, read_failed, credential_echo_detected}' "$record"
```

Decode `body_base64` only into a private terminal or mode-0600 file outside Git,
the Nix store, and synced folders. Records can contain author source material,
model output, and reasoning. Do not paste or commit them as public fixtures;
reproduce the response shape with synthetic data instead. A partial or
credential-withheld record is not a complete replay artifact.

Post-request disk errors can still prevent capture. Such errors explicitly say
the request was already attempted and identify the planned record ID; inspect
the private directory before deciding what happened. No capture or validation
failure triggers a retry, fallback, selection, or pass advancement. The earlier
failed canary's discarded response cannot be recovered by this change.

Known directory, symlink, and permission problems are checked before sending a
request. Disk errors or concurrent changes can still prevent saving after a
successful response; the error explicitly says a request was already made.
The response is not printed or automatically submitted again. Inspect local
candidates and account usage before deciding on any further action.

- Missing/invalid token: fails locally without sending a request.
- HTTP 401: check or rotate the persistent token.
- HTTP 403: check subscription and model entitlement.
- HTTP 429: wait before a new explicit invocation; no retry occurs.
- Redirect, transport, timeout, malformed/oversized response, or server error:
  generation outcome may be unknown even if no candidate was saved. Do not
  assume nothing happened and repeat blindly. Generation errors identify a
  private response record when capture succeeded. Provider bodies remain out of
  CLI errors; model-discovery bodies and raw transport errors remain suppressed.

Use only your own token within subscription limits and the
[NovelAI terms](https://novelai.net/terms). Review and manuscript incorporation
remain manual; no generated fiction becomes autobiography by default.

## API references and validation

The official [text API documentation](https://text.novelai.net/docs/index.html)
and its linked [OpenAPI document](https://text.novelai.net/docs/doc.json) specify
`GET /oa/v1/models`, `POST /oa/v1/completions`, and
`POST /oa/v1/chat/completions`. The chat request schema includes `messages` with
`system`/`user`/`assistant` roles and `enable_thinking`. These use plain UTF-8
messages or prompts, not the legacy tokenizer-dependent `/ai/generate` endpoint.
Images are outside this text workflow.

From `apps/blogctl`:

```sh
nix develop . --command env -u OPENROUTER_API_KEY -u NOVELAI_API_TOKEN just validate
```

Tests cover model discovery, exact request shape, response parsing, UTF-8 and
whitespace, credentials, rate limits/errors, redirect refusal, explicit single
requests without terminal input, dry-run previews, immutable provenance,
pre-request storage checks, and post-response storage failure. New API tests use
only local mocks and synthetic files. Removing credentials also prevents the
existing opt-in OpenRouter live test from running during this validation.
