# `kolohelios-nix`

Shared nix utilities consumed by every project flake in this repo.
Centralizes the `nixpkgs` pin and the workflow-tool list so the entire
repo shares one `nixpkgs` revision and project closures hit the cache
together.

## What it exports

- `lib.supportedSystems` — the systems every consumer flake supports.
- `lib.forEachSupportedSystem` — helper that maps a function over every
  supported system, providing a system-specific `pkgs`.
- `lib.workflowPackages` — the workflow-tool list (`jujutsu`, `git`,
  `just`, `jq`, `cue`, `nixfmt-rfc-style`, `nil`, `typos`, `cargo-deny`,
  `cargo-machete`, `vale`, `_1password-cli`) that every project's
  devshell wants; consumers spread it and add project-specific tools on
  top.
- `formatter` — `nixfmt-rfc-style` per system, used by `nix fmt`.

## Usage from a consumer flake

```nix
{
  inputs = {
    kolohelios-nix.url = "path:../../nix/kolohelios-nix";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";
  };
}
```

The `nixpkgs.follows` line is what keeps the repo on a single `nixpkgs`
revision.

## Distribution

Published to FlakeHub by the `build-nix-lib` CI job. Pushed closures
back-fill the substituter for `path:` consumers too, since derivations are
content-addressed.
