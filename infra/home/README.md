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

GC has two layers on this host: the reactive `min-free`/`max-free`
above (catches catastrophic disk-full mid-build), and a proactive
weekly `launchd.daemons.nix-gc` (also in `modules/darwin.nix`, per
#664) that runs `nix-store --gc --delete-older-than 7d` Sundays at
03:00, logging to `/var/log/nix-gc.log`. macOS `launchd` doesn't make
up missed runs on wake, so a sleeping Mac skips that week's GC —
`min-free` remains the safety net in that case.

## Local zsh extensions

The generated `~/.zshrc` is a symlink into the Nix store. Tool-specific
PATH additions that aren't worth nix-managing (for example, `lmstudio`)
live in `~/.zshrc.local`, which the generated zshrc sources at the end
if it exists. `~/.zshrc.local` is hand-maintained, not nix-managed.

## Linux desktop: Pi and WezTerm

`modules/common.nix` installs `pi-coding-agent` on both platforms and
WezTerm on Linux. The pinned `nixpkgs` currently supplies Pi 0.75.4 and
WezTerm `0-unstable-2026-03-31`. The desktop's old system WezTerm
`20240203` rendered text as blocks; the newer build works with OpenGL.
The shared `wezterm.lua` selects OpenGL on Linux and leaves macOS on its
default renderer. Copy/paste uses `Ctrl+Shift+C` / `Ctrl+Shift+V`.

The NixOS module targets the devbox account `jon`. The standalone
`homeConfigurations.jone` profile targets the physical desktop and uses
its own pinned packages, independently of the host's older NixOS release.
Build and activate it from the repository root:

```sh
nix build ./infra/home#homeConfigurations.jone.activationPackage
HOME_MANAGER_BACKUP_EXT=backup ./result/activate
```

Existing managed files are preserved with a `.backup` suffix. Review
those backups after activation. Start a new terminal for the managed shell
configuration. This does not change `/etc/nixos/configuration.nix` or
install system device permissions.

## MSI Vector keyboard backlight (opt-in)

The Vector GP66HX 12UGS has a SteelSeries KLC controller (`1038:113a`).
OpenRGB 0.9 did not detect it. `packages/msi-perkeyrgb.nix` pins the
upstream tool, fixes Nix library/tool discovery and the feature-report
`size_t` binding, and supplies a narrowly scoped `uaccess` rule.
The GS65 keyboard layout and purple `a020f0` were tested on this laptop.

`common.nix` imports the keyboard module but leaves it disabled. On the
physical desktop, once Home Manager is wired into NixOS:

```nix
# In the desktop's NixOS module (paths relative to this directory):
imports = [ ./modules/msi-keyboard-udev.nix ];
home-manager.users.jone.programs.msi-perkeyrgb.enable = true;
```

The optional NixOS module grants the active local user access to this
controller; Home Manager installs the CLI and applies purple at graphical
login. Macs and cloud devboxes are unaffected. The generated `udev` rule
is specific to `1038:113a`; overriding `usbId` requires a matching host
permission rule too. Rebuild NixOS and reboot to apply device permissions.

Manual color changes (may replace the controller's stored lighting):

```sh
msi-perkeyrgb --id 1038:113a --model GS65 --steady a020f0
```

Hardware brightness is separate: use the laptop's keyboard-backlight
brightness keys if the color is too dim. The power-button LED is not
reliably controlled by this tool. No OpenRGB daemon, root GUI, or extra
I2C access is needed for this USB controller.

## Adding a tool

1. If it's a binary, add to `home.packages` in `modules/common.nix`.
2. If home-manager has a generator for it (for example,
   `programs.helix`), wire it via the `programs.*` option set.
3. If the upstream configuration is too elaborate to express in nix,
   drop the source file under `dotfiles/<tool>/` and reference it via
   `xdg.configFile` or `home.file`.
