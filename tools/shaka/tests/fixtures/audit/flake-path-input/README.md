# flake-path-input fixture

Infra project whose `flake.nix` pins `kolohelios-nix` via a `path:` input
instead of the canonical FlakeHub URL. Audit must fail with the
`kolohelios-nix-via-flakehub` rule.
