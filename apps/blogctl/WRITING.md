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
      "requires_input": false,
      "approach": "Return only a compact outline."
    },
    {
      "name": "draft",
      "requires_input": true,
      "approach": "Draft from the selected outline.",
      "context": ["profile", "samples"]
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
  hash-bound decision; incorporation remains manual. Do not mistake a candidate's
  existence or acceptance for manuscript promotion, or rewrite its provenance.

## Validation

From `apps/blogctl` inside its Nix devshell:

```sh
cargo test --test writing
just validate
```

Real fixture files use only synthetic ideas and fake provider metadata. Tests
cover deterministic variants, exact provenance/snapshot hashes, no overwrite,
private permissions, path safety, candidate uniqueness, CLI behavior without
credentials, and byte-for-byte compatibility of existing post/config files.
No new live API tests or generation behavior are introduced by this issue.
