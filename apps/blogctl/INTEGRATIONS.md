# Review, agents, and prototype migration

These integrations build on [experiments](WRITING.md) and
[explicit NovelAI generation](NOVELAI.md). No review or agent call promotes prose
into a manuscript.

## Human review

Read the candidate, then record a decision:

```sh
blogctl candidate --workdir /private/writing-lab read EXPERIMENT_ID CANDIDATE_ID
blogctl candidate --workdir /private/writing-lab review EXPERIMENT_ID CANDIDATE_ID \
  --decision accept --reviewer 'Your name' --notes-file /private/review-notes.txt
blogctl candidate --workdir /private/writing-lab reviews EXPERIMENT_ID CANDIDATE_ID
```

Use `--decision reject` to reject it; the notes file is optional. You must type
`ACCEPT` or `REJECT` at the controlling terminal for that invocation. Piped input
is not approval. As with generation, terminal confirmation is an accident guard,
not a sandbox against another process with full user permissions.

Decisions are separate immutable JSON records under
`.writing/reviews/<experiment>/<candidate>/<review-id>.json` (0400, private
parent directories). Each records reviewer, decision, notes, UTC time, IDs, and
SHA-256 of the **entire candidate file**, including its provenance and snapshot.
A changed file invalidates its reviews. Subsequent decisions append history;
none overwrite earlier records. The candidate's original `unreviewed` status
remains unchanged: consult review history, not that initial status, for decisions.

Acceptance means suitable for consideration, not approved autobiography and not
permission to publish. To incorporate material into Notebooks:

1. Review the fictional candidate against your original argument and actual
   experience. Reject invented professional incidents, weak analogies, and
   generic prose.
2. After acceptance, manually create a separate supporting note in the existing
   Notebooks/Dropbox book, not directly in the canonical chapter. Copy only the
   approved excerpts, not the private request snapshot by accident.
3. Retain provider/model, source candidate ID and hash, review ID/date, provenance,
   and the human decision in that note's metadata. Then edit and incorporate
   deliberately through the normal iterative manuscript process.
4. Keep the original candidate and review records unchanged outside Dropbox.
   Nothing here discovers Dropbox paths or writes to Notebooks.

## Read-only MCP

```sh
blogctl mcp serve --workdir /private/writing-lab
```

The five tools are:

| Tool | Arguments | Effect |
| --- | --- | --- |
| `writing_list_experiments` | none | List IDs locally |
| `writing_prepare_variant` | `experiment`, `variant` | Preview a local prompt |
| `writing_list_candidates` | `experiment` | List candidate IDs locally |
| `writing_read_candidate` | `experiment`, `candidate` | Read text and hash-checked reviews |
| `novelai_list_models` | none | One authenticated model-discovery GET |

No generation, review, migration, installation, or publishing tools exist.
Unknown tools and extra arguments such as `authorized: true` are rejected.
Annotations advertise read-only behavior, but the closed dispatch table enforces
it. Standard input/output carry bounded line-delimited JSON-RPC only, with no
prompt logging or debug output. Tool errors suppress local/private error details.

Pi 1.4.0's native MCP support has no mandatory per-call confirmation by default.
Extensions can gate calls, but configuration can change. Current Codex supports
`default_tools_approval_mode` and per-tool `approval_mode`; those settings are
also mutable. Neither is used as a claim of reliable human generation approval.
The common integration therefore remains read-only, with generation on the
explicit terminal-confirmed CLI path.

### Client registration

Register an absolute executable and explicit private working directory:

```sh
blogctl mcp install --home "$HOME" --command /absolute/path/to/blogctl \
  --workdir /private/writing-lab
```

This merges only the owned `writing` entry in `~/.pi/agent/mcp.json` and
`~/.codex/config.toml`. It preserves unrelated servers/settings and TOML comments,
refuses conflicting entries or symlink-managed files, and validates both complete
documents before replacing either. Repeated registration is safe; replacements
are atomic per file, not a transaction across both files. Avoid simultaneous
manual configuration edits. A failed second replacement can be repaired by
rerunning installation.

Only the recognized Nix-store `novelai-lab mcp` prototype entry is removed;
other entries named `novelai` remain untouched. Server name/tool names intentionally
change to the writing-oriented interface above. No credentials go in MCP settings.

Restart Codex and use `/reload` in Pi, then check `pi mcp list --json` and
`codex mcp list`. Pi listing tests a live connection; Codex listing verifies
registration, not a paid agent session or an actual tool invocation.

## 1Password boundary and Home Manager

The optional module `infra/home/modules/writing-lab.nix` installs `blogctl`,
`blogctl-novelai`, `op`, and Codex, and registers the MCP server. It fixes the
local MCP data root at `~/.local/share/blogctl-writing`. The engine still does
not resolve credentials.

Save only a 1Password field reference, never its value:

```sh
umask 077
mkdir -p ~/.config/blogctl
printf '%s\n' 'op://YOUR_VAULT/NovelAI/credential' > ~/.config/blogctl/novelai-token-reference
```

Authenticate `op` locally. `blogctl-novelai models` and generation with `--run`
use `op run` at invocation time; previews do not access credentials. Pass the same
arguments as `blogctl novelai`, including the explicit `--workdir` for generation.
`NOVELAI_API_TOKEN_REF` can override the reference file. Existing process-scoped
`NOVELAI_API_TOKEN` is supported. No token is copied into a keyring or local file.

MCP model discovery uses a trusted helper configured through `--models-command`.
It resolves credentials only for the discovery call and returns a JSON model
array using `novelai models --json`. The helper never passes its resolved credential back to the parent, and local
tools/startup work while 1Password is unavailable.
The helper's errors are captured and suppressed by the adapter. The executable
path is trusted configuration, never a tool argument. The helper is not a general
shell-command MCP tool.

**Release ordering:** the module is opt-in and disabled in the normal desktop
profile until a published FlakeHub `blogctl` includes these MCP commands. Update
`infra/home`'s `blogctl` lock after that publication, then enable
`programs.writing-lab.enable = true` in the desktop module and activate normally.
Do not enable against the older generation-only package. The enabled module can
be built before publication with the explicit local-package test expression in
`infra/home/tests/writing-desktop.nix`; see the home guide. This avoids a broken
default activation or a permanent local flake input.

The Python prototype is not installed by the new module. There is no
`novelai-lab` compatibility alias; use `blogctl` and `blogctl-novelai` instead.
Do not discard the old private files until migration and the new commands are
verified. The prototype workspace was never merged and is not a second maintained engine.

## Explicit copy-only migration

```sh
install -dm700 "$HOME/.local/share/blogctl-writing"
blogctl experiment --workdir "$HOME/.local/share/blogctl-writing" migrate-novelai-lab \
  --experiment-file "$HOME/.config/novelai-lab/experiment.json" \
  --runs-dir "$HOME/.local/share/novelai-lab/runs"
```

Only the supplied paths are read. The original configuration is archived,
byte-for-byte, under `.writing/legacy/novelai-lab/experiment.json`, and a native
chapter experiment is created with the original ID/seed and the prototype's
three fictional approaches/style. Original files are not deleted or changed.

Each existing run's name, content, hashes, provenance, and initial review status
are preserved verbatim in the candidate directory. Legacy prompt/seed hashes
are checked against their original request snapshot; conflicting identities,
corrupt snapshots, links, or differing destination bytes cause failure, not an
overwrite. Same-byte destinations make a rerun safe after a partial migration.
No existing run is accepted or promoted during migration. New previews use the
native prompt format; old snapshots and hashes are never rewritten to match it.
A real token/reference file is not read or migrated by the engine.
