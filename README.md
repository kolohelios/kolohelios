# kolohelios

Personal monorepo for infrastructure, tooling, and projects.

## Layout

| Directory | Purpose |
|-----------|---------|
| `apps/` | Applications |
| `packages/` | Shared libraries and packages |
| `projects/` | Standalone projects |
| `tools/` | Developer tooling (e.g. shaka CLI) |
| `infra/` | Infrastructure as code (Terraform, NixOS) |

## Getting started

Prerequisites: [Nix](https://nixos.org/) with flakes enabled.

```sh
# Enter the dev shell (or use direnv)
nix develop

# Run tasks
just <recipe>
```

## Tenets

### Devboxes are ephemeral

Development environments — whether baremetal Macs or cloud VMs — are disposable
workspaces. They don't persist and aren't durable. The durable artifacts are:

- The **code repository**
- **Flake caches** and binary substituters
- **Deployed services**

Work flows through GitHub Issues: created, picked up, completed. Configuration
changes must be made in code. Shell history is backed up to object storage, but
nothing else on a devbox is expected to survive. The dev environment improves
incrementally, and the cost to rebuild should stay low.

### Secrets live in 1Password

1Password is the canonical secret store — for local development (`op` CLI), CI
(GitHub Actions integration), and future infrastructure (VM service accounts,
Kubernetes ExternalSecrets). Secrets are never committed to the repo.

### Version control conventions

- **Jujutsu (jj)** for all version control operations
- **Conventional commits**: `<type>(<scope>): <subject>` (max 70 chars, declarative language)
- Title answers "why", body answers "what"
- Commits are atomic (single logical change) and vertical (one layer/concern)

### Command runner

`just` is the standard task runner. Root justfile for cross-project tasks,
per-project justfiles for project-specific recipes.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

This is a personal monorepo and is not actively soliciting contributions.
The dual-license declaration applies to the contents regardless.
