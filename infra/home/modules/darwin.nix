{
  claude-hooks,
  kolohelios-nix,
  shaka,
  ...
}:

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
  home-manager.extraSpecialArgs = { inherit claude-hooks kolohelios-nix shaka; };
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
      "claude-code@latest"
      # `pkgs.wezterm` is fragile on aarch64-darwin (the GUI build pulls a
      # heavy toolchain nixpkgs struggles to build reliably), so the Mac
      # install rides the same cask path as `claude-code`. Linux installs
      # `pkgs.wezterm` from nixpkgs instead (see `common.nix`). The wezterm
      # config is managed via `xdg.configFile` in `common.nix` on both.
      "wezterm"
    ];
  };

  # Determinate writes `/etc/nix/nix.conf` itself (header says "do not
  # modify"), but the user-editable overlay it `!include`s,
  # `/etc/nix/nix.custom.conf`, is fair game. `nix.*` options are
  # unreachable via the gate (#631), but `environment.etc` is a
  # different module entirely and works fine — gives us a declarative
  # home for what would otherwise have been `nix.settings.*`. The
  # daemon auto-GCs mid-build when free space drops below `min-free`
  # and reclaims up to `max-free`, which is the load-bearing fix for
  # the 30–60 GB/hour accumulation symptom from #573. Tracked in #632.
  environment.etc."nix/nix.custom.conf".text = ''
    min-free = ${toString (10 * 1024 * 1024 * 1024)}
    max-free = ${toString (50 * 1024 * 1024 * 1024)}
    auto-optimise-store = true
  '';

  # Proactive companion to the reactive `min-free`/`max-free` settings
  # above. `min-free` only fires when disk pressure hits, so dead
  # dev-shell generations from the daily `kolohelios-nix` bump can sit
  # rooted-then-unrooted indefinitely if free space stays above the
  # threshold. This `launchd` job runs `nix-store --gc` weekly
  # (Sundays 03:00) with a 7-day retention bound. launchd lives outside
  # nix-darwin's `nix.*` namespace so the Determinate gate (#631)
  # doesn't reach it. macOS doesn't make up missed runs on wake — if
  # the Mac's asleep at 03:00 the week's GC just skips, and
  # `min-free` remains the safety net. Tracked in #664.
  launchd.daemons.nix-gc = {
    command = "/nix/var/nix/profiles/default/bin/nix-store --gc --delete-older-than 7d";
    serviceConfig = {
      StartCalendarInterval = [
        {
          Weekday = 0;
          Hour = 3;
          Minute = 0;
        }
      ];
      StandardOutPath = "/var/log/nix-gc.log";
      StandardErrorPath = "/var/log/nix-gc.log";
      RunAtLoad = false;
    };
  };
}
