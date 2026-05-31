{ claude-hooks, ... }:

{
  system.stateVersion = 5;

  # Required by nix-darwin's `homebrew` module (below) and any other
  # user-scoped option in the modern multi-user model — activation runs
  # as root but these options apply to this user.
  system.primaryUser = "jedwards";

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

  # Declarative brew-cask install via nix-darwin's Brewfile emission.
  # Reachable even with `nix.enable = false` because `homebrew.*` lives
  # outside the `nix.*` namespace Determinate gates. Casks that get
  # carved out of `pkgs.*` for the "fast-moving upstream, nixpkgs lags"
  # reason (per #652) come here so they're still declared in version
  # control. Conservative `onActivation` defaults: ensure listed casks
  # are installed; don't touch anything else. Bump to
  # `cleanup = "uninstall"` later if full declarative parity is wanted.
  homebrew = {
    enable = true;
    onActivation = {
      autoUpdate = false;
      upgrade = false;
      cleanup = "none";
    };
    casks = [
      "claude-code"
    ];
  };
}
