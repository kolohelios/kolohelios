# Desktop writing integration

The desktop enables `modules/writing-lab.nix`. The `blogctl` input is pinned to a
published FlakeHub release with review, migration, and read-only MCP support;
the stable profile uses that published package. When changing engine command
semantics and launchers together, publish and pin the matching engine before
normal activation. Use the local-package validation profile below until then.

For future engine changes, publish the engine first, then update the pin inside
`infra/home`:

```sh
nix flake update blogctl
```

Build and activate using the normal commands in [README.md](README.md). Keep
`programs.writing-lab.enable = true` in the desktop module; no local/path input
should be committed.

The enabled module installs `blogctl`, `blogctl-novelai`, the 1Password CLI, and
Codex. Activation creates `~/.local/share/blogctl-writing` privately and merges
only the owned `writing` MCP registration, removing the recognized prototype
entry. Other MCP configuration is preserved. It never migrates data, copies a
token, or requests generation during activation.

## Validation before publication

From this repository's root, explicitly supply the local engine to the enabled
module's test expression:

```sh
nix build --impure --expr "
  let
    root = builtins.toPath \"$PWD\";
    homeFlake = builtins.getFlake (toString root + \"/infra/home\");
    writing = builtins.getFlake (toString root + \"/apps/blogctl\");
  in import (root + \"/infra/home/tests/writing-desktop.nix\") {
    inherit homeFlake;
    writingPackage = writing.packages.x86_64-linux.default;
  }
" --out-link /tmp/writing-home
HOME_MANAGER_BACKUP_EXT=backup /tmp/writing-home/activate
```

This declarative validation profile uses the local package only for that build
and does not change the published package pin. The normal desktop profile uses
the pinned FlakeHub package. No privileged system changes are needed.

## NixOS desktop-app authentication

Installing the desktop app and the Home Manager CLI package is not sufficient
for their secure connection. Enable the NixOS CLI module in the host's system
configuration, not in Home Manager:

```nix
programs._1password.enable = true;
```

On this machine the host configuration is `/etc/nixos/configuration.nix`. After
adding that option, apply it locally with administrator privileges:

```sh
sudo nixos-rebuild switch
```

This creates the protected `/run/wrappers/bin/op` with the required group
permissions. Keep **Settings → Developer → Integrate with 1Password CLI** enabled
in the desktop app, unlock it, and approve its local authorization prompt.
Restart the app after the rebuild if necessary. A separate `op account add` or
shell-session login is not required for desktop integration.

The writing launchers explicitly prefer the protected system wrapper on NixOS.
If it is missing, they fail with the required system option rather than silently
using an unwrapped store binary. Home Manager cannot create that privileged
wrapper. Do not manually change executable permissions or copy the binary.
Outside NixOS, the launchers use the packaged CLI normally. An explicitly supplied
process-scoped token still works, and help/previews need no authentication.

## Private setup

Follow the engine's [integration guide](../../apps/blogctl/INTEGRATIONS.md)
for explicit copy-only migration and manual review/promotion into Notebooks.
The old `novelai-lab` command is intentionally replaced, not aliased.

Only one local credential reference is needed after authenticating `op`:

```sh
umask 077
mkdir -p ~/.config/blogctl
printf '%s\n' 'op://YOUR_VAULT/NovelAI/credential' > ~/.config/blogctl/novelai-token-reference
```

Use the actual field reference, never the token value. No secret is stored in
Nix, MCP configuration, a second keyring, manuscript files, or Dropbox.

```sh
blogctl-novelai models
blogctl novelai generate EXPERIMENT_ID --workdir ~/.local/share/blogctl-writing \
  --variant workshop --model MODEL --dry-run
blogctl-novelai generate EXPERIMENT_ID --workdir ~/.local/share/blogctl-writing \
  --variant workshop --model MODEL
```

Only the final command requests generation. The explicit `generate` command is
the action: there is no extra flag, typed word, or terminal requirement. Use
`--dry-run` for a credential-free preview. MCP startup also needs no 1Password
access; its tools remain read-only.
Restart Codex or reload Pi after activation. `pi mcp list --json` should show the
five read-only tools under `writing`; no generation/review tool is registered.
If registration reports a conflict or symlink-managed configuration, inspect the
local source of that configuration rather than replacing the whole file.
