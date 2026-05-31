# `aof`

Areas of focus — a typed tree of life domains, reconciled against
`Todoist` projects and rendered as an inline terminal diagram.

The tree is defined in CUE under `data/`, so a malformed structure
fails locally and in CI. `aof render` produces an SVG via the `d2`
binary, converts it to a bitmap in-process with `resvg`, and displays
it using the terminal's inline image protocol (Kitty / iTerm2).

`Todoist` reconciliation reports drift in both directions: projects
with no matching area, and areas with no `Todoist` project. The match
is read-only — `aof` never mutates `Todoist` state.

## Subcommands

- `aof validate` — vet the areas tree against the schema.
- `aof sync` — reconcile against `Todoist` and print a drift report.
- `aof render` — emit an inline diagram of the tree.

This crate is currently a skeleton — each subcommand is stubbed.
Real behavior lands across follow-up issues (#605–#610).
