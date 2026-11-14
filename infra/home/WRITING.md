# Optional writing integration

The desktop imports `modules/writing-lab.nix`, but its enable option defaults to
false. The `blogctl` input consumes the existing FlakeHub package. The initial
lock contains the generation-only release, not the new MCP commands. Do not
turn the option on against that older package.

After the engine changes publish, update the pin inside `infra/home`:

```sh
nix flake update blogctl
```

Then set `programs.writing-lab.enable = true` in `modules/desktop.nix`, build and
activate using the normal commands in [README.md](README.md). The pin and flag
are the final release-order step; no local/path input should be committed.

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

This is a declarative validation profile, not a claim that the published pin
already contains the new commands. It uses the local package only for that
build; the normal source profile stays safely off until publication and the
pin/flag update. Rebuilding the ordinary profile before that step will not
reinstall the optional tools. No privileged system changes are needed.

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
  --variant workshop --model MODEL
blogctl-novelai generate EXPERIMENT_ID --workdir ~/.local/share/blogctl-writing \
  --variant workshop --model MODEL --run
```

Only the final command requests generation, and it still requires fresh terminal
confirmation. Local previews and MCP startup do not need 1Password access.
Restart Codex or reload Pi after activation. `pi mcp list --json` should show the
five read-only tools under `writing`; no generation/review tool is registered.
If registration reports a conflict or symlink-managed configuration, inspect the
local source of that configuration rather than replacing the whole file.
