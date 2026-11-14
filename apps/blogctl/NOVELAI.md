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
  --variant workshop --model MODEL
```

Model discovery makes one authenticated GET, not a generation:

```sh
blogctl novelai models
```

Choose an ID actually returned by discovery. Access still depends on subscription
and model entitlement; discovery does not guarantee that generation is allowed.
The tool never falls back to another model or sends a discovery request during
generation.

To request a candidate, invoke this yourself in a normal terminal:

```sh
blogctl novelai generate EXPERIMENT_ID --workdir /private/writing-lab \
  --variant workshop --model MODEL --run
```

Supply credentials as described below. `--run` is necessary but insufficient:
the command opens the controlling terminal, shows the model, output cap, and
SHA-256 fingerprint of the full request, then asks you to type `GENERATE`. Each
invocation requires fresh confirmation. There is no `--yes`, environment approval,
standard-input approval, saved authorization, or batch mode. Review the request preview
before authorizing; the fingerprint covers the compact JSON sent to the API,
including model, prompt, and parameters. The preview uses readable indentation.

One authorization permits one POST with streaming turned off, requesting one candidate:
`n=1`, `best_of=1`, `max_tokens=600`, `temperature=0.8`, `stream=false`, and
`echo=false`. These deliberately small initial settings are not configurable in
this slice. The request has a 60-second timeout. No redirects, automatic retries,
schedules, autonomous loops, or chained generations are implemented. Connection
pooling is disabled to avoid hidden stale-connection retries by the HTTP client.

Terminal confirmation prevents accidental unattended use; it is not a sandbox
against agents/processes with full user permissions or control of a terminal.
The authorization is single-use and bound to the exact request in the Rust API.
Generation is not exposed through MCP in this issue.

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
    --variant workshop --model MODEL --run
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
API-base environment override that could silently redirect a real token to an
server you did not choose. Mock tests use private constructors and local servers.

## Artifacts and failures

Success prints one candidate ID. The text is published through the existing
immutable candidate store, with provider/model, exact prompt and seed hashes,
UTC record timestamp, parameters, attribution, fiction flag, and initial
`review_status: unreviewed`. The prompt/experiment snapshot is captured before
confirmation and network access, so subsequent definition edits cannot alter it.
No headers or credentials enter provenance. No post body, canonical manuscript,
stage, or VCS state is modified.

```sh
blogctl candidate --workdir /private/writing-lab list EXPERIMENT_ID
blogctl candidate --workdir /private/writing-lab read EXPERIMENT_ID CANDIDATE_ID
```

Known directory, symlink, and permission problems are checked before sending a
request. Disk errors or concurrent changes can still prevent saving after a
successful response; the error explicitly says a request was already made.
The response is not printed or automatically submitted again. Inspect local
candidates and account usage before deciding on any further action.

- Missing/invalid token: fails locally without sending a request.
- HTTP 401: check or rotate the persistent token.
- HTTP 403: check subscription and model entitlement.
- HTTP 429: wait before a new manually authorized invocation; no retry occurs.
- Redirect, transport, timeout, malformed/oversized response, or server error:
  generation outcome may be unknown even if no candidate was saved. Do not
  assume it is safe/free to repeat. API bodies and raw transport errors are
  suppressed to avoid leaking credentials or private request material.

Use only your own token within subscription limits and the
[NovelAI terms](https://novelai.net/terms). Review and manuscript incorporation
remain manual; no generated fiction becomes autobiography by default.

## API references and validation

The official [text API documentation](https://text.novelai.net/docs/index.html)
and its linked [OpenAPI document](https://text.novelai.net/docs/doc.json) specify
`GET /oa/v1/models` and `POST /oa/v1/completions`. This uses plain UTF-8 prompts
and text responses, not the legacy tokenizer-dependent `/ai/generate` endpoint.
Images are outside this text workflow.

From `apps/blogctl`:

```sh
nix develop . --command env -u OPENROUTER_API_KEY -u NOVELAI_API_TOKEN just validate
```

Tests cover model discovery, exact request shape, response parsing, UTF-8 and
whitespace, credentials, rate limits/errors, redirect refusal, single-use request
binding, terminal-required CLI behavior, local previews, immutable provenance,
pre-request storage checks, and post-response storage failure. New API tests use
only local mocks and synthetic files. Removing credentials also prevents the
existing opt-in OpenRouter live test from running during this validation.
