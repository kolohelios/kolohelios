# `aof`

Areas of focus — a typed tree of life domains, reconciled against
`Todoist` projects and rendered as an inline terminal diagram.

The tree is defined in CUE under `data/`, so a malformed structure
fails locally and in CI. `aof render` produces an SVG via the `d2`
binary, converts it to a bitmap in-process with `resvg`, and emits
it using the Kitty graphics protocol (`Ghostty` / `Kitty` /
`WezTerm`).

`Todoist` reconciliation reports drift in both directions: projects
with no matching area, and areas with no `Todoist` project. The match
is read-only — `aof` never mutates `Todoist` state.

## Subcommands

- `aof validate` — vet the areas tree against the schema.
- `aof sync` — reconcile against `Todoist` and print a drift report.
- `aof render --from <path>` — display an SVG inline via the Kitty
  graphics protocol. Tree-to-SVG generation lands in #608; today this
  takes a pre-rendered SVG path.

`validate` and `sync` are still stubbed; real behavior lands across
the remaining `aof` issues.
