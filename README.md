# kolohelios

Personal monorepo for infrastructure, tooling, and projects. Public so the
shared tooling can be referenced freely; not actively soliciting external
contributions.

## Layout

Top-level directories are **slots**; each slot contains one directory per
project. Every project has its own `flake.nix` — there is no root flake.

| Slot         | Purpose                                              |
| ------------ | ---------------------------------------------------- |
| `apps/`      | End-user applications                                |
| `packages/`  | Shared libraries                                     |
| `projects/`  | Standalone projects that don't fit another slot      |
| `services/`  | Long-running services (reserved; not yet populated)  |
| `tools/`     | Developer tooling (for example, `tools/shaka`)       |
| `infra/`     | Infrastructure as code (for example, `infra/devbox`) |
| `nix/`       | Shared nix infrastructure (for example, `nix/kolohelios-nix`) |

## Getting started

Prerequisites: [Nix](https://nixos.org/) with flakes enabled, and
[`direnv`](https://direnv.net/) for automatic environment loading.

```sh
# Each project has its own dev shell. cd into it; direnv loads the flake.
cd tools/shaka
just <recipe>      # build, test, lint, validate, ...
```

To run validation across the whole repo (the same command CI runs):

```sh
shaka preflight
# or, scoped to changes since a ref:
shaka preflight --since main@origin
```

`shaka` is on `$PATH` inside any project's devshell (via a shim in
`kolohelios-nix.lib.workflowPackages`); from outside a devshell, the
wrapper at `tools/shaka/bin/shaka` is the cold-start escape hatch.

## Build system

- **Per-project flakes**, with [`nix/kolohelios-nix`](nix/kolohelios-nix)
  as a shared lib. Consumers reference it via FlakeHub so they can be
  evaluated outside this working tree.
- **`just`** as the per-project task runner. Per-project `justfile`s are
  generated from `project.cue` by `shaka project generate-justfiles` —
  don't hand-edit them; CI fails on drift.
- **`shaka`** ([`tools/shaka`](tools/shaka)) is the build/repo command-line tool:
  - `shaka preflight` — runs every CI check locally; CI runs the same
    command, so local and CI cannot drift.
  - `shaka project schema-check|lint|generate-justfiles` — project
    metadata tooling.
  - `shaka commit lint` — conventional-commit and atomicity enforcement.
  - `shaka whitespace check|fix` — cross-language hygiene.
  - `shaka repo sync|send|status` — `jj`/PR workflow helpers.
  - `shaka workspace` — sibling `jj` working copies for parallel sessions.

## Tenets

- **Devboxes are ephemeral.** Local devboxes (baremetal Mac, cloud VM)
  are disposable workspaces, not durable infrastructure. The durable
  artifacts are the code repo, the FlakeHub-published flakes, and the
  deployed services. Configuration changes live in version control.
- **Secrets live in 1Password.** Canonical for local development (`op`
  command-line tool), CI (GitHub Actions integration), and infrastructure.
  Never committed to the repo.
- **Version control via [`Jujutsu`](https://github.com/jj-vcs/jj) (`jj`)**
  on a colocated git repo. Conventional commits (`<type>(<scope>):
  <subject>`, max 70 chars), enforced by `shaka commit lint`. Atomic,
  vertical commits — one logical change per commit.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
