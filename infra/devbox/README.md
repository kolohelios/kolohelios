# devbox

Infrastructure for the personal devbox: a NixOS-on-Linode VM provisioned by
Terraform and configured by a NixOS module evaluated from this flake.

## Layout

- `nixos/configuration.nix` — the running system definition (services,
  users, packages).
- `nixos/image.nix` — the variant of the system used to bake a
  Linode-compatible disk image.
- `nixos/hardware.nix` — generic hardware bits shared by both.
- `terraform/` — the Linode resources (instance, networking) and the
  variables file template (`terraform.tfvars.example`).

## Building

```
nix develop . --command just validate
```

runs the same fmt/lint/flake-check steps CI runs for this project (via
`shaka preflight`). To build the image artifact directly:

```
nix build .#image
```

## Provisioning

Copy `terraform/terraform.tfvars.example` to `terraform/terraform.tfvars`,
fill in the secrets from 1Password, and run `tofu apply` from the
`terraform/` directory. The plan stage is also covered by
`just validate`.
