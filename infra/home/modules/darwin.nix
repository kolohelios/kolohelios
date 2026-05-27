{ claude-hooks, ... }:

{
  system.stateVersion = 5;

  nixpkgs.hostPlatform = "aarch64-darwin";
  nixpkgs.config.allowUnfree = true;

  nix.settings.experimental-features = [
    "nix-command"
    "flakes"
  ];

  nix.linux-builder.enable = true;
  # Required so non-root users can dispatch to the linux-builder VM.
  nix.settings.trusted-users = [ "@admin" ];

  # Per-project flakes + the daily `kolohelios-nix` lock bump means every
  # project's old dev shell becomes GC-able after the next direnv reload,
  # easily piling up tens of GB per day. `min-free`/`max-free` is the
  # load-bearing pair: the daemon auto-GCs mid-build when free space drops
  # below `min-free` and reclaims up to `max-free`, so the store doesn't
  # grow unbounded between manual sweeps. The daily timer + 7d retention
  # bounds long-tail roots; `optimise.automatic` deduplicates identical
  # store paths via hardlinks.
  nix.gc = {
    automatic = true;
    interval.Hour = 3;
    options = "--delete-older-than 7d";
  };
  nix.optimise.automatic = true;
  nix.settings.min-free = 10 * 1024 * 1024 * 1024;
  nix.settings.max-free = 50 * 1024 * 1024 * 1024;

  users.users.jedwards = {
    name = "jedwards";
    home = "/Users/jedwards";
  };

  programs.zsh.enable = true;

  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.extraSpecialArgs = { inherit claude-hooks; };
  home-manager.users.jedwards = import ./common.nix;
}
