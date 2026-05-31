# infra/home

Cross-platform home environment — single source of truth for the user's
shell, editor, version-control, and CLI toolchain across
`aarch64-darwin` (nix-darwin, daemon ceded to Determinate per #631) and
`x86_64-linux` (NixOS module intended for `infra/devbox`).

## Outputs

- `darwinConfigurations.Jons-MacBook-Pro` — nix-darwin system for the
  laptop. `nix.enable = false` so nix-darwin coexists with Determinate
  Nix; all `nix.*` settings on this host live in `/etc/nix/nix.custom.conf`
  and `determinate-nixd` instead.
- `nixosModules.home` — NixOS module wiring home-manager for the
  devbox user. Exposed but **not yet consumed** by `infra/devbox`:
  cross-flake `path:` inputs fail in pure-eval mode, and the standard
  fix (FlakeHub publish + FlakeHub-URL consumption, mirroring
  `kolohelios-nix`) is tracked in #273.

## Switching the macOS system

System activation requires `sudo`:

```
sudo nix run nix-darwin -- switch --flake ./infra/home#Jons-MacBook-Pro
```

After the first switch, subsequent rebuilds use:

```
sudo darwin-rebuild switch --flake ./infra/home#Jons-MacBook-Pro
```

On first switch, any pre-existing `dotfile` that home-manager would
otherwise clobber is renamed to `<path>.backup`
(`home-manager.backupFileExtension = "backup"` in `modules/darwin.nix`).
Review and delete the `.backup` files once you've confirmed the
nix-managed version has everything you care about.

## Nix configuration on macOS

Determinate Nix manages the daemon and writes `/etc/nix/nix.conf`
itself (the file header literally says "do not modify"). The
user-editable overlay is `/etc/nix/nix.custom.conf`, which `nix.conf`
pulls in via `!include`.

Because nix-darwin's `nix.*` options are unreachable here (gated by
Determinate per #631), settings that would normally live there are
written via `environment.etc."nix/nix.custom.conf"` in
`modules/darwin.nix`. Today that's just GC tuning (`min-free` /
`max-free` / `auto-optimise-store` from #632); same pattern applies for
any future daemon setting.

If GC behavior needs another knob, edit `darwin.nix` and re-run the
`darwin-rebuild switch` from below. Don't hand-edit `nix.custom.conf` —
the next switch overwrites it.

## Local zsh extensions

The generated `~/.zshrc` is a symlink into the Nix store. Tool-specific
PATH additions that aren't worth nix-managing (for example, `lmstudio`)
live in `~/.zshrc.local`, which the generated zshrc sources at the end
if it exists. `~/.zshrc.local` is hand-maintained, not nix-managed.

## Adding a tool

1. If it's a binary, add to `home.packages` in `modules/common.nix`.
2. If home-manager has a generator for it (for example,
   `programs.helix`), wire it via the `programs.*` option set.
3. If the upstream configuration is too elaborate to express in nix,
   drop the source file under `dotfiles/<tool>/` and reference it via
   `xdg.configFile` or `home.file`.
