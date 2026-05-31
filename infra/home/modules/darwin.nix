{ claude-hooks, ... }:

{
  system.stateVersion = 5;

  nixpkgs.hostPlatform = "aarch64-darwin";
  nixpkgs.config.allowUnfree = true;

  # Determinate Nix manages the daemon and store on this host; nix-darwin
  # aborts activation otherwise. With `nix.enable = false`, every `nix.*`
  # option (gc, optimise, settings, linux-builder, experimental-features,
  # trusted-users) becomes unreachable here and must be configured on the
  # Determinate side instead — see `/etc/nix/nix.custom.conf` and
  # `determinate-nixd`. Tracked in #631.
  nix.enable = false;

  users.users.jedwards = {
    name = "jedwards";
    home = "/Users/jedwards";
  };

  programs.zsh.enable = true;

  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.extraSpecialArgs = { inherit claude-hooks; };
  home-manager.users.jedwards = import ./common.nix;

  # First-switch activation will encounter pre-existing dotfiles
  # (`~/.zshrc`, `~/.config/jj/config.toml`, etc.) that home-manager
  # refuses to clobber. With `backupFileExtension`, the existing file
  # is renamed to `<path>.backup` and home-manager's symlink takes its
  # place — safer than `force = true` on individual files and survives
  # future drift the same way. Review and delete `.backup` files after
  # activation. Tracked in #633.
  home-manager.backupFileExtension = "backup";
}
