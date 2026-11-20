---
ai_provenance: "Written by an OpenAI coding assistant for issue #1131; examples and test prose are synthetic."
---

# Writing experiments and candidates

Experiments and candidate import are local-only additions to `blogctl`, not a
replacement for the post pipeline. They make no provider calls and need no
credentials. Optional [NovelAI generation](NOVELAI.md) runs one request per
explicit `generate` invocation; add `--dry-run` for a local-only preview. See [INTEGRATIONS.md](INTEGRATIONS.md) for separate review
records, read-only MCP, and explicit prototype migration.

## Private working directory

Use an existing private blog working directory or a separate local directory:

```sh
install -dm700 "$HOME/.local/share/writing-lab"
```

All commands require an explicit `--workdir` **before** the subcommand:

```text
<private-workdir>/
  .writing/
    experiments/<experiment-id>.json
    candidates/<experiment-id>/<candidate-id>.md
```

Read-only commands do not create directories. Writing commands create managed
subdirectories with mode 0700. Definitions are 0600, candidates 0400; existing
working directory permissions are not changed. Choose a private location outside
Dropbox for candidates awaiting review. Keep real seeds/manuscripts out of the public repo and
Nix source trees: flakes copy source into the world-readable store. Store-rooted
working directories, managed symlink paths, and path-shaped identifiers are refused.

These protections prevent accidental overwrite/path escape, not a malicious
process running as the same user. No sandbox or filesystem immutability flag is
claimed. Owners can change permissions; use an appropriate private backup.

## Define an experiment

Edit a private JSON file, using
[`tests/fixtures/writing/valid/chapter.json`](tests/fixtures/writing/valid/chapter.json)
as a synthetic example. Fields:

- `id`: stable kebab-case ID, at most 120 bytes.
- `target`: `{ "kind": "chapter", "book": "...", "chapter": "..." }`, or
  `{ "kind": "post", "slug": "..." }` / `{ "kind": "article", "slug": "..." }`.
  These are logical references; no draft or book file is created or loaded.
- `seed`: the original idea, preserved byte-for-byte as UTF-8 text after JSON
  decoding, including leading/trailing whitespace.
- `instructions`: private voice/style and writing directions.
- `fiction`: explicit `true` or `false`. Fiction prompts prohibit invented autobiography;
  nonfiction prompts prohibit fabricated facts/firsthand experiences.
- `variants`: named approaches, such as `workshop`, `woodland`, and `ordinary`.
  Names are kebab-case; descriptions and all required text must be nonempty.

Unknown fields are rejected, to catch typos rather than silently omit material.

```sh
blogctl experiment --workdir "$HOME/.local/share/writing-lab" import /private/experiment.json
blogctl experiment --workdir "$HOME/.local/share/writing-lab" list
blogctl experiment --workdir "$HOME/.local/share/writing-lab" show EXPERIMENT_ID
blogctl experiment --workdir "$HOME/.local/share/writing-lab" prepare EXPERIMENT_ID --variant workshop
```

Import validates and atomically publishes the definition, refusing any existing
ID. Edit the imported JSON locally when iterating, or create a new experiment ID
for a new original seed. Already-recorded candidate snapshots never change.
Previews print private text to your terminal; do not publish session logs.
Prompt format version 1 is deterministic: no timestamps, model defaults, template
execution, implicit whitespace trimming, or network access.

## Prepare a config-driven writing pass

`workflow prepare` composes one deterministic experiment from a consumer-owned
workflow definition, explicit local context files, the source experiment in the
private store, and an optional explicitly selected prior candidate. It does not
generate, select, review, advance, promote, or mutate manuscript text.

A workflow JSON file uses reusable context sets so multiple passes can share a
profile or selected samples without duplicating paths:

```json
{
  "version": 1,
  "source_experiment": "private-source-experiment",
  "instructions": "Instructions shared by every pass.",
  "fiction": false,
  "context_sets": {
    "profile": [
      {"label": "Voice profile", "path": "docs/voice/profile.md"}
    ],
    "samples": [
      {
        "label": "Voice sample (style only)",
        "path": "published/example.md",
        "format": "markdown-body"
      }
    ]
  },
  "passes": [
    {
      "name": "shape",
      "mode": "generate",
      "requires_input": false,
      "approach": "Return only a compact outline.",
      "artifact_kind": "outline",
      "max_tokens": 400,
      "temperature": 0.5,
      "max_words": 200,
      "require_complete_ending": true
    },
    {
      "name": "draft",
      "requires_input": true,
      "requires_evidence": true,
      "approach": "Draft from the selected outline and evidence.",
      "context": ["profile", "samples"],
      "min_words": 800,
      "max_words": 1200,
      "forbid_workflow_metadata": true
    },
    {
      "name": "investigation",
      "mode": "import-only",
      "requires_input": true,
      "approach": "Import an inspectable evidence pack."
    }
  ]
}
```

Paths are relative to an explicit configuration root. `text` (the default)
includes the whole UTF-8 file after trimming its outside whitespace;
`markdown-body` removes a leading YAML frontmatter block and includes only the
nonempty body. Configuration and context paths must stay beneath the root and
may not traverse symlinks. The root and private working directory are refused beneath the Nix store.

```sh
blogctl workflow --workdir "$HOME/.local/share/writing-lab" prepare shape \
  --config-root /private/consumer-repo \
  --config books/example/workflow.json

blogctl workflow --workdir "$HOME/.local/share/writing-lab" prepare draft \
  --config-root /private/consumer-repo \
  --config books/example/workflow.json \
  --input PRIOR_EXPERIMENT/PRIOR_CANDIDATE
```

A pass declaring `requires_input` fails unless `--input` is supplied; no latest
candidate is inferred. Candidate bodies are obtained through typed integrity
inspection rather than Markdown parsing. Preparation prints the deterministic
experiment ID. Repeating the same preparation confirms the existing definition;
a differing definition at the same ID is never overwritten.

Pass mode defaults to `generate`; `import-only` refuses previews and model
requests. `requires_evidence` refuses preparation until the caller selects at
least one immutable evidence pack. `artifact_kind` defaults to the pass name and
records structured lineage separately from prompt prose. Optional `max_tokens` and `temperature`
values use the same hard provider bounds as direct generation. The optional word bounds,
`forbid_workflow_metadata`, and `require_complete_ending` fields drive advisory
checks only. They never alter, reject, or select candidate text.

## Import immutable evidence packs

Evidence collection remains a manual or research assistant action. `blogctl` stores
only the structured material supplied in an explicit local JSON file; it never
browses, searches the working directory, or infers permission to read other
files.

```json
{
  "version": 1,
  "importer": "Author name",
  "provenance": "Collected manually from public sources.",
  "entries": [
    {
      "reference": "https://example.com/source",
      "accessed_on": "2026-11-15",
      "note": "Relevant quotation or concise source note.",
      "claim": "The claim this source bears on.",
      "relationship": "supports",
      "classification": "primary"
    }
  ]
}
```

`relationship` is `supports`, `complicates`, or `contradicts`;
`classification` is `primary` or `secondary`. Import validates every required
field and ISO date, calculates a SHA-256 content hash, derives a content-addressed
ID, and publishes the pack read-only. Reimporting identical content returns the
same ID. Inspection verifies the filename, identity, hash algorithm, content
hash, timestamp, and entries.

```sh
blogctl evidence --workdir "$HOME/.local/share/writing-lab" import /private/evidence.json
blogctl evidence --workdir "$HOME/.local/share/writing-lab" list
blogctl evidence --workdir "$HOME/.local/share/writing-lab" inspect EVIDENCE_PACK_ID

blogctl workflow --workdir "$HOME/.local/share/writing-lab" prepare synthesis \
  --config-root /private/consumer-repo \
  --config books/example/workflow.json \
  --input PRIOR_EXPERIMENT/PRIOR_CANDIDATE \
  --evidence EVIDENCE_PACK_ID
```

Repeat `--evidence` to include more than one pack. Selection order is explicit
and contributes to the deterministic experiment identity. The exact verified
pack JSON, including its content hash and provenance, enters the prepared
experiment and therefore its later candidate request snapshot. No pack is
selected implicitly.

## Conduct a resumable session

The conductor composes preparation, one-request generation, external import,
selection, review, and navigation without turning the workflow into an
unattended pipeline:

```sh
op run -- blogctl workflow \
  --workdir "$HOME/.local/share/writing-lab" \
  conduct example-chapter \
  --config-root /private/consumer-repo \
  --config books/example/workflow.json \
  --model exact-model-id
```

Session files live under `.writing/sessions/` with mode `0600`. The numbered
loop can read the selected candidate, preview a request, make exactly one
request, import a file, select an existing candidate, record a hash-bound
review, advance, move to another pass, select evidence packs, record a polished
final-draft decision, explicitly export approved prose, or stop. Import and
generation leave new candidates unselected. Advancing requires an explicit selection, clears it,
and carries that exact candidate as the next pass input. Moving to another pass
requires an explicit input when its policy says so; the conductor never infers
the latest candidate.

For instruction-following editorial passes, declare `"transport": "chat"` in
consumer workflow policy. This uses NovelAI's role-separated chat endpoint with
thinking disabled, not raw prose continuation. Prior candidates, voice context,
and selected evidence enter explicit `source_material`, separate from system
instructions. Omitted transport preserves legacy behavior and warns; changing
transport or output bounds requires a new session. Choose an output token cap
large enough for the requested deliverable: a 900-token cap is not adequate for
a full 1,000-word chapter rewrite. See [NOVELAI.md](NOVELAI.md) for request and
response contracts, truncation warnings, and exact request provenance.

Preview doesn't resolve credentials or contact NovelAI. `import-only` passes
refuse preview and generation, leaving evidence acquisition to a human or
research assistant. Advisory findings report word bounds, leaked think
markup, likely instruction braces, workflow metadata, and incomplete endings.
They don't mutate immutable candidates. Reaching the final pass doesn't approve
`polished` text or promote a manuscript; those remain separate human actions.

## Resume author review without chat

Register a session once with its explicit consumer configuration and model:

```sh
blogctl workflow --workdir /private/writing-lab conduct chapter \
  --config-root /private/consumer-repo --config books/example/workflow.json \
  --model exact-model-id --width 66
```

The private session saves the exact workflow JSON, context-root reference, and
chosen model. Registration archives the previous session bytes privately before
adding this binding. It does not authorize a request. Subsequent launches need
no chat transcript, candidate ID, or repeated configuration arguments:

```sh
blogctl workflow --workdir /private/writing-lab sessions
blogctl workflow --workdir /private/writing-lab status chapter
blogctl workflow --workdir /private/writing-lab conduct chapter --width 66
```

Status and listing are read-only. Older unsupported session files are reported,
not rewritten. A legacy Rust session needs the explicit registration arguments
once. Its existing active-pass output journal supplies review focus without
searching candidate directories or inferring the newest file.

The conductor distinguishes **review focus** from **acceptance**:

| Action | Result |
| --- | --- |
| `1` | Read the focused draft, pending working text, or initial input. |
| `d` | Compare working text against its parent, or the focused output against its explicit parent/input. |
| `edit` | Prepare a private hash-bound copy and open the configured editor. Successful editing saves an unselected checkpoint. |
| `save` | Import an interrupted or externally edited tracked copy as a checkpoint. |
| `abandon` | Explicitly stop tracking a working copy; preserve both files. |
| `f` | Focus an explicit current output or a direct editorial child, without selecting it. |
| `guards` | Record intentionally superseded protections in a new private policy session. |
| `a` | Explicitly select the reviewed checkpoint for this pass, without advancement. |
| `n` | Explicitly select it and advance, without generation. |
| `u` | Explicitly use the review draft as input for this same pass, without acceptance, advancement, or a request. |
| `2` / `3` | Preview / authorize exactly one request from the displayed input. |
| `9` | Stop; the session remains resumable. |

Reading shows prose, not provenance frontmatter or request snapshots. The plain
reader wraps to 72 columns by default (20–120 configurable); it uses `less` on an
interactive terminal when available, otherwise prints wrapped text. Pager history
and shell escapes are disabled. Rendering never changes candidate bytes.

Set `VISUAL` or `EDITOR` to a trusted editor command. The working path is passed
as a positional argument, not interpolated into that command. Editor and reader
children do not inherit NovelAI/OpenRouter credentials. If no editor is configured,
the tool prints the private path; edit it and choose `save`. The contributor label
is requested once and persisted as caller-asserted provenance, not verified identity.
An unchanged editor visit creates no candidate and needs no identity prompt.

Managed working copies live under `<workdir>/review-working/`, with mode `0600`
and a private sidecar. Pending intent is saved before file creation, so an editor
abort or interrupted preparation is visible after restart. Unsafe, stale, empty,
or incomplete copies fail closed; they are not discarded automatically. While a
copy is pending, accepting, generating, changing evidence, or navigating to another
draft cannot silently lose it. Finish or explicitly abandon it first.

New generated/imported outputs and saved author checkpoints become review targets
and update the ledger/handoff, never acceptance. Any prior explicit selection stays
bound to its original bytes until the author selects something else or changes
pass/input; reviewing or editing another candidate does not revoke that selection.
An existing standalone checkpoint
can be attached explicitly, including after a fresh terminal launch:

```sh
blogctl workflow --workdir /private/writing-lab focus chapter EXPERIMENT/CANDIDATE
```

Only current-experiment outputs or direct editorial children of the current target
are eligible. Standalone low-level revision commands still do not guess which
session owns their result. For the ordinary session-owned edit loop, use `edit`
and `save` rather than manually managing sidecars or candidate IDs.

### Intentional changes to protected wording

A model failing a guard is not permission to relax it. After an explicit author
checkpoint changes protected text, `guards` shows each missing literal. Enter
its exact replacement in the revised draft, or explicitly remove that protection.
Blank cancels the whole operation. Replacements must exist in the reviewed bytes
and must not conflict with forbidden output. Confirm the complete decision with
`FORK POLICY`.

The tool creates a new named session with a newly fingerprinted exact policy;
it does not edit consumer files or require a repository PR. Matching protections
are updated across the new policy. The original session stays unchanged, its
exact bytes are archived immutably, and previous assessments retain their policy
hash. The new session contains the review target but no selected output. The
conductor continues there and prints its resume command; `sessions` lists both
branches. Existing destinations are never overwritten.

A policy fork changes the contract, not the original generation input. To request
another version from the revised text, explicitly choose `u` first; otherwise
advance an accepted checkpoint normally. Preparation fails before a request if
the displayed input does not satisfy its protected-input contract.

Findings remain advisory, not editorial approval. Selecting despite findings
requires `ACCEPT WITH FINDINGS`. Accept-and-next checks the next pass's protections
before changing selection. Formal review sidecars and controlling-terminal polished
approval remain separate; neither `a` nor `n` approves publication or exports text.
All read/edit/status/compare/protection operations are credential-free and local.
For a paid request, prefer a separate credential-wrapped invocation:

```sh
op run -- blogctl workflow --workdir /private/writing-lab run-pass chapter
```

This uses the saved model, exact policy, and explicit input. It requests one output,
records it for review, and never retries, accepts, advances, or exports. Session-owned
policy and review artifacts stay outside Git, the Nix store, and Dropbox. A missing
old consumer directory does not prevent reading saved drafts; preparation needs an
available explicitly configured context root. Relocate a bound session with
`conduct SESSION --config-root /new/consumer-root`; the private snapshot stays
unchanged, including for policy forks. Full re-registration with both configuration
arguments requires a matching exact consumer policy. Neither path silently adopts
a changed consumer configuration.

## Authoritative editorial inputs and outcomes

Later editing passes can set `input_policy` to `selected-draft`. This requires
an explicit input candidate and uses its exact body as the seed. Historical
source-experiment prose, source instructions, and inherited source material are
not sent. The original source experiment remains unchanged. Only context sets
and evidence explicitly chosen for this pass are added; for a minimal voice
edit, consumers should select a concise profile rather than published samples.
The default `source` policy preserves existing workflows.

Consumers own protected choices and output restrictions:

```json
{
  "name": "voice",
  "mode": "generate",
  "requires_input": true,
  "input_policy": "selected-draft",
  "transport": "chat",
  "approach": "Return the complete selected draft with minimal voice edits.",
  "context": ["profile"],
  "protected_text": ["The potter waited."],
  "forbidden_strings": ["unapproved", "—"],
  "max_tokens": 4096,
  "min_words": 10,
  "max_words": 40,
  "require_complete_ending": true
}
```

Protected text must occur exactly in the selected input. Missing protected input,
empty constraints, or contradictions between protected and forbidden strings
fail before a request. Forbidden strings use case-insensitive `substring` matching,
not a universal profanity detector. These fields affect request preparation and
workflow fingerprints: policy changes require a new session, not reinterpretation
of old decisions. Consumers must also remove contradictory task instructions.

Inspect any explicit candidate before selecting it:

```sh
blogctl workflow --workdir /private/writing-lab assess voice \
  EXPERIMENT/CANDIDATE --config-root /private/consumer-repo \
  --config books/example/workflow.json --json
```

Assessment is local, credential-free, and read-only. It reports `needs-review`
when mechanical checks are clear or `constraint-violation` when findings exist.
Neither status means editorial acceptance, factual verification, or polished
approval. Checks include protected text, forbidden strings, configured word and
ending bounds, leaked reasoning/scaffolding, and provider completion status.
Advisory findings never rewrite a candidate or decide its selection.

One deliberate current-pass generation can be requested without scripting menu
input:

```sh
op run -- blogctl workflow --workdir /private/writing-lab run-pass SESSION \
  --config-root /private/consumer-repo --config books/example/workflow.json \
  --model exact-model-id
```

The invocation authorizes preparation, one provider request, immutable storage,
assessment, and outcome reporting. It does not authorize another request,
selection, or advancement. The conductor's generation/import actions also assess
new output immediately, while it remains unselected; selection lists display
findings before asking for an explicit candidate ID. A mechanical failure is not
presented as a successful editorial pass or an invitation to retry automatically.

Every session update refreshes a private mode-0600 `SESSION.handoff.md` next to
its JSON. The JSON is authoritative; the Markdown is a derived operational
handoff. It identifies current input, evidence, explicit selection, prepared
experiment, and recorded outputs or sanitized generation failures with findings.
No candidate body or credential is copied into it. An outside credential wrapper
that never launches the engine cannot create an engine attempt record.

Known unsafe handoff paths fail before preparation/request. A post-request disk
failure can still prevent refresh; a saved candidate or response remains evidence,
not permission to repeat a request. Handoff failure after saving session JSON is
reported explicitly and can be repaired by reopening the session without a new
provider request. Read-only assessment and MCP never open or update a session.

Operationally, carry a bounded request through preparation, checking, preservation,
and reporting without asking for separate authorization at each local step.
Editorial selection, navigation/advancement, additional provider requests, and
polished approval remain deliberate author actions. Keep implementation,
validation, publication, and downstream adoption in one tracked task rather
than treating each completed PR as a reason to lose the writing handoff.

## Record an externally obtained candidate

The local import command is useful for manually obtained text. It **does not**
generate, authorize a paid request, accept a candidate, or promote a manuscript.
Create a private metadata JSON file with exactly these fields:

```json
{
  "provider": "external-provider",
  "model": "exact-model-id",
  "ai_provenance": "AI-generated fiction; entered manually and awaiting review.",
  "parameters": {"temperature": 0.8, "max_tokens": 600, "n": 1}
}
```

`parameters` contains only generation settings, never credentials or headers.
Provider/model/provenance must be nonempty. Metadata is **caller-asserted**, not
proof that the external service used this prompt/model. Use the current prepared
prompt for the external request and record before changing the experiment. For a
future integrated provider, `Store::record_candidate` takes the exact immutable
`PreparedExperiment` captured before requesting a response, even if the on-disk
experiment changes while the request is running.

```sh
blogctl candidate --workdir "$HOME/.local/share/writing-lab" import EXPERIMENT_ID \
  --variant workshop --metadata-file /private/generation.json --body-file /private/candidate.txt \
  --artifact-kind draft --parent PRIOR_EXPERIMENT/PRIOR_CANDIDATE
blogctl candidate --workdir "$HOME/.local/share/writing-lab" list EXPERIMENT_ID
blogctl candidate --workdir "$HOME/.local/share/writing-lab" read EXPERIMENT_ID CANDIDATE_ID
blogctl candidate --workdir "$HOME/.local/share/writing-lab" inspect EXPERIMENT_ID CANDIDATE_ID
```

`--artifact-kind` and `--parent` are optional for backward compatibility. When
present, they record pass/artifact kind and explicit ancestry in provenance,
separate from free-form prompts. The parent must already exist and pass integrity
inspection. Multiple candidates may share one parent, so branches and repeated
passes do not imply a selected winner.

`read` prints the canonical Markdown artifact byte-for-byte. `inspect` parses it
inside `blogctl` and emits JSON with `provenance`, manuscript `body`, and the
structured `request_snapshot`. Inspection validates the filename, experiment,
variant, prompt hash, original-seed hash, artifact kind, and parent-reference
shape instead of requiring an external wrapper to parse storage Markdown. It
remains read-only and credential-free.

Import outputs a timestamp/random candidate ID. The UTF-8 body is stored without
trimming, followed by a labeled JSON request snapshot. YAML frontmatter records
provider, model, UTC record timestamp, candidate/experiment IDs, variant,
SHA-256 hashes of the exact prompt/seed, generation parameters, AI attribution,
fiction flag, and fixed initial `review_status: unreviewed`. The snapshot includes
the target, instructions, all variants, original seed, exact prompt, and prompt
format version. It is supporting material, not manuscript text.

Contents and permissions are finalized in a temporary file, then published
atomically without replacing any existing filename. Partial files are not listed
as candidates. An interrupted operation or filesystem error can leave a completed
artifact; inspect the list before importing again.

## Preserve explicit editorial revision checkpoints

Do not rely on one mutable Markdown file to preserve assistant, author, and
mechanical-edit handoffs. Prepare a private working copy from an exact candidate:

```sh
blogctl candidate --workdir "$HOME/.local/share/writing-lab" revise-prepare \
  EXPERIMENT_ID CANDIDATE_ID \
  --output /private/working/chapter.md
```

Preparation copies only the candidate body and creates a sibling
`chapter.md.blogctl-revision.json` sidecar. Both files use mode `0600`. The
sidecar binds the working path to the exact parent candidate file and body hashes.
Preparation requires no credentials or network, launches no editor, mutates no
session, and refuses overwrite, symlinks, parent traversal, Nix-store paths, and
managed `.writing` destinations.

After an explicit editing handoff, import a checkpoint:

```sh
blogctl candidate --workdir "$HOME/.local/share/writing-lab" revise-import \
  /private/working/chapter.md \
  --origin-kind human-revision \
  --revision-kind substantive \
  --contributor "Author name" \
  --notes-file /private/working/revision-notes.txt
```

Origin kinds are `generated`, `imported`, `human-revision`,
`assisted-revision`, and `mechanical-cleanup`. Revision kinds are `substantive`,
`mechanical`, `transcription`, and `other`. Contributors and notes are
caller-asserted provenance, not verified identity. The imported candidate keeps
the parent's request snapshot and artifact kind, records automatic parent
lineage and typed editorial provenance, and remains without a review or selection.
Repeating the same parent, exact body, and provenance returns the existing
candidate ID.

A mechanical cleanup is another checkpoint, not an overwrite of the substantive
revision. Compare any two exact candidate bodies without writing files:

```sh
blogctl candidate --workdir "$HOME/.local/share/writing-lab" diff \
  EXPERIMENT/OLDER_CANDIDATE EXPERIMENT/NEWER_CANDIDATE
blogctl candidate --workdir "$HOME/.local/share/writing-lab" diff \
  EXPERIMENT/OLDER_CANDIDATE EXPERIMENT/NEWER_CANDIDATE --json
```

The conductor exposes revision preparation/import as action `r` and comparison
as action `d`. It never imports when an editor exits, infers the latest
candidate, selects a checkpoint, or advances the pass automatically.

## Approve polished text and export it explicitly

A generated or imported candidate is never polished by default. First inspect the
complete candidate and ensure its `artifact_kind` is `final-draft`. Then record a
human decision from a controlling terminal:

```sh
blogctl candidate --workdir "$HOME/.local/share/writing-lab" polished \
  EXPERIMENT_ID CANDIDATE_ID \
  --decision approve \
  --approver "Author name" \
  --notes-file /private/polished-notes.txt

blogctl candidate --workdir "$HOME/.local/share/writing-lab" \
  polished-decisions EXPERIMENT_ID CANDIDATE_ID
```

Approval requires typing `APPROVE POLISHED` at `/dev/tty`; rejection requires
`REJECT POLISHED`. Piped input, a model process, or an unattended script cannot
supply that confirmation. Decisions are immutable, content-addressed sidecars
bound to the exact candidate file hash and exact manuscript-body hash. Repeating
the same decision, approver, and notes returns the same ID. Altered candidate
bytes make every prior decision stale and fail inspection. Export accepts only
the latest polished decision, so a later rejection revokes an earlier approval.

The conductor exposes these operations as actions `p` (polished decision) and
`x` (export) for its explicitly selected final-draft candidate. The standalone
commands below provide the same boundary for other callers.

An approved decision ID authorizes a separate explicit export:

```sh
blogctl candidate --workdir "$HOME/.local/share/writing-lab" export \
  EXPERIMENT_ID CANDIDATE_ID \
  --approval POLISHED_ID \
  --destination /private/manuscript/chapter.md
```

Export writes only the exact candidate body. It performs no generation,
rewriting, source selection, stage transition, commit, push, or destination
layout inference. The destination parent must already exist. Parent traversal,
symlinks, Nix-store paths, managed `.writing` paths, and an existing file with
different bytes are refused. Repeating the export to an existing file with the
same bytes succeeds without replacing it. Destination and
manuscript layout policy remain consumer-owned.

## Relationship to drafts and stages

- Experiments/candidates do not require `blogctl init`, post stage directories,
  `.blog-os.toml`, or a `jj` repository. A chapter's book is not a post stage.
- Existing `draft`, `refine`, and `final-edit` commands retain their current
  behavior: they edit post bodies. Nothing here changes their semantics.
- #233's proposed `.draft-N.md` output is not a second artifact store to build.
  Provider/stage work that needs immutable alternatives should reuse this
  candidate primitive, with an explicit prompt snapshot. Stage exit predicates
  still belong to `promote`, not candidate preparation/import.
- These commands never auto-commit, push, promote, or write to Notebooks/Dropbox,
  even if the surrounding blog working directory enables sync. A later explicit
  VCS action can include `.writing`; keep the whole working directory private.
- Review outside the canonical manuscript. `candidate review` appends a separate
  hash-bound editorial decision. `candidate polished` is the stronger,
  terminal-authorized final-draft decision required by `candidate export`. Do not
  mistake candidate existence, ordinary acceptance, or model output for polished
  approval.

## Validation

From `apps/blogctl` inside its Nix devshell:

```sh
cargo test --test writing
just validate
```

Real fixture files use only synthetic ideas and fake provider metadata. Tests
cover deterministic variants, exact provenance/snapshot hashes, no overwrite,
private permissions, path safety, candidate uniqueness, polished approval,
stale hashes, idempotent no-clobber export, CLI behavior without credentials,
and byte-for-byte compatibility of existing post/config files.
No new live API tests or generation behavior are introduced by this issue.
