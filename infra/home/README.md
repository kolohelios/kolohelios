# infra/home

Cross-platform home environment — single source of truth for the user's
shell, editor, multiplexer, version-control, and CLI toolchain across
`aarch64-darwin` (full nix-darwin) and `x86_64-linux` (NixOS module
intended for `infra/devbox`).

## Outputs

- `darwinConfigurations.Jons-MacBook-Pro` — full nix-darwin system for
  the laptop.
- `nixosModules.home` — NixOS module wiring home-manager for the
  devbox user. Exposed but **not yet consumed** by `infra/devbox`:
  cross-flake `path:` inputs fail in pure-eval mode, and the standard
  fix (FlakeHub publish + FlakeHub-URL consumption, mirroring
  `kolohelios-nix`) is tracked in #273.

## Switching the macOS system

```
nix run nix-darwin -- switch --flake ./infra/home#Jons-MacBook-Pro
```

After the first switch, subsequent rebuilds use:

```
darwin-rebuild switch --flake ./infra/home#Jons-MacBook-Pro
```

## Local zsh extensions

The generated `~/.zshrc` is a symlink into the Nix store. Tool-specific
PATH additions that aren't worth nix-managing (Windsurf, nvm, lmstudio,
etc.) live in `~/.zshrc.local`, which the generated zshrc sources at
the end if it exists.

## Adding a tool

1. If it's a binary, add to `home.packages` in `modules/common.nix`.
2. If home-manager has a generator for it (for example,
   `programs.helix`), wire it via the `programs.*` option set.
3. If the upstream configuration is too elaborate to express in nix
   (for example, the zellij keybind tree), drop the source file under
   `dotfiles/<tool>/` and reference it via `xdg.configFile`.
