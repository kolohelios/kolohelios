# flake-nix-no-home fixture

Infra project whose `flake.nix` consumes `kolohelios-nix` via the canonical
FlakeHub URL but declares **no** `home`/`home-env` input — the shape of an
external consumer (e.g. buzzingo). Audit must pass: `kolohelios-nix-via-flakehub`
passes and `kolohelios-home-via-flakehub` is N/A (not flagged).
